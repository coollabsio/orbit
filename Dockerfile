FROM oven/bun:1.3.14-alpine AS frontend-builder

WORKDIR /build/site
COPY apps/site/package.json apps/site/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/site ./
RUN bun run build

WORKDIR /build/dashboard
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web ./
RUN bun run build

FROM rust:1.96-alpine AS server-builder

RUN apk add --no-cache build-base
WORKDIR /build/server
COPY apps/server/Cargo.toml apps/server/Cargo.lock ./
COPY apps/server/migrations ./migrations
COPY apps/server/assets ./assets
COPY apps/server/src ./src
RUN cargo build --release --locked

FROM alpine:3.23

RUN apk add --no-cache ca-certificates && \
    addgroup -S coolbot && \
    adduser -S -G coolbot coolbot && \
    mkdir -p /app/config /app/public/site /app/public/dashboard /data && \
    ln -s /data /app/data && \
    chown -R coolbot:coolbot /app /data

WORKDIR /app
ENV APP_ENV=production

COPY --from=server-builder /build/server/target/release/server /usr/local/bin/server
COPY --from=frontend-builder /build/site/dist ./public/site
COPY --from=frontend-builder /build/dashboard/dist ./public/dashboard
COPY --chown=coolbot:coolbot apps/server/config ./config

USER coolbot
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:3000/health || exit 1

ENTRYPOINT ["server"]
