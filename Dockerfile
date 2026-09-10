# syntax=docker/dockerfile:1
#
# Xazz official image (issue #66, E2)
#
#   docker build -t xazz .
#   docker run --rm -p 8005:8005 -v "$PWD/data:/data" xazz
#   open http://127.0.0.1:8005        # Visual IDE + REST API
#
# The image bundles: xazz (CLI), xazz-runner (isolated engine bridge),
# xazz-server (REST API + IDE), and the built Visual IDE under /app/web.
# Mount a host directory at /data to read/write datasets and artifacts.

# ── Stage 1: Rust binaries ─────────────────────────────────────────────────
FROM rust:1-bookworm AS rust-builder
WORKDIR /src

# cmake/g++ cover the bundled DuckDB C++ build in libduckdb-sys.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake g++ \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release -p xazz -p xazz-runner -p xazz-server

# ── Stage 2: Visual IDE frontend ───────────────────────────────────────────
FROM node:20-bookworm-slim AS web-builder
WORKDIR /web
COPY visual-ide/package*.json ./
RUN npm ci
COPY visual-ide/ ./
RUN npm run build

# ── Stage 3: Runtime ───────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder \
    /src/target/release/xazz \
    /src/target/release/xazz-runner \
    /src/target/release/xazz-server \
    /app/
COPY --from=web-builder /web/dist /app/web

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
