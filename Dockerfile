# syntax=docker/dockerfile:1.7
ARG ORBIT_BUILD_REVISION=container
FROM oven/bun:1.3.14-alpine AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web/ ./
ARG ORBIT_BUILD_REVISION
RUN ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION} bun run build

FROM rust:1.97.1-alpine AS server
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY apps/server/ apps/server/
COPY --from=web /src/apps/web/dist/ apps/web/dist/
ARG ORBIT_BUILD_REVISION
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION} \
    cargo build --locked --release -p orbit-server \
    && cp target/release/orbit /orbit

FROM alpine:3.23
RUN apk add --no-cache bash ca-certificates tzdata \
    && mkdir -p /var/lib/orbit /var/backups/orbit /etc/orbit \
    && chown -R 65532:65532 /var/lib/orbit /var/backups/orbit /etc/orbit
COPY --from=server /orbit /orbit
USER 65532:65532
VOLUME ["/var/lib/orbit", "/var/backups/orbit", "/etc/orbit"]
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=10 CMD ["/orbit", "--config", "/etc/orbit/orbit.toml", "healthcheck"]
ENTRYPOINT ["/orbit", "--config", "/etc/orbit/orbit.toml"]
CMD ["serve"]
