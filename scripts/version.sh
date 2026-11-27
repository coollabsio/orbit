#!/usr/bin/env bash
# Prints the Orbit version: `[workspace.package] version` in Cargo.toml.
# `--stdin` prints the version of the Cargo.toml on standard input (the release workflow reads an older commit).
# With a version (for example `0.2.0`), sets it in Cargo.toml, Cargo.lock, apps/web/package.json and bun.lock.
# `scripts/release.sh` sets the version of a release; see RELEASE.md.
set -euo pipefail
cd "$(dirname "$0")/.."

read_version() {
  awk '/^\[/{section=$0} section=="[workspace.package]" && /^version *=/{gsub(/[" ]/,"",$3); print $3; exit}'
}

current() {
  read_version < Cargo.toml
}

# Replaces the first `"version": "…"` in file $1 after the line that matches $2.
set_json_version() {
  awk -v new="$new" -v after="$2" '
    index($0, after) { found = 1 }
    found && !done && /"version": "[^"]*"/ { sub(/"version": "[^"]*"/, "\"version\": \"" new "\""); done = 1 }
    { print }
  ' "$1" > "$1.tmp"
  mv "$1.tmp" "$1"
}

if [ "${1:-}" = --stdin ]; then
  read_version
  exit 0
fi

if [ $# -eq 0 ]; then
  version=$(current)
  [ -n "$version" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
  echo "$version"
  exit 0
fi

new=$1
if ! [[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "'$new' is not a version such as 1.2.3 or 1.2.3-rc.1" >&2
  exit 1
fi
awk -v new="$new" '/^\[/{section=$0} section=="[workspace.package]" && /^version *=/{$0="version = \"" new "\""} {print}' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml
cargo update --workspace --offline --quiet
set_json_version apps/web/package.json '"name": "web"'
set_json_version bun.lock '"apps/web": {'
echo "Orbit version: $(current)"
