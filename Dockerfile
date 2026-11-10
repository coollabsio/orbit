# syntax=docker/dockerfile:1.7
ARG ORBIT_BUILD_REVISION=container
FROM oven/bun:1.4.4-alpine AS web
WORKDIR /src
COPY package.json bun.lock bunfig.toml ./
COPY apps/web/package.json apps/web/
COPY apps/mobile/package.json apps/mobile/
COPY packages/api/package.json packages/api/
COPY packages/editor/package.json packages/editor/
COPY packages/database/package.json packages/database/
RUN bun install --frozen-lockfile --filter web
COPY packages/api/ packages/api/
COPY packages/editor/ packages/editor/
COPY packages/database/ packages/database/
COPY apps/web/ apps/web/
WORKDIR /src/apps/web
ARG ORBIT_BUILD_REVISION
RUN ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION} bun run build

# cargo-chef builds the Rust dependencies in their own layer. That layer changes only when
# Cargo.toml or Cargo.lock change, so the GitHub Actions layer cache keeps it between builds.
FROM lukemathwalker/cargo-chef:0.1.78-rust-1.97.1-alpine3.23 AS chef
RUN apk add --no-cache musl-dev
WORKDIR /src

FROM chef AS planner
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY apps/server/ apps/server/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --locked --release -p orbit-server --recipe-path recipe.json
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY apps/server/ apps/server/
COPY --from=web /src/apps/web/dist/ apps/web/dist/
ARG ORBIT_BUILD_REVISION
RUN ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION} \
    cargo build --locked --release -p orbit-server \
    && cp target/release/orbit /orbit

FROM alpine:3.23
RUN apk add --no-cache bash ca-certificates tzdata \
    && mkdir -p /var/lib/orbit /var/backups/orbit \
    && chown -R 65532:65532 /var/lib/orbit /var/backups/orbit
COPY --from=server /orbit /orbit
# No ORBIT_TRUSTED_PROXIES default: production refuses to start until it lists the reverse proxy addresses.
# Include 127.0.0.1/32 so the HEALTHCHECK (from inside the container) counts as HTTPS.
ENV ORBIT_ENV=production \
    ORBIT_BIND=0.0.0.0 \
    ORBIT_DATABASE_PATH=/var/lib/orbit/orbit.sqlite \
    ORBIT_ATTACHMENTS_PATH=/var/lib/orbit/attachments \
    ORBIT_BACKUPS_PATH=/var/backups/orbit
USER 65532:65532
VOLUME ["/var/lib/orbit", "/var/backups/orbit"]
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=10 CMD ["/orbit", "healthcheck"]
ENTRYPOINT ["/orbit"]
CMD ["serve"]
