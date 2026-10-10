#!/usr/bin/env bash
# Live check: the dataset idle rule on a real Codex candidate
# (docs/specs/dataset-competition.md 4.11.5, F5). Re-runnable. Prints PASS/FAIL/SKIP
# per step and exits 1 on any FAIL. Writes only under
# .worktrees/_scratch/live-dataset/ and its sibling log files, plus the dataset
# state the coordinator keeps under ~/.local/state/horch/multi-herdr/.
# Never touches ANTHROPIC_API_KEY. See docs/live-checks/dataset.md.
#
# Paid step: 1 codex-luna candidate with a 1-line task, budget ceiling 2 USD
# (the projection is 0.01 USD). The candidate cannot finish: the task forbids
# `horch done` and commits.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY
ROOT=$PWD
R=$ROOT/.worktrees/_scratch/live-dataset
LOG=$ROOT/.worktrees/_scratch/live-dataset-run
NUDGE_S=40
END_S=40
fails=0
pass() { echo "PASS $1${2:+: $2}"; }
fail() { echo "FAIL $1: $2"; fails=$((fails + 1)); }
skip() { echo "SKIP $1: $2"; }
for t in multi-herdr-dataset herdr codex jq git; do
  command -v "$t" >/dev/null 2>&1 || { skip "D0 tools" "$t is not installed"; exit 0; }
done
mkdir -p "$R" "$LOG"

# Codex asks for trust per git root, and the scratch repo is its own root (PRE-14).
if ! grep -qF "[projects.\"$R\"]" "${CODEX_HOME:-$HOME/.codex}/config.toml" 2>/dev/null; then
  skip "D0 codex trust" "add these 2 lines to ${CODEX_HOME:-$HOME/.codex}/config.toml (keep a copy first), then re-run: [projects.\"$R\"] / trust_level = \"trusted\". To undo, delete the same 2 lines."
  exit 0
fi

# A fresh repo each run; keep only the config backup.
find "$R" -mindepth 1 -maxdepth 1 ! -name codex-config.toml.bak -exec rm -rf {} +
mkdir -p "$R/.multi-herdr"
git -C "$R" init -q -b main
git -C "$R" config user.email scratch@example.invalid
git -C "$R" config user.name scratch
printf '# live dataset scratch\n' > "$R/README.md"
printf 'codex-config.toml.bak\n' > "$R/.gitignore"
cat > "$R/.multi-herdr/dataset.yaml" <<EOF
candidates: 1
baseline: codex-luna
budget:
  hard_usd_micro: 2000000
  expected_tokens:
    all: { input: 20000, cache_read: 100000, output: 4000 }
caps:
  candidate_deadline_s: 300
  idle_nudge_after_s: $NUDGE_S
  idle_end_after_s: $END_S
EOF
git -C "$R" add -A
git -C "$R" commit -q -m "scratch repo"

STATE=$HOME/.local/state/horch/multi-herdr/$(printf '%s' "$R" | sed 's#[/._]#-#g')
task='Reply with the single word READY and then stop. Do not create files. Do not run any command. Never run horch done and never commit: a coordinator ends this session.'
start=$(date -u +%Y-%m-%dT%H:%M:%S)
rm -f "$LOG/panes.log"
echo "     (D1 starts 1 codex-luna candidate with a 1-line task: a small paid session; it ends by itself after about $((NUDGE_S + END_S + 30)) s)"
multi-herdr-dataset --project "$R" run "$task" > "$LOG/run.out" 2> "$LOG/run.err" &
runpid=$!

