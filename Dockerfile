# syntax=docker/dockerfile:1
#
# Xazz official image (issue #66, E2)
#
#   docker build -t xazz .
#   docker run --rm -p 8005:8005 -v "$PWD/data:/data" xazz
#   open http://127.0.0.1:8005        # Visual IDE + REST API
#
# The image bundles: xazz (CLI), xazz-runner (isolated engine bridge),
# xazz-exec (Polars engine — required next to the runner), and xazz-server
# (REST API + IDE) with the built Visual IDE under /app/web.
# Mount a host directory at /data to read/write datasets and artifacts.
# Ports, volumes, uid 10001 permissions, GHCR tags: docs/DOCKER.md

# ── Stage 1: Rust binaries ─────────────────────────────────────────────────
FROM rust:1-bookworm AS rust-builder
WORKDIR /src

# cmake/g++ cover the bundled DuckDB C++ build in libduckdb-sys.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake g++ \
    && rm -rf /var/lib/apt/lists/*

COPY . .
# xazz-exec is required: xazz-runner resolves it next to itself (no PATH fallback).
# The cargo registry and target dir are cache mounts so rebuilds skip unchanged
# crates. A cache mount is not part of the image layer, so the finished binaries
# are copied to /out for the runtime stage to pick up.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p xazz -p xazz-runner -p xazz-exec -p xazz-server \
    && mkdir -p /out \
    && cp target/release/xazz target/release/xazz-runner \
          target/release/xazz-exec target/release/xazz-server /out/

# ── Stage 2: Visual IDE frontend ───────────────────────────────────────────
FROM node:20-bookworm-slim AS web-builder
WORKDIR /web
COPY visual-ide/package*.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY visual-ide/ ./
RUN npm run build

# ── Stage 3: Runtime ───────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder \
    /out/xazz \
    /out/xazz-runner \
    /out/xazz-exec \
    /out/xazz-server \
    /app/
COPY --from=web-builder /web/dist /app/web
COPY visual-ide/data/seoul_air_quality.csv /data/visual-ide/data/seoul_air_quality.csv

# Non-root runtime user.
RUN useradd -m -u 10001 xazz && mkdir -p /data && chown -R xazz:xazz /app /data
USER xazz

# Reachable from outside the container; server defaults to loopback otherwise.
ENV XAZZ_BIND=0.0.0.0:8005
ENV XAZZ_WEB_DIR=/app/web

EXPOSE 8005
VOLUME ["/data"]
WORKDIR /data

ENTRYPOINT ["/app/xazz-server"]
