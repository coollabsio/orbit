# Use latest short-SHA image builds and fix container health

- [x] Reproduce the unhealthy Docker state and identify the rejected HTTP readiness request.
- [x] Send the internal health request as HTTPS through the trusted proxy policy.
- [x] Make the image smoke test require Docker health to become healthy.
- [x] Tag images with the seven-character commit SHA.
- [x] Cancel older image workflows when a newer main commit starts.
- [x] Run tests, push, and verify the final multi-platform image.

## Review

- Root cause: the health-check client sent plain HTTP metadata, while production rejects insecure requests with HTTP 400.
- The local production image reports `healthy` with the trust-all proxy setting and passes the production smoke test.
- Workflow run `35095822143` passed for both native architectures.
- Published image `ghcr.io/coollabsio/orbit:0579111` contains AMD64 and ARM64 manifests.
- The published AMD64 container reports `healthy` and runs as user and group 65532.
