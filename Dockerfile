# syntax=docker/dockerfile:1

# Keep this version aligned with rust-toolchain.toml. scripts/check.py verifies it.
# The slim image already has gcc and the C library headers the bundled SQLite
# needs; nothing in the build links OpenSSL or asks pkg-config.
FROM rust:1.98.0-slim-bookworm@sha256:1469a27c125cb5a3aebfa4f4e4665d935b02fb72cc093b2c974b3d740e43f157 AS toolchain

# An explicit override avoids installing the Windows target inside Linux.
ENV RUSTUP_TOOLCHAIN=1.98.0 \
    CARGO_HOME=/cache/cargo \
    CARGO_TARGET_DIR=/workspace/target

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
ARG BUILD_PROFILE=release
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target-windows,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    cargo build --locked --profile "$BUILD_PROFILE" --bins --target x86_64-pc-windows-gnu \
    && cp target/x86_64-pc-windows-gnu/"$BUILD_PROFILE"/mogumogu.exe target/x86_64-pc-windows-gnu/"$BUILD_PROFILE"/mogumogu-cli.exe \
       LICENSE README.md THIRD_PARTY.md Cargo.lock /out/ \
    && cp assets/fonts/zen-maru-gothic/OFL.txt /out/FONT-LICENSE.txt

# Only the files for dist\mogumogu, nothing of the build environment.
FROM scratch AS windows-exe
COPY --from=windows /out/ /

# Base for check/sandbox/audit stages: installs Python, rustfmt and Clippy
# before copying source so tool layers (including osv-scanner) stay cached.
FROM toolchain AS checks-base
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt clippy --toolchain 1.98.0
ENV PYTHONDONTWRITEBYTECODE=1

# Pinned official OSV-Scanner v2.7.1 binary verified against release SHA256SUMS.
# Shared by the offline test sandbox and Cargo.lock security-audit stage; the
# default `validated` stage does not download it.
FROM checks-base AS osv-scanner-bin
ARG TARGETARCH
COPY packages/mogumogu-advisories/src /opt/advisories/src
RUN mkdir -p /opt/osv && chown 10001:10001 /opt/osv
USER 10001:10001
RUN PYTHONPATH=/opt/advisories/src python3 -m mogumogu_advisories fetch-scanner \
    --os linux --arch "${TARGETARCH:-amd64}" --output /opt/osv/osv-scanner

# Source is copied into the image; checks never need a host filesystem mount.
FROM checks-base AS checks
COPY . .
USER 10001:10001
ENTRYPOINT ["python3", "scripts/check.py"]
CMD ["--core"]

# Self-contained offline test and benchmark sandbox: compiles core tests and
# the release benchmark into /opt/mogumogu-test during build and bundles the
# verified OSV-Scanner binary so the runtime container runs with
# `network_mode: none`, read-only rootfs and zero volumes.
FROM checks AS test-env
USER root
RUN mkdir -p /opt/mogumogu-test/bin && chown -R 10001:10001 /opt/mogumogu-test
COPY --from=osv-scanner-bin --chown=10001:10001 /opt/osv/osv-scanner /opt/mogumogu-test/bin/osv-scanner
USER 10001:10001
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    python3 scripts/docker_sandbox.py --build /opt/mogumogu-test
ENTRYPOINT ["python3", "scripts/docker_sandbox.py", "--run", "/opt/mogumogu-test"]
CMD ["--all"]

# Isolated offline Cargo.lock CVE/RustSec audit: fetches the public crates.io
# OSV archive during build (bustable via OSV_FEED_DATE) and evaluates Cargo.lock
# against security/advisory-triage.toml completely offline at runtime.
FROM checks AS security-audit
ARG OSV_FEED_DATE=local
ENV PYTHONPATH=/workspace/packages/mogumogu-advisories/src \
    MOGUMOGU_OSV_CACHE=/opt/mogumogu-audit/cache \
    MOGUMOGU_OSV_SCANNER=/opt/mogumogu-audit/bin/osv-scanner
USER root
RUN mkdir -p /opt/mogumogu-audit/bin /opt/mogumogu-audit/cache \
    && chown -R 10001:10001 /opt/mogumogu-audit
COPY --from=osv-scanner-bin --chown=10001:10001 /opt/osv/osv-scanner /opt/mogumogu-audit/bin/osv-scanner
USER 10001:10001
RUN echo "OSV feed refresh epoch: ${OSV_FEED_DATE}" \
    && python3 -m mogumogu_advisories refresh --ecosystem crates.io
ENTRYPOINT ["python3", "-m", "mogumogu_advisories", "audit", "Cargo.lock"]

# CI verifies during the build. Local Compose runs use the checks target and
# named volumes, so subsequent checks retain downloads and compilation output.
# As the last stage, this stays the default target of `docker build .`.
FROM checks AS validated
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    python3 scripts/check.py --core
