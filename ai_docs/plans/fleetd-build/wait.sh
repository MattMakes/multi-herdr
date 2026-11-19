#!/bin/zsh
# Blocks until: a worker must be spawned, a worker has a question, a task is in an odd state, or a run is settled. Max ~9 min.
cd /Users/MASCOTT/projects/mh/multi-herdr
for i in {1..16}; do
  p=""
  free=$(df -g / | tail -1 | awk '{print $4}')
  nb=$(horch ledger status --run fleetd-build 2>&1 | grep -E '^T-[0-9]+ +running ' | grep -vc 'codex-reviewer')
  gate=""; for g in T-273 T-302 T-404 T-498 T-567 T-606; do [ -f "ai_docs/plans/fleetd-build/.gate-ok-$g" ] && continue; horch ledger status --run fleetd-build 2>&1 | grep -E "^$g +queued " | grep -qv '\[' && gate="$g"; done
  horch ledger plan-workers --run fleetd-build 2>&1 | grep -E 'spawn [1-9]' | while read line; do
    [ -n "$gate" ] && [ "${line%%:*}" = "opus" ] && continue
    per="${line%%:*}"; n="$(echo "$line" | sed -E 's/.*spawn ([0-9]+).*/\1/')"
    for k in $(seq 1 $n); do
      if [ ! -f ai_docs/plans/fleetd-build/.hold ] && [ "$nb" -lt 16 ] && [ "$free" -gt 20 ]; then horch spawn "$per" --run fleetd-build >/dev/null 2>&1 && echo "$(date -u +%H:%MZ) auto-spawn $per" >> ai_docs/plans/fleetd-build/spawn.log; nb=$((nb+1)); fi
    done
  done
  horch ledger status --run fleetd-build 2>&1 | grep -E '^T-[0-9]+ +running ' | grep -v 'codex-reviewer' | awk '{print $4}' | while read rr; do
    grep -qxF "$rr" ai_docs/plans/fleetd-build/.noted 2>/dev/null && continue
    horch tell "$rr" "$(cat ai_docs/plans/fleetd-build/standing-note.txt)" >/dev/null 2>&1; echo "$rr" >> ai_docs/plans/fleetd-build/.noted
  done
  horch ledger status --run fleetd-build 2>&1 | grep -E '^T-[0-9]+ +running ' | grep -v 'codex-reviewer' | awk '{print $1" "$4}' | while read tt rr; do
    [ -f "ai_docs/plans/fleetd-build/notes/$tt.txt" ] || continue
    grep -qxF "$tt $rr" ai_docs/plans/fleetd-build/.tnoted 2>/dev/null && continue
    horch tell "$rr" "[orchestrator] Note for $tt: $(cat ai_docs/plans/fleetd-build/notes/$tt.txt)" >/dev/null 2>&1; echo "$tt $rr" >> ai_docs/plans/fleetd-build/.tnoted
  done
  [ "$free" -le 12 ] && p="LOW DISK ${free}G: spawns held"
  [ -n "$gate" ] && p="GATE $gate can start. Check that each follow-up task of its phase landed, then: touch ai_docs/plans/fleetd-build/.gate-ok-$gate ; horch spawn opus --run fleetd-build"
  [ -f ai_docs/plans/fleetd-build/.hold ] && [ "$(horch ledger status --run fleetd-build 2>&1 | grep -cE "^T-[0-9]+ +(running|landing) ")" = 0 ] && p="HOLD and no builder runs: spawn the exclusive task now"
  q="$(horch inbox --peek 2>&1 | grep -E '#[0-9a-f]{8}\] (QUESTION|BLOCKED)' | grep -vE 'code-sweep round|codex-reviewer[a-z-]*-[0-9]+ #' | head -6)"
  pk="$(horch inbox --peek 2>&1)"
  echo "$pk" | grep -E '^\[codex-reviewer-light-[0-9]+ #[0-9a-f]{8}\].*(readonly|read-only)' | sed -E 's/^\[(codex-reviewer-light-[0-9]+) .*/\1/' | sort -u | while read rr; do
    grep -qxF "$rr" ai_docs/plans/fleetd-build/.retried 2>/dev/null && continue
    horch tell "$rr" "[orchestrator] The ledger error is temporary. Run your horch ledger done command again now, with the same summary. Try up to 3 times, 20 seconds apart. Do not repeat the review." >/dev/null 2>&1; echo "$rr" >> ai_docs/plans/fleetd-build/.retried
  done
  echo "$pk" | grep -qE 'code-sweep round|codex-reviewer' && horch inbox >/dev/null 2>&1
  w=""; for a in .horch/reviews/T*/round-*/answers.md(N); do grep -qxF "$a" ai_docs/plans/fleetd-build/.handled 2>/dev/null && continue; [ -f "${a:h}/report.md" ] || continue; n="$(sed -n '/^## Critical/,/^## Minor/p' "${a:h}/report.md" | grep -E '^- F[0-9]+' | grep -E 'answered:|answer rejected' | grep -vicE 'answered: fixed in')"; [ "$n" != 0 ] && w="$w $a"; done
  st="$( (horch ledger status --run fleetd-build; horch ledger status --run fleetd-design) 2>&1 | grep -E '^T-[0-9]+ ')"
  odd="$(echo "$st" | grep -vE '^T-[0-9]+ +(done|queued|running|landing|cancelled) ' | cut -c1-140)"
  live="$(echo "$st" | grep -cE '^T-[0-9]+ +(running|landing) ')"
  if [ -n "$p$q$odd$w" ] || { [ "$live" = 0 ] && [ $i -ge 5 ]; }; then echo "SPAWN: $p"; echo "Q: $q" | cut -c1-500; echo "ODD: $odd"; echo "WAIVE?: $w"; echo "live=$live"; echo "$st" | grep -E '^T-[0-9]+ +(running|landing) ' | cut -c1-60; echo "queued=$(echo "$st" | grep -c ' queued ')"; uptime | sed 's/.*load/load/'; df -h / | tail -1 | awk '{print "free "$4}'; exit 0; fi
  sleep 30
done
echo "no change; live=$live"; uptime | sed 's/.*load/load/'
