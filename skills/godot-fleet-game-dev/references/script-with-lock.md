# Lock helper: `with-lock.sh`

Usage: `sh with-lock.sh <role> <lockdir>[,<lockdir>...] <command> [args...]`

The script takes the first free lock of a comma-separated list, runs 1 command, and releases the lock. It uses the if-mkdir shape: the release happens only inside the success branch, and only when the owner line names this role and pid. It never removes another worker's lock. It reports a lock that is older than `LOCK_STALE`, and it exits 75 when no lock frees within `LOCK_WAIT`.

Copy the block into a file named `with-lock.sh`. `sh with-lock.sh --help` prints the usage.

```sh
#!/bin/sh
# Usage: sh with-lock.sh <role> <lockdir>[,<lockdir>...] <command> [args...]
#   POSIX shell script: run it with sh or bash, not with python3. sh with-lock.sh --help prints this.
#   Takes the first free lock of the comma-separated list, runs 1 command, releases.
#   Take and release use the if-mkdir shape: release only inside the success branch,
#   and only when the owner line names this role and pid. It never removes another lock.
#   Env: LOCK_WAIT seconds (default 600), LOCK_STEP (default 15),
#        LOCK_STALE seconds after which a held lock is reported, not removed (default 1800).
#   Exit code: the command's code, or 75 when no lock frees within LOCK_WAIT.
set -u
case "${1:-}" in -h|--help) sed -n '2,9p' "$0"; exit 0 ;; esac
[ $# -ge 3 ] || { sed -n '2,9p' "$0" >&2; exit 64; }
role=$1; locks=$2; shift 2
wait_max=${LOCK_WAIT:-600}
step=${LOCK_STEP:-15}
stale=${LOCK_STALE:-1800}
me="$role $$"
waited=0
while :; do
  old_ifs=$IFS; IFS=,
  for l in $locks; do
    IFS=$old_ifs
    if mkdir "$l" 2>/dev/null; then
      echo "$me $(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$l/owner"
      trap 'if [ "$(cut -d" " -f1,2 "$l/owner" 2>/dev/null)" = "$me" ]; then rm -f "$l/owner"; rmdir "$l"; fi; exit 130' INT TERM
      "$@"
      rc=$?
      if [ "$(cut -d' ' -f1,2 "$l/owner" 2>/dev/null)" = "$me" ]; then
        rm -f "$l/owner" && rmdir "$l"
      else
        echo "with-lock: owner of $l changed; lock left in place" >&2
      fi
      exit "$rc"
    fi
    if [ -f "$l/owner" ]; then
      age=$(( $(date +%s) - $(stat -f %m "$l/owner" 2>/dev/null || stat -c %Y "$l/owner") ))
      [ "$age" -gt "$stale" ] && echo "with-lock: $l held ${age}s by: $(cat "$l/owner"); report it to its owner" >&2
    fi
  done
  IFS=$old_ifs
  if [ "$waited" -ge "$wait_max" ]; then
    echo "with-lock: no free lock in [$locks] after ${waited}s" >&2
    exit 75
  fi
  sleep "$step"; waited=$((waited + step))
done
```
