#!/usr/bin/env bash
# Restore a verified snapshot into a *separate* data directory. Never the live path.
# Usage: deploy/backup-rehearsal.sh <config.toml> <backup-id> <scratch-dir>
set -euo pipefail
config="${1:?config.toml}"
backup_id="${2:?backup-id}"
scratch="${3:?scratch directory for the restored copy}"

orbit_bin="${ORBIT_BIN:-./target/release/orbit}"
[[ -x "$orbit_bin" ]] || fail_bin() { echo "set ORBIT_BIN to the orbit binary" >&2; exit 1; }
[[ -x "$orbit_bin" ]] || fail_bin

mkdir -p "$scratch"
restore_db="${scratch}/orbit.sqlite"
restore_attachments="${scratch}/attachments"
if [[ -e "$restore_db" ]]; then
  echo "scratch already contains a database; refusing to overwrite ${restore_db}" >&2
  exit 1
fi

"$orbit_bin" --config "$config" backup verify "$backup_id"
echo "verified ${backup_id}"
echo "Restore into ${scratch} with the documented restore procedure while Orbit is stopped."
echo "Do not point this rehearsal at the live data or backup directories."
