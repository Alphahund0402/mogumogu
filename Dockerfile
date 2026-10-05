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

# Source is copied into the image; checks never need a host filesystem mount.
FROM toolchain AS checks
COPY . .
USER 10001:10001
ENTRYPOINT ["python3", "scripts/check.py"]
CMD ["--core"]

# CI verifies during the build. Local Compose runs use the checks target and
# named volumes, so subsequent checks retain downloads and compilation output.
FROM checks AS validated
RUN --mount=type=cache,id=mogumogu-cargo,target=/cache/cargo,uid=10001,gid=10001,sharing=locked \
    --mount=type=cache,id=mogumogu-target,target=/workspace/target,uid=10001,gid=10001,sharing=locked \
    python3 scripts/check.py --core
