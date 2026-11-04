#!/usr/bin/env bash
# Prints the Orbit version: `[workspace.package] version` in Cargo.toml.
# With an argument (for example `0.2.0`), sets that version and updates Cargo.lock.
set -euo pipefail
cd "$(dirname "$0")/.."

current() {
  awk '/^\[/{section=$0} section=="[workspace.package]" && /^version *=/{gsub(/[" ]/,"",$3); print $3; exit}' Cargo.toml
}

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
echo "Orbit version: $(current). Commit Cargo.toml and Cargo.lock, then publish the GitHub release with tag v$new."
