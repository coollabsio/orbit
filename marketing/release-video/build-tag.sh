#!/usr/bin/env bash
# Build a release tag in its own folder and start it with seed data on a free port.
# Usage: marketing/release-video/build-tag.sh v0.4.0 [port]   (work folder: /root/orbit-release-<tag>)
# Uses the main disk, not /tmp: /tmp is a 16 GB RAM disk and a cargo build fills it.
set -euo pipefail
tag=$1
port=${2:-19080}
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
work=/root/orbit-release-$tag
mkdir -p "$work/data/attachments"
if [ ! -d "$work/src" ]; then
    mkdir "$work/src"
    git -C "$repo" archive "$tag" | tar -x -C "$work/src"
fi
cd "$work/src"
bun install --frozen-lockfile
(cd apps/web && bun run build)
CARGO_TARGET_DIR="$work/target" cargo build -p orbit-server
[ -f "$work/appkey" ] || openssl rand -base64 32 > "$work/appkey"
export ORBIT_ENV=development
export ORBIT_DATABASE_PATH=$work/data/orbit.sqlite
export ORBIT_ATTACHMENTS_PATH=$work/data/attachments
export ORBIT_APP_KEY=$(cat "$work/appkey")
"$work/target/debug/orbit" migrate run --seed
nohup "$work/target/debug/orbit" serve --listen "127.0.0.1:$port" --origin "http://127.0.0.1:$port" \
    > "$work/serve.log" 2>&1 &
sleep 4
echo "Orbit $tag runs on http://127.0.0.1:$port (test@example.com / password). Stop: pkill -f '$work/target/debug/orbit serve'"
