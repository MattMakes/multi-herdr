# Slot runner: `slot-run.sh`

Usage: `sh slot-run.sh <role> <command> [args...]`

The script runs 1 command in a free slot of a shared slot pool. Each slot is a folder that `mkdir` takes atomically, with an owner file that holds the role, the pid and the UTC time. The script removes a slot whose owner pid is gone, but leaves a slot with no owner file. It runs the command under `nice`, and it exits with the command's code, or 75 when no slot frees within `SLOT_WAIT`. Put the time limit on the command inside the slot, as the usage comment shows.

Copy the block into a file named `slot-run.sh`. `sh slot-run.sh --help` prints the usage.

```sh
#!/bin/sh
# Usage: sh slot-run.sh <role> <command> [args...]   (or: sh slot-run.sh --help)
#   POSIX shell script: run it with sh or bash, not with python3.
#   Runs 1 command in a free slot of a shared mkdir slot pool, under nice.
#   Env: SLOT_DIR (default /tmp/godot-slots), SLOT_COUNT (default 4),
#        SLOT_WAIT seconds to wait for a slot (default 600), SLOT_STEP (default 5),
#        SLOT_NICE (default 10; 0 runs without nice, for example for timing runs).
#   Put the time limit on the command itself, inside the slot, for example:
#     slot-run.sh unit-a perl -e 'alarm 540; exec @ARGV' godot --headless --path . -s tool.gd
#   Exit code: the command's code, or 75 when no slot frees within SLOT_WAIT.
set -u
case "${1:-}" in -h|--help) sed -n '2,10p' "$0"; exit 0 ;; esac
[ $# -ge 2 ] || { sed -n '2,10p' "$0" >&2; exit 64; }
role=$1; shift
dir=${SLOT_DIR:-/tmp/godot-slots}
count=${SLOT_COUNT:-4}
wait_max=${SLOT_WAIT:-600}
step=${SLOT_STEP:-5}
niceness=${SLOT_NICE:-10}
mkdir -p "$dir"
slot=""
release() {
  # Remove the slot only when the owner line names this runner.
  if [ -n "$slot" ] && [ -f "$slot/owner" ] && [ "$(cut -d' ' -f2 "$slot/owner")" = "$$" ]; then
    rm -f "$slot/owner" && rmdir "$slot"
  fi
}
trap 'release; exit 130' INT TERM
waited=0
while :; do
  i=1
  while [ "$i" -le "$count" ]; do
    s="$dir/slot$i"
    if mkdir "$s" 2>/dev/null; then
      echo "$role $$ $(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$s/owner"
      slot=$s
      break 2
    fi
    # Stale cleanup: the owner runner is gone. A slot with no owner file is left alone,
    # because its owner can be between mkdir and the owner write.
    if [ -f "$s/owner" ]; then
      pid=$(cut -d' ' -f2 "$s/owner")
      if [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; then
        echo "slot-run: removing stale $s ($(cat "$s/owner"))" >&2
        rm -f "$s/owner" && rmdir "$s" 2>/dev/null
        continue
      fi
    fi
    i=$((i + 1))
  done
  if [ "$waited" -ge "$wait_max" ]; then
    echo "slot-run: no free slot in $dir after ${waited}s" >&2
    exit 75
  fi
  sleep "$step"; waited=$((waited + step))
done
if [ "$niceness" -gt 0 ]; then
  nice -n "$niceness" "$@"
else
  "$@"
fi
rc=$?
release
exit "$rc"
```
