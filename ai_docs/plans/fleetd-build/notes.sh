#!/bin/zsh
# Sends the standing note once to each running builder, and the task note once to each worker of a task that has one.
cd /Users/MASCOTT/projects/mh/multi-herdr
P=ai_docs/plans/fleetd-build
horch ledger status --run fleetd-build 2>&1 | grep -E '^T-[0-9]+ +running ' | grep -v 'codex-reviewer' | awk '{print $1" "$4}' | while read tt rr; do
  if ! grep -qxF "$rr" $P/.noted 2>/dev/null; then
    horch tell "$rr" "$(cat $P/standing-note.txt)" >/dev/null 2>&1; echo "$rr" >> $P/.noted; echo "noted $rr"
  fi
  if [ -f "$P/notes/$tt.txt" ] && ! grep -qxF "$tt $rr" $P/.tnoted 2>/dev/null; then
    horch tell "$rr" "[orchestrator] Note for $tt: $(cat $P/notes/$tt.txt)" >/dev/null 2>&1; echo "$tt $rr" >> $P/.tnoted; echo "tnoted $tt $rr"
  fi
done
