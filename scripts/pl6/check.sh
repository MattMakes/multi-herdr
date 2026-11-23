#!/bin/sh
set -u
here=$(cd "$(dirname "$0")" && pwd)
tmpdir=${TMPDIR:-/tmp}
tmp=$(mktemp -d "${tmpdir%/}/pl6.XXXXXX") || exit 2
mkdir -p "$tmp/data" "$tmp/state"
echo "pl6: test log: $tmp/test.log"
env -u ANTHROPIC_API_KEY HORCH_DATA_DIR="$tmp/data" HORCH_STATE_DIR="$tmp/state" \
    HORCH_LEDGER_DB="$tmp/ledger.sqlite3" \
    cargo test --workspace --no-fail-fast >"$tmp/test.log" 2>&1
python3 -I "$here/pl6.py" record "$tmp/test.log" "$tmp/after.txt" || exit 2
python3 -I "$here/pl6.py" compare "$here/baseline-macos.txt" "$tmp/after.txt"
status=$?
echo "pl6: test log: $tmp/test.log"
exit $status
