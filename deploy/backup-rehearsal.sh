#!/usr/bin/env bash
# Restore a verified snapshot into a *separate* data directory. Never the live path.
# Usage: deploy/backup-rehearsal.sh <backup-id> <scratch-dir>
# Orbit reads its configuration from the ORBIT_* environment variables of the caller.
# Set them as for the live server before you run this script (see docs/operations.md).
set -euo pipefail
backup_id="${1:?backup-id}"
scratch="${2:?scratch directory for the restored copy}"

orbit_bin="${ORBIT_BIN:-./target/release/orbit}"
unset ORBIT_BIN # Orbit rejects unknown ORBIT_ variables.
[[ -x "$orbit_bin" ]] || fail_bin() { echo "set ORBIT_BIN to the orbit binary" >&2; exit 1; }
[[ -x "$orbit_bin" ]] || fail_bin

mkdir -p "$scratch"
restore_db="${scratch}/orbit.sqlite"
restore_attachments="${scratch}/attachments"
if [[ -e "$restore_db" ]]; then
  echo "scratch already contains a database; refusing to overwrite ${restore_db}" >&2
  exit 1
fi

"$orbit_bin" backup verify "$backup_id"
echo "verified ${backup_id}"
echo "Restore into ${scratch} with the documented restore procedure while Orbit is stopped."
echo "Do not point this rehearsal at the live data or backup directories."
