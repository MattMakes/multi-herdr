#!/usr/bin/env bash
# NFR-05: the telemetry work adds no runtime crate beyond ratatui and
# crossterm, and those only in the horch crate. Compares the direct normal
# dependencies of every workspace crate with the allowed list.
set -euo pipefail
cd "$(dirname "$0")/.."
allowed_core="anyhow serde serde_json serde_yaml chrono uuid libc"
allowed_horch="horch-core anyhow clap serde serde_json tempfile libc ratatui crossterm"
fail=0
check() {
  local crate=$1 allowed=$2
  local deps
  deps=$(cargo tree -p "$crate" -e normal --depth 1 --prefix none 2>/dev/null | tail -n +2 | awk '{print $1}' | sort -u)
  for d in $deps; do
    case " $allowed " in *" $d "*) ;; *) echo "NFR-05: $crate depends on '$d', which is not allowed"; fail=1 ;; esac
  done
}
check horch-core "$allowed_core"
check horch "$allowed_horch"
exit $fail
