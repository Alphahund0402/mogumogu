# syntax=docker/dockerfile:1

# Keep this version aligned with rust-toolchain.toml. scripts/check.py verifies it.
FROM rust:1.98.0-bookworm@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS toolchain

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt clippy

# An explicit override avoids installing the Windows target inside Linux.
ENV RUSTUP_TOOLCHAIN=1.98.0 \
    CARGO_HOME=/cache/cargo \
    CARGO_TARGET_DIR=/workspace/target \
    PYTHONDONTWRITEBYTECODE=1

RUN useradd --uid 10001 --create-home --shell /usr/sbin/nologin mogumogu \
    && mkdir -p /workspace/target /cache/cargo \
    && chown mogumogu:mogumogu /workspace/target /cache/cargo

WORKDIR /workspace

# Windows executables without a local Rust, Visual Studio or Windows SDK:
# the GNU toolchain (mingw-w64) cross-compiles the desktop build in Linux.
# The programs still run natively on Windows; Windows-only tests do not run
# here. Use: docker build --target windows-exe --output dist/mogumogu .
FROM toolchain AS windows
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-pc-windows-gnu --toolchain 1.98.0 \
    && mkdir /out && chown 10001:10001 /out
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
    AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
COPY . .
USER 10001:10001
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target-windows,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    cargo build --locked --release --bins --target x86_64-pc-windows-gnu \
    && cp target/x86_64-pc-windows-gnu/release/mogumogu.exe target/x86_64-pc-windows-gnu/release/mogumogu-cli.exe \
       LICENSE README.md THIRD_PARTY.md Cargo.lock /out/ \
    && cp assets/fonts/zen-maru-gothic/OFL.txt /out/FONT-LICENSE.txt

# Only the files for dist\mogumogu, nothing of the build environment.
FROM scratch AS windows-exe
COPY --from=windows /out/ /

# Source is copied into the image; checks never need a host filesystem mount.
FROM toolchain AS checks
COPY . .
USER 10001:10001
ENTRYPOINT ["python3", "scripts/check.py"]
CMD ["--core"]

# CI verifies during the build. Local Compose runs use the checks target and
# named volumes, so subsequent checks retain downloads and compilation output.
# As the last stage, this stays the default target of `docker build .`.
FROM checks AS validated
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    python3 scripts/check.py --core
