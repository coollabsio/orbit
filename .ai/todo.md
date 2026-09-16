# Fix production WebSocket connections

- [x] Reproduce the Cloudflare WebSocket handshake failure.
- [x] Identify whether the application or proxy rejects the upgrade.
- [x] Add a regression test and implement the smallest fix.
- [ ] Verify locally and publish a new short-SHA image.

## Review

- Cloudflare reaches Orbit, but the application returns `invalid_proxy_headers` during the upgrade.
- The proxy chain repeats the same `X-Forwarded-Proto` value. Orbit now accepts identical repeated values but still rejects conflicting values.
- Pending local suite and published-image verification.
