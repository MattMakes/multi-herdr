#!/usr/bin/env bash
# Every requirement ID defined in docs/specs/*.md must have at least one
# (tracked files only: an operator's untracked draft must not change the result)
# test whose name starts with the lowercase ID: TEL-05 -> tel_05_...
#
# A row `| XXX-00 | ...` defines its ID only inside a table whose header's
# second cell is `Requirement` or `Check`; other tables (test mappings) only
# refer to it. An ID defined twice is an error. When the table has a `Phase`
# (or `Ph`) column, the ID's phase is that cell's first token (A0..A12,
# B1..B6); otherwise the ID is legacy and has no phase.
#
#   scripts/check-req-coverage.sh                  # every ID (all phases have landed)
#   scripts/check-req-coverage.sh --through A3     # legacy IDs + phases A0..A3
#   scripts/check-req-coverage.sh --phase A1,B2    # only IDs of these phases
#   scripts/check-req-coverage.sh TEL-05 ...       # only these IDs
#
# Prints each ID with the tests found, and exits 1 if any ID has none, or if
# nothing is left to check (a moved or deleted spec must not pass silently).
set -euo pipefail
cd "$(dirname "$0")/.."
order="A0 A1 A2 A3 A4 A5 A6 A7 A8 A9 A10 A11 A12 B1 B2 B3 B4 B5 B6"

# The position of a phase in $order (1-based), or 0 when it is not a phase.
phase_index() {
  local i=1 p
  for p in $order; do
    if [ "$p" = "$1" ]; then
      echo "$i"
      return
    fi
    i=$((i + 1))
  done
  echo 0
}

# One line per definition: `ID <tab> phase-or-"-" <tab> file:line`.
definitions() {
  awk '
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    function cell(row, n,   parts) {
      split(row, parts, "|")
      return trim(parts[n + 1])
    }
    FNR == 1 { intable = 0 }
    /^\|/ {
      if (!intable) {
        intable = 1
        defining = (cell($0, 2) == "Requirement" || cell($0, 2) == "Check")
        phasecol = 0
        n = split($0, parts, "|")
        for (i = 2; i < n; i++) {
          h = trim(parts[i])
          if (h == "Phase" || h == "Ph") phasecol = i - 1
        }
        next
      }
      if (defining && $0 ~ /^\| [A-Z][A-Z][A-Z]-[0-9][0-9] \|/) {
        id = cell($0, 1)
        phase = "-"
        if (phasecol) {
          c = cell($0, phasecol)
          if (match(c, /^(A1[0-2]|A[0-9]|B[1-6])/)) phase = substr(c, 1, RLENGTH)
        }
        printf "%s\t%s\t%s:%d\n", id, phase, FILENAME, FNR
      }
      next
    }
    { intable = 0 }
  ' "$@"
}

specs=$(git ls-files 'docs/specs/*.md')
if [ -z "$specs" ]; then
  echo "no tracked spec in docs/specs/" >&2
  exit 1
fi
# shellcheck disable=SC2086 # one word per path; spec names have no spaces
defs=$(definitions $specs)

# Reject an ID that is defined more than once.
dups=$(echo "$defs" | awk -F'\t' 'NF { n[$1]++; at[$1] = at[$1] " " $3 } END { for (id in n) if (n[id] > 1) print "DUPLICATE " id ":" at[id] }' | sort)
if [ -n "$dups" ]; then
  echo "$dups"
  exit 1
fi

# Which IDs to check: legacy ones (no phase) if $1 is 1, and those whose
# phase is in the space-separated list $2.
select_ids() {
  local want_legacy=$1 phases=$2
  echo "$defs" | while IFS="$(printf '\t')" read -r id phase _; do
    [ -n "$id" ] || continue
    if [ "$phase" = "-" ]; then
      if [ "$want_legacy" = 1 ]; then echo "$id"; fi
    else
      case " $phases " in *" $phase "*) echo "$id" ;; esac
    fi
  done | sort -u
}

case "${1:-}" in
  "")
    ids=$(select_ids 1 "$order")
    ;;
  --through)
    [ "$#" -eq 2 ] || { echo "usage: $0 --through <phase>" >&2; exit 2; }
    last=$(phase_index "$2")
    [ "$last" -gt 0 ] || { echo "unknown phase '$2'" >&2; exit 2; }
    phases=$(echo "$order" | tr ' ' '\n' | head -n "$last" | tr '\n' ' ')
    ids=$(select_ids 1 "$phases")
    ;;
  --phase)
    [ "$#" -eq 2 ] || { echo "usage: $0 --phase <phase>[,<phase>...]" >&2; exit 2; }
    phases=$(echo "$2" | tr ',' ' ')
    for p in $phases; do
      [ "$(phase_index "$p")" -gt 0 ] || { echo "unknown phase '$p'" >&2; exit 2; }
    done
    ids=$(select_ids 0 "$phases")
    ;;
  -*)
    echo "unknown option '$1'" >&2
    exit 2
    ;;
  *)
    ids="$*"
    ;;
esac

if [ -z "$ids" ]; then
  echo "no requirement IDs to check" >&2
  exit 1
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
