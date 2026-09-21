# syntax=docker/dockerfile:1.7
#
# One image: the Rust backend serving the built frontend from the same origin, which is
# what keeps this app free of a CORS layer (see CLAUDE.md). The model runner is a
# separate container — see compose.yaml.

# --- frontend -----------------------------------------------------------------------
FROM node:22-slim AS web
WORKDIR /web
# Lockfile first, so a source edit does not re-download every package.
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# --- backend ------------------------------------------------------------------------
FROM rust:1-slim-bookworm AS api
# reqwest links OpenSSL. SQLite needs nothing: sqlx compiles its own copy in.
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY backend/ ./
# The migrations are embedded by `sqlx::migrate!` at compile time, so the binary carries
# its own schema and needs no DATABASE_URL here or at run time — and `build.rs` is what
# makes a newly added migration trigger that rebuild. Cache mounts keep the crate
# registry and the target dir between builds; the binary is copied out because a cache
# mount is not part of the image. The target mount has its own id so no other project on
# this builder that happens to use /src/target can share, and poison, it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=syspractice-target,target=/src/target \
    cargo build --release --locked \
 && cp target/release/syspractice-backend /usr/local/bin/syspractice

# --- runtime ------------------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --create-home app \
 && mkdir /data && chown app:app /data

COPY --from=api /usr/local/bin/syspractice /usr/local/bin/syspractice
COPY --from=web /web/dist /srv/dist

# 0.0.0.0 inside the container is not the same decision as 0.0.0.0 on a host: the port
# is only reachable from outside through what compose.yaml publishes.
ENV SYSPRACTICE_ADDR=0.0.0.0:8787 \
    SYSPRACTICE_DB=/data/syspractice.db \
    SYSPRACTICE_DIST=/srv/dist

# /data is created and owned by `app` above, so a fresh named volume inherits that
# ownership. Created by root instead, the database would not be writable and the first
# migration would fail.
VOLUME /data
USER app
EXPOSE 8787

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8787/api/scenarios >/dev/null || exit 1

CMD ["syspractice"]