# Watch the candidate pane's agent_status while the coordinator runs.
exp=""; pane=""
while kill -0 "$runpid" 2>/dev/null; do
  [ -n "$exp" ] || exp=$(sed -n 's/^experiment //p' "$LOG/run.out" | head -1)
  if [ -z "$pane" ] && [ -n "$exp" ] && ls "$STATE"/events/*.jsonl >/dev/null 2>&1; then
    pane=$(cat "$STATE"/events/*.jsonl | jq -r --arg e "$exp" 'select(.experiment_id == $e and .kind == "candidate.spawned") | .payload.pane' | head -1)
  fi
  [ -z "$pane" ] || echo "$(date -u +%H:%M:%S) $(herdr pane get "$pane" 2>/dev/null | jq -r '.result.pane.agent_status // "gone"' 2>/dev/null || echo gone)" >> "$LOG/panes.log"
  sleep 3
done
set +e; wait "$runpid"; code=$?; set -e
exp=$(sed -n 's/^experiment //p' "$LOG/run.out" | head -1)
[ -n "$exp" ] || { fail "D1 run" "no experiment id; exit $code: $(tail -3 "$LOG/run.out" "$LOG/run.err" | tr '\n' ' ')"; exit 1; }
events=$(cat "$STATE"/events/*.jsonl | jq -c --arg e "$exp" 'select(.experiment_id == $e)')
printf '%s\n' "$events" | jq -r '[.occurred_at[11:19], .kind, (.payload.failure // .payload.reason // "" | tostring)] | join(" ")' > "$LOG/events.txt"
echo "     coordinator exit $code; events: $(jq -r .kind <<<"$events" | tr '\n' ' ')"

# D2: exactly 1 nudge. No event records it (spec 4.11.5): the coordinator says it on stderr,
# and the candidate's Codex rollout holds the line that arrived in the pane.
said=$(grep -c 'nudging it once' "$LOG/run.err" || true)
wt=$(printf '%s\n' "$events" | jq -r 'select(.kind == "worktree.created") | .payload.path' | head -1)
rollout=""
for f in $(find "${CODEX_HOME:-$HOME/.codex}/sessions" -name 'rollout-*.jsonl' -newermt "$start" 2>/dev/null); do
  grep -qF "$wt" "$f" && { rollout=$f; break; }
done
typed=0; first_idle=""; nudged_at=""
if [ -n "$rollout" ]; then
  typed=$(jq -r 'select(.type == "response_item" and .payload.type == "message" and .payload.role == "user") | .payload.content[0].text // empty' "$rollout" | grep -c '^If you are finished, run: horch done' || true)
  first_idle=$(jq -r 'select(.type == "event_msg" and .payload.type == "task_complete") | .timestamp[11:19]' "$rollout" | head -1)
  nudged_at=$(jq -r 'select(.type == "response_item" and .payload.type == "message" and .payload.role == "user" and ((.payload.content[0].text // "") | startswith("If you are finished"))) | .timestamp[11:19]' "$rollout" | head -1)
fi
if [ "$said" = 1 ] && [ "$typed" = 1 ]; then pass "D2 one nudge" "stderr says it once; the Codex rollout holds the nudge line once; the candidate went idle at $first_idle UTC and the nudge arrived at $nudged_at UTC (nudge after $NUDGE_S s idle)"
else fail "D2 one nudge" "stderr lines: $said; nudge lines in the rollout ${rollout:-<none found>}: $typed"; fi

# D3: the idle end. The candidate must end as Cancelled{idle_without_done}, not time out.
failure=$(printf '%s\n' "$events" | jq -c 'select(.kind == "candidate.failed") | .payload.failure' | head -1)
ended=$(grep ' candidate.failed ' "$LOG/events.txt" | cut -c1-8)
spawned=$(grep ' candidate.spawned ' "$LOG/events.txt" | cut -c1-8)
if printf '%s' "$failure" | grep -q 'idle_without_done'; then
  pass "D3 idle_without_done" "candidate.failed $failure at $ended UTC (spawned $spawned, nudge $nudged_at); round outcome: $(tail -1 "$LOG/run.out" | cut -c1-120)"
else
  fail "D3 idle_without_done" "candidate.failed was ${failure:-absent} at ${ended:-?} UTC (spawned $spawned, nudge $nudged_at, deadline 300 s); pane agent_status after the nudge: $(awk -v n="$nudged_at" '$1 >= n {print $2}' "$LOG/panes.log" | uniq -c | tr -s ' ' | tr '\n' ',')"
fi
echo "     pane status log: $LOG/panes.log; events: $LOG/events.txt"
echo "result: $fails FAIL"
[ "$fails" -eq 0 ]
