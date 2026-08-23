# syntax=docker/dockerfile:1.7

FROM oven/bun:1.4.0@sha256:5ff609364c049b54eb0ff560ec96319729a972078ef2c755d758f0c6ef89c2d6 AS bun

FROM rust:1.89.0-bookworm@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends clang cmake nasm pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun

WORKDIR /src

COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY frontend ./frontend

RUN --mount=type=cache,target=/root/.bun/install/cache \
    cd frontend \
    && bun install --frozen-lockfile

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    --mount=type=cache,target=/root/.bun/install/cache \
    cargo build --locked --release \
    && cp target/release/monoize /tmp/monoize

FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

ARG VERSION=dev
ARG REVISION=unknown

LABEL org.opencontainers.image.title="Monoize" \
      org.opencontainers.image.description="Self-hosted AI API gateway with protocol conversion and multi-provider routing" \
      org.opencontainers.image.source="https://github.com/Ikaleio/monoize" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --user-group --home-dir /app --shell /usr/sbin/nologin monoize \
    && install --directory --owner monoize --group monoize /app/data

COPY --from=builder --chown=monoize:monoize /tmp/monoize /usr/local/bin/monoize

USER monoize
WORKDIR /app

VOLUME ["/app/data"]
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/ >/dev/null || exit 1

ENTRYPOINT ["monoize"]
