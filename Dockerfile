# syntax=docker/dockerfile:1.7
ARG ORBIT_BUILD_REVISION=container
FROM oven/bun:1.3.14-alpine AS web
ARG ORBIT_BUILD_REVISION
ENV ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION}
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web/ ./
RUN bun run build

FROM rust:1.97.1-alpine AS server
ARG ORBIT_BUILD_REVISION
ENV ORBIT_BUILD_REVISION=${ORBIT_BUILD_REVISION}
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY apps/server/ apps/server/
COPY --from=web /src/apps/web/dist/ apps/web/dist/
RUN cargo build --locked --release -p orbit-server

FROM alpine:3.23 AS runtime-files
RUN apk add --no-cache ca-certificates tzdata \
    && mkdir -p /var/lib/orbit /var/backups/orbit /etc/orbit \
    && chown -R 65532:65532 /var/lib/orbit /var/backups/orbit /etc/orbit

FROM scratch
COPY --from=runtime-files /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=runtime-files /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=runtime-files --chown=65532:65532 /var/lib/orbit /var/lib/orbit
COPY --from=runtime-files --chown=65532:65532 /var/backups/orbit /var/backups/orbit
COPY --from=runtime-files --chown=65532:65532 /etc/orbit /etc/orbit
COPY --from=server /src/target/release/orbit /orbit
USER 65532:65532
VOLUME ["/var/lib/orbit", "/var/backups/orbit", "/etc/orbit"]
EXPOSE 8080 2525
ENTRYPOINT ["/orbit", "--config", "/etc/orbit/orbit.toml"]
CMD ["serve"]
