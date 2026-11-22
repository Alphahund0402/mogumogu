# syntax=docker/dockerfile:1

# Standalone reusable container image for `mogumogu-advisories`.
# Installs the zero-dependency Python package offline and bundles the
# SHA-256-verified official OSV-Scanner v2.7.1 binary so CI/CD pipelines can
# run vulnerability audits offline (`--network none --read-only`).
FROM python:3.12-slim-bookworm AS base

ARG TARGETARCH
ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    MOGUMOGU_OSV_CACHE=/cache \
    MOGUMOGU_OSV_SCANNER=/opt/mogumogu-advisories/bin/osv-scanner

RUN useradd --uid 10001 --create-home --shell /usr/sbin/nologin auditor \
    && mkdir -p /opt/mogumogu-advisories/bin /cache /work \
    && chown -R 10001:10001 /opt/mogumogu-advisories /cache /work

WORKDIR /src
COPY pyproject.toml _build_backend.py README.md LICENSE ./
COPY src ./src

# Zero-dependency PEP 517 install: requires no network and no external build packages.
RUN pip install --no-cache-dir --no-build-isolation --no-deps . \
    && rm -rf /src

USER 10001:10001
RUN mogumogu-advisories fetch-scanner --os linux --arch "${TARGETARCH:-amd64}" --output /opt/mogumogu-advisories/bin/osv-scanner

WORKDIR /work
ENTRYPOINT ["mogumogu-advisories"]
CMD ["audit", "/work"]
