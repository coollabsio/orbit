#!/usr/bin/env bash
# Verify a running Orbit instance against the team-readiness production checks.
# Does not provision hosts. Set ORBIT_VERIFY_ORIGIN to the public HTTPS origin.
set -euo pipefail
origin="${ORBIT_VERIFY_ORIGIN:?set ORBIT_VERIFY_ORIGIN to the public HTTPS origin, e.g. https://orbit.example.com}"
origin="${origin%/}"

fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok  $*"; }

curl -fsS --max-time 15 "${origin}/health/ready" >/dev/null || fail "readiness check failed"
ok "GET /health/ready"

headers="$(mktemp)"
trap 'rm -f "$headers"' EXIT
status="$(curl -sS -D "$headers" -o /dev/null -w '%{http_code}' --max-time 15 "${origin}/api/v1/auth/me" || true)"
[[ "$status" == "401" ]] || fail "unauthenticated /api/v1/auth/me expected 401, got ${status}"
ok "unauthenticated /api/v1/auth/me is 401"

if [[ "$origin" == https://* ]]; then
  grep -qi '^strict-transport-security:' "$headers" || fail "missing Strict-Transport-Security"
  ok "HSTS present"
else
  echo "skip HSTS (origin is not https)"
fi

echo "Production verification against ${origin} passed the automated subset."
echo "Still required with an operator: Secure/HttpOnly cookies after login, restart persistence, and no development seeding."
