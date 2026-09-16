#!/usr/bin/env bash
# Tests an image in isolated temporary storage. Never touches the dev database.
set -euo pipefail
image=${1:-orbit:local}
root=$(cd "$(dirname "$0")/.." && pwd)
name="orbit-deployment-smoke-$$"
tmp=$(mktemp -d)
cleanup() { docker rm -fv "$name" >/dev/null 2>&1 || true; rm -rf "$tmp"; }
trap cleanup EXIT
proxy=$(docker network inspect bridge --format '{{(index .IPAM.Config 0).Gateway}}')
docker run -d --name "$name" --read-only --cap-drop ALL --security-opt no-new-privileges:true \
  --tmpfs /tmp:size=64m,mode=1777 -p 127.0.0.1::8080 \
  -v "$root/deploy/orbit.toml:/etc/orbit/orbit.toml:ro" \
  -e ORBIT__ENVIRONMENT=production \
  -e ORBIT__HTTP__PUBLIC_ORIGIN=https://orbit.test \
  -e "ORBIT__HTTP__TRUSTED_PROXIES=$proxy/32" "$image" >/dev/null
url="http://$(docker port "$name" 8080/tcp)"
ready() {
  url="http://$(docker port "$name" 8080/tcp)"
  for _ in $(seq 1 60); do
    if curl --max-time 3 -fsS -H 'X-Forwarded-Proto: https' "$url/health/ready" >"$tmp/ready" 2>/dev/null; then return; fi
    sleep 1
  done
  echo 'Container did not become ready; inspect its logs locally.' >&2
  return 1
}
ready
[[ $(docker inspect "$name" --format '{{.Config.User}}') == 65532:65532 ]]
docker exec "$name" /bin/bash -lc 'test "$(id -u)" = 65532'
[[ $(curl -s -o /dev/null -w '%{http_code}' "$url/health/ready") == 400 ]]
curl -fsS -D "$tmp/headers" -H 'X-Forwarded-Proto: https' "$url/" >"$tmp/index"
grep -qi 'strict-transport-security:' "$tmp/headers"
grep -q '<div id="root">' "$tmp/index"
curl -fsS -H 'X-Forwarded-Proto: https' "$url/api/v1/setup/status" >"$tmp/setup"
python3 -c 'import json,sys; s=json.load(open(sys.argv[1])); assert not s["complete"],s' "$tmp/setup"
[[ $(curl -s -o /dev/null -w '%{http_code}' -H 'X-Forwarded-Proto: https' -H 'Origin: https://orbit.test' -H 'Content-Type: application/json' --data '{"email":"test@example.com","password":"password"}' "$url/api/v1/auth/login") == 401 ]]
for flag in --seed --reset; do
  if docker exec "$name" /orbit --config /etc/orbit/orbit.toml migrate run "$flag" --yes >"$tmp/guard" 2>&1; then
    echo "Production incorrectly accepted $flag" >&2; exit 1
  fi
  grep -q 'development mode' "$tmp/guard"
done
docker restart "$name" >/dev/null
ready
echo "PASS production image at $url: non-root, HTTPS boundary, embedded SPA, setup required, no seeded login, seed/reset blocked, restart readiness."
