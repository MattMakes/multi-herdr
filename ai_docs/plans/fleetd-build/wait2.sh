#!/bin/zsh
cd /Users/MASCOTT/projects/mh/multi-herdr
cnt() { horch ledger status --run fleetd-build 2>&1 | grep -oE '8[0-9][0-9] tasks: [0-9]+ done' | head -1 }
./ai_docs/plans/fleetd-build/notes.sh; base="$(cnt)"; echo "base=$base"
for i in {1..18}; do
  now="$(cnt)"
  [ -n "$now" ] && [ "$now" != "$base" ] && { echo "CHANGED $base -> $now"; break }
  f=$(df -g / | tail -1 | awk '{print $4}')
  [ "$f" -le 4 ] && { echo "DISK $f"; break }
  q="$(horch inbox --peek 2>&1 | grep -E '#[0-9a-f]{8}\] (QUESTION|BLOCKED)' | head -3)"
  [ -n "$q" ] && { echo "Q: $q" | cut -c1-400; break }
  ./ai_docs/plans/fleetd-build/notes.sh; sp="$(horch ledger plan-workers --run fleetd-build 2>&1 | grep -E "spawn [1-9]")"; [ -n "$sp" ] && { echo "CAN START: $sp"; break }
  sleep 30
done
horch ledger status --run fleetd-build 2>&1 | grep -o '8[0-9][0-9] tasks.*'
df -g / | tail -1 | awk '{print "free",$4}'
uptime | sed 's/.*load/load/'
horch ledger plan-workers --run fleetd-build 2>&1 | grep -E 'spawn [1-9]'
