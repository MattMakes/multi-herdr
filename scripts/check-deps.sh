#!/usr/bin/env bash
# NFR-05 and NFR-06: no runtime crate beyond the allowed lists. NFR-05 keeps
# the telemetry crates (ratatui, crossterm) in the horch crate; NFR-06 (OD6)
# allows sha2 in horch-core and horch-marketplace. Compares the direct normal
# dependencies of each checked crate with its allowed list. Keep these in sync
# with tests/nfr.rs: self-update (2026-10-20), task ledger (ADR 0001), and
# Windows process identity (#29) are already approved runtime dependencies.
set -euo pipefail
cd "$(dirname "$0")/.."
allowed_core="anyhow serde serde_json serde_yaml chrono uuid libc sha2 horch-marketplace rusqlite windows-sys"
allowed_horch="horch-core anyhow clap serde serde_json chrono tempfile libc ratatui crossterm ureq sha2 flate2 tar zip"
allowed_marketplace="anyhow serde serde_json serde_yaml sha2 thiserror"
fail=0
check() {
  local crate=$1 allowed=$2
  local deps
  deps=$(cargo tree -p "$crate" -e normal --depth 1 --prefix none 2>/dev/null | tail -n +2 | awk '{print $1}' | sort -u)
  for d in $deps; do
    case " $allowed " in *" $d "*) ;; *) echo "NFR-05/06: $crate depends on '$d', which is not allowed"; fail=1 ;; esac
  done
}
check horch-core "$allowed_core"
check horch "$allowed_horch"
# horch-marketplace arrives in phase A8; skip it until it exists.
if cargo tree -p horch-marketplace --depth 0 >/dev/null 2>&1; then
  check horch-marketplace "$allowed_marketplace"
fi
exit $fail
