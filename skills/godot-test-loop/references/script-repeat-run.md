# Script: repeat_run.sh (flake counter)

Usage: `sh repeat_run.sh <project dir> <filter> [runs=20] [timeout seconds=300]`

A POSIX shell script that runs 1 headless test filter many times and counts the failed runs. Each run writes its own log into a new temp folder. The script prints the folder and the fail count, and exits 0 only when every run passed. It uses `perl` for the timeout, because macOS has no `timeout` command. It needs a `godot` binary on PATH, or the binary path in `GODOT`.

Copy the block into a file named `repeat_run.sh`.

```sh
#!/bin/sh
# Runs one headless test filter many times and counts the failed runs. Use it to measure a flake.
# Usage: sh repeat_run.sh <project dir> <filter> [runs=20] [timeout seconds=300]
# Example: sh repeat_run.sh ./game test_focus 20
# Each run writes its own log to a new temp folder. The script prints the folder and the fail count.
# The timeout uses perl, because macOS has no `timeout` command.
case "$1" in
  -h|--help) echo "usage: sh repeat_run.sh <project dir> <filter> [runs] [timeout seconds]"; exit 0 ;;
esac
project=$1
filter=$2
runs=${3:-20}
limit=${4:-300}
if [ -z "$project" ] || [ -z "$filter" ]; then
  echo "usage: sh repeat_run.sh <project dir> <filter> [runs] [timeout seconds]" >&2
  exit 2
fi
godot_bin=${GODOT:-godot}
logs=$(mktemp -d "${TMPDIR:-/tmp}/repeat-run.XXXXXX")
fails=0
i=1
while [ "$i" -le "$runs" ]; do
  log="$logs/run-$i.log"
  perl -e 'alarm shift; exec @ARGV' "$limit" \
    "$godot_bin" --headless --path "$project" -s res://tests/run_tests.gd -- "--filter=$filter" > "$log" 2>&1
  rc=$?
  if [ "$rc" -ne 0 ]; then
    fails=$((fails + 1))
    echo "run $i: exit $rc ($(grep -c '^FAIL' "$log") FAIL lines)"
  fi
  i=$((i + 1))
done
echo "$fails of $runs runs failed; logs in $logs"
[ "$fails" -eq 0 ]
```
