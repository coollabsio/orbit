# Use latest short-SHA image builds and fix container health

- [x] Reproduce the unhealthy Docker state and identify the rejected HTTP readiness request.
- [x] Send the internal health request as HTTPS through the trusted proxy policy.
- [x] Make the image smoke test require Docker health to become healthy.
- [x] Tag images with the seven-character commit SHA.
- [x] Cancel older image workflows when a newer main commit starts.
- [ ] Run tests, push, and verify the final multi-platform image.

## Review

- Root cause: the health-check client sent plain HTTP metadata, while production rejects insecure requests with HTTP 400.
- The local production image reports `healthy` with the trust-all proxy setting and passes the production smoke test.
- Pending published-image verification.
