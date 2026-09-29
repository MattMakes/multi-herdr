#!/usr/bin/env bash
# Every requirement ID in the telemetry design (section 3) must have at least
# one test whose name starts with the lowercase ID: TEL-05 -> tel_05_...
#
#   scripts/check-req-coverage.sh            # every ID
#   scripts/check-req-coverage.sh TEL-05 ... # only these (one milestone)
#
# Prints each ID with the tests found, and exits 1 if any ID has none.
set -euo pipefail
cd "$(dirname "$0")/.."
design=ai_docs/designs/2026-09-28-fleet-telemetry-design.md

if [ "$#" -gt 0 ]; then
  ids="$*"
else
  ids=$(grep -oE '^\| (TEL|QUO|SPC|BAL|NFR)-[0-9]{2} \|' "$design" | grep -oE '[A-Z]{3}-[0-9]{2}' | sort -u)
fi

list=$(cargo test --workspace --quiet -- --list 2>/dev/null | sed -n 's/: test$//p' | sed 's/.*:://' | sort -u)

missing=0
for id in $ids; do
  prefix=$(echo "$id" | tr 'A-Z-' 'a-z_')_
  found=$(echo "$list" | grep "^$prefix" | tr '\n' ' ' || true)
  if [ -z "$found" ]; then
    echo "MISSING $id: no test named ${prefix}*"
    missing=$((missing + 1))
  else
    echo "ok      $id: $found"
  fi
done
if [ "$missing" -gt 0 ]; then
  echo "$missing requirement(s) without a test" >&2
  exit 1
fi
