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
    && mkdir -p /var/lib/orbit /var/backups/orbit \
    && chown -R 65532:65532 /var/lib/orbit /var/backups/orbit
COPY --from=server /orbit /orbit
# The image trusts forwarding headers from every address: it expects to run behind a proxy that is the only
# way in. Publish port 8080 directly, or share the network with untrusted containers, and clients can fake
# their IP address and https. Then set ORBIT_TRUSTED_PROXIES to the proxy network.
ENV ORBIT_ENV=production \
    ORBIT_BIND=0.0.0.0 \
    ORBIT_TRUSTED_PROXIES=0.0.0.0/0,::/0 \
    ORBIT_DATABASE_PATH=/var/lib/orbit/orbit.sqlite \
    ORBIT_ATTACHMENTS_PATH=/var/lib/orbit/attachments \
    ORBIT_BACKUPS_PATH=/var/backups/orbit
USER 65532:65532
VOLUME ["/var/lib/orbit", "/var/backups/orbit"]
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --start-period=5s --retries=10 CMD ["/orbit", "healthcheck"]
ENTRYPOINT ["/orbit"]
CMD ["serve"]
