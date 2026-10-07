#!/usr/bin/env bash
# Live check: telemetry on the operator's Mac (docs/specs/telemetry.md section 17,
# L1-L10). Re-runnable. Prints PASS/FAIL/SKIP per step and exits 1 on any FAIL.
# Writes only under .worktrees/_scratch/live-telemetry/. Never touches
# ANTHROPIC_API_KEY. See docs/live-checks/telemetry.md.
#
# Paid steps, each a 1-line task: L6 (1 sonnet worker), L8 (1 codex worker).
# Steps that need the operator (L3 comparison, L5) print the exact procedure.
# L7 sends kill -9 to the live collector and restarts it with
# `horch telemetry ensure`; it runs only with LIVE_TELEMETRY_KILL_COLLECTOR=1.
set -euo pipefail
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY
ROOT=$PWD
S=$ROOT/.worktrees/_scratch/live-telemetry
STATE=$S/state                      # a private XDG_STATE_HOME: no collector holds its lock
TEL=$HOME/.local/state/horch/telemetry   # the operator's live store
FX=$ROOT/crates/horch-core/tests/fixtures/telemetry
mkdir -p "$S" "$STATE/horch/telemetry" "$S/probe-cwd"
fails=0
pass() { echo "PASS $1${2:+: $2}"; }
fail() { echo "FAIL $1: $2"; fails=$((fails + 1)); }
skip() { echo "SKIP $1: $2"; }
info() { echo "     $*"; }
now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
need() { command -v "$1" >/dev/null 2>&1 || { skip "$2" "$1 is not installed"; return 1; }; }
iso_epoch() { # RFC 3339 with optional fraction -> epoch seconds (float)
  perl -MTime::Local -e '
    my $t = shift; $t =~ /^(\d+)-(\d+)-(\d+)T(\d+):(\d+):(\d+)(\.\d+)?/ or exit 1;
    printf "%.3f\n", timegm($6,$5,$4,$3,$2-1,$1) + ($7 || 0)' "$1"
}
# The fixtures are dated 2026-09-28. Shift every timestamp so "now" is in them.
shift_fixture() { # <fixture> <out>
  local base delta
  base=$(date -u -j -f %Y-%m-%dT%H:%M:%SZ "$(jq -r .written_at "$1")" +%s)
  delta=$(( $(date -u +%s) - base ))
  jq --argjson d "$delta" 'walk(if type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$")
        then (fromdateiso8601 + $d | todate) else . end)' "$1" > "$2"
}
# The record of a pane: newest ledger record that has this pane id.
record_of_pane() { horch sessions --json | jq -r --arg p "$1" '[.[] | select(.pane_id == $p)] | last | .record_id // empty'; }
wait_done() { # <record> <seconds>
  local end=$(( $(date +%s) + $2 ))
  while [ "$(date +%s)" -lt "$end" ]; do
    [ "$(horch sessions --json | jq -r --arg r "$1" '.[] | select(.record_id == $r) | .status')" = done ] && return 0
    sleep 3
  done
  return 1
}
store_tokens() { # <record> <events dir>: the 5 token classes summed from the store
  cat "$2"/events-*.jsonl 2>/dev/null | jq -s -c --arg r "$1" '[.[] | select(.record_id == $r)]
    | {n: length, input: (map(.tokens.input) | add // 0), cache_write_5m: (map(.tokens.cache_write_5m) | add // 0),
       cache_write_1h: (map(.tokens.cache_write_1h) | add // 0), cache_read: (map(.tokens.cache_read) | add // 0),
       output: (map(.tokens.output) | add // 0)}'
}
dup_keys() { cat "$TEL"/events-*.jsonl | jq -r '[.agent, .session_id, .event_id] | join("|")' | sort | uniq -d | wc -l | tr -d ' '; }

need jq L1 || exit 1
need horch L1 || exit 1
since=$(date -u -v-7d +%Y-%m-%d)

# ---- L1 fixture drift --------------------------------------------------------
# Key paths (array indexes dropped) of real lines that carry usage, against what
# each reader uses (docs/specs/telemetry.md 7.1, readers.rs) and the fixture.
# The tail keeps jq fast on 100 MB transcripts; fromjson? skips a cut or half-written line.
keypaths() { tail -n 3000 "$2" | jq -R -r "fromjson? | $1" | sort -u; }
l1_harness() { # <name> <real file> <fixture file> <jq select+paths> <required paths...>
  local name=$1 real=$2 fixture=$3 sel=$4; shift 4
  local rp fp missing=() r new
  rp=$(keypaths "$sel" "$real"); fp=$(keypaths "$sel" "$fixture")
  for r in "$@"; do printf '%s\n' "$rp" | grep -qx -- "$r" || missing+=("$r"); done
  new=$(comm -13 <(printf '%s\n' "$fp") <(printf '%s\n' "$rp") | wc -l | tr -d ' ')
  if [ ${#missing[@]} -gt 0 ]; then
    fail "L1 $name" "missing in $(basename "$real"): ${missing[*]}"
  else
    pass "L1 $name" "all $# reader key paths in $(basename "$real"); $new key paths are not in the fixture (listed, not failed)"
    comm -13 <(printf '%s\n' "$fp") <(printf '%s\n' "$rp") | head -12 | sed 's/^/       new: /'
  fi
}
USAGE_PATHS='select(.message.usage?) | [paths(scalars) | map(select(type == "string")) | join(".")] | .[]'
claude_real=""
for f in $(ls -t "$HOME"/.claude/projects/*/*.jsonl 2>/dev/null | head -40); do
  [ "$(stat -f %z "$f")" -lt 30000000 ] || continue
  if grep -q '"cache_creation"' "$f" 2>/dev/null && grep -q '"usage"' "$f"; then claude_real=$f; break; fi
done
if [ -n "$claude_real" ]; then
  l1_harness claude "$claude_real" "$FX/claude/-work-alpha/11111111-1111-4111-8111-111111111111.jsonl" "$USAGE_PATHS" \
    message.id message.model message.role message.usage.input_tokens message.usage.output_tokens \
    message.usage.cache_read_input_tokens message.usage.cache_creation_input_tokens \
    message.usage.cache_creation.ephemeral_5m_input_tokens message.usage.cache_creation.ephemeral_1h_input_tokens \
    timestamp sessionId
else skip "L1 claude" "no Claude transcript with usage lines"; fi
codex_real=$(find "${CODEX_HOME:-$HOME/.codex}/sessions" -name 'rollout-*.jsonl' -type f 2>/dev/null | xargs ls -t 2>/dev/null | head -1 || true)
CODEX_PATHS='select(.type == "token_usage_record" or .type == "session_meta" or .type == "turn_context" or (.type == "event_msg" and .payload.type == "token_count"))
  | .type as $t | [paths(scalars) | map(select(type == "string")) | join(".")] | .[] | "\($t): " + .'
if [ -n "$codex_real" ]; then
  # the newest rollout may be a 1-turn file: take the newest one that has a record
  for f in $(find "${CODEX_HOME:-$HOME/.codex}/sessions" -name 'rollout-*.jsonl' -type f | xargs ls -t 2>/dev/null | head -20); do
    grep -q '"token_usage_record"' "$f" && grep -q '"turn_context"' "$f" && { codex_real=$f; break; }
  done
  l1_harness codex "$codex_real" "$FX/codex/sessions/2026/09/27/rollout-2026-09-27T20-44-04-01a0e61c-d73e-74a3-837c-b5aade8b1c38.jsonl" "$CODEX_PATHS" \
    'token_usage_record: payload.response_id' 'token_usage_record: payload.usage.input_tokens' \
    'token_usage_record: payload.usage.cached_input_tokens' 'token_usage_record: payload.usage.cache_write_input_tokens' \
    'token_usage_record: payload.usage.output_tokens' 'token_usage_record: payload.usage.reasoning_output_tokens' \
    'session_meta: payload.cli_version' 'turn_context: payload.model' 'turn_context: payload.effort' \
    'event_msg: payload.rate_limits.primary.used_percent' 'event_msg: payload.rate_limits.primary.window_minutes' \
    'event_msg: payload.rate_limits.primary.resets_at'
else skip "L1 codex" "no Codex rollout"; fi
pi_real=$(ls -t "$HOME"/.pi/agent/sessions/*/*.jsonl 2>/dev/null | head -1 || true)
PI_PATHS='select(.type == "message" and .message.role == "assistant" and .message.usage?) | [paths(scalars) | map(select(type == "string")) | join(".")] | .[]'
if [ -n "$pi_real" ] && grep -q '"usage"' "$pi_real"; then
  l1_harness pi "$pi_real" "$FX/pi/sessions/--work-beta--/2026-09-28T12-53-00-000Z_33333333-3333-4333-8333-333333333333.jsonl" "$PI_PATHS" \
    message.usage.input message.usage.output message.usage.cacheRead message.usage.cacheWrite
else skip "L1 pi" "no pi session with usage lines"; fi
oc_db=${HORCH_OPENCODE_DB:-$HOME/.local/share/opencode/opencode.db}
if [ -f "$oc_db" ] && command -v sqlite3 >/dev/null; then
  mkdir -p "$S/oc"
  sqlite3 -readonly -json "$oc_db" "SELECT data FROM message WHERE json_extract(data,'\$.tokens') IS NOT NULL ORDER BY time_updated DESC LIMIT 20" \
    | jq -c '.[] | .data | fromjson' > "$S/oc/real.jsonl" 2>/dev/null || true
  if [ -s "$S/oc/real.jsonl" ]; then
    rm -f "$S/oc/fixture.db"; sqlite3 "$S/oc/fixture.db" < "$FX/opencode/opencode.sql"
    sqlite3 -readonly -json "$S/oc/fixture.db" "SELECT data FROM message" | jq -c '.[] | .data | fromjson' > "$S/oc/fixture.jsonl"
    l1_harness opencode "$S/oc/real.jsonl" "$S/oc/fixture.jsonl" '[paths(scalars) | map(select(type == "string")) | join(".")] | .[]' \
      tokens.input tokens.output tokens.reasoning tokens.cache.read tokens.cache.write cost
  else skip "L1 opencode" "no message with tokens in the database"; fi
else skip "L1 opencode" "no opencode database or no sqlite3"; fi
skip "L1 prime" "no stable real session location to sample (readers share the pi format)"

# ---- L2 cost parity (TEL-10) --------------------------------------------------
# A fresh private store reads every transcript with the installed binary, so it
# proves the readers agree. The live store is compared too: it can hold events
# written by an older collector binary or priced before the price table knew a model.
l2_compare() { # <usage json> <cost json> -> "<compared> <live> <bad> <first bad...>"
  local live; live=$(jq -r '.rows[].transcript' "$2" | while read -r t; do
    [ -n "$(find "$t" -mmin -5 2>/dev/null)" ] && echo "$t"; done | jq -R . | jq -s -c .)
  jq -n -r --slurpfile u "$1" --slurpfile c "$2" --argjson live "$live" '
    ($u[0].records | map({(.record_id): .}) | add) as $U
    | [$c[0].rows[] | select(.transcript as $t | $live | index($t) | not)] as $rows
    | [$rows[] | . as $c | ($U[$c.record_id]) as $u
        | {id: .record_id, t: .teammate, miss: ($u == null),
           d: (if $u == null then ["no usage record"] else
                [([ "input", "cache_write_5m", "cache_write_1h", "cache_read", "output" ][]
                  | select($u.tokens[.] != $c.tokens[.]) | "\(.) \($u.tokens[.]) vs \($c.tokens[.])"),
                 (if (($u.cost_usd - $c.cost) | fabs) > 1e-6 then "cost \($u.cost_usd) vs \($c.cost)" else empty end)]
              end)}
        | select(.d | length > 0)] as $bad
    | "\($rows | length) \(($c[0].rows | length) - ($rows | length)) \($bad | length) \($bad | map(select(.d | map(startswith("cost") | not) | any)) | length) " + ($bad[0:3] | map("\(.t): \(.d | join("; "))") | join(" | "))'
}
horch cost --json --since "$since" > "$S/cost.json"
cp "$HOME"/.local/state/horch/*.json "$STATE/horch/" 2>/dev/null || true
XDG_STATE_HOME=$STATE horch usage --json --since "$since" > "$S/usage-fresh.json"
read -r n live bad tokbad rest <<EOF
$(l2_compare "$S/usage-fresh.json" "$S/cost.json")
EOF
if [ "$bad" = 0 ]; then pass "L2 cost parity (fresh store)" "$n settled records equal on 5 token classes and cost within 1e-6 USD; $live live records skipped"
else fail "L2 cost parity (fresh store)" "$bad of $n records differ ($tokbad on tokens): $rest"; fi
horch usage --json --since "$since" > "$S/usage-live.json"
read -r n live bad tokbad rest <<EOF
$(l2_compare "$S/usage-live.json" "$S/cost.json")
EOF
nullcost=$(cat "$TEL"/events-*.jsonl | jq -c 'select(.cost_usd == null) | .model' | sort | uniq -c | tr -s ' ' | tr '\n' ',' )
if [ "$bad" = 0 ]; then pass "L2 cost parity (live store)" "$n settled records equal; $live live records skipped"
else fail "L2 cost parity (live store)" "$bad of $n settled records differ, $tokbad of them on tokens (the rest on cost only): $rest; events with cost_usd null by model: ${nullcost:-none}"; fi

# ---- L3 probe truth -----------------------------------------------------------
# No collector holds the private state dir, so --refresh really probes.
rm -f "$STATE/horch/telemetry/quota.json"
( cd "$S/probe-cwd" && XDG_STATE_HOME=$STATE horch quota --refresh ) > "$S/quota-refresh.txt" 2>&1 || true
sed 's/^/     /' "$S/quota-refresh.txt"
XDG_STATE_HOME=$STATE horch quota --json | jq -r '.pools | to_entries[] | select(.value.windows | length > 0)
  | .key as $p | .value.windows[] | "     \($p) \(.name)\(if .scope_model then " (" + .scope_model + ")" else "" end): used \(.used * 100 | round)%, resets \(.resets_at)"'
skip "L3 probe truth" "needs the operator. In an interactive Claude session run /usage; in Codex run /status. Compare with the table above: every window within 1 percentage point and the same reset time (the table rounds the reset to the minute). Record the result in docs/live-checks/telemetry.md."

# ---- L4 probe hygiene ---------------------------------------------------------
# Other panes write transcripts all the time, so isolate the probes: the Claude probe
# runs in <tmp>/horch-quota-<pid>, the Codex probe in the caller's cwd ($S/probe-cwd).
marker=$S/l4-marker; rm -f "$marker"; touch "$marker"; sleep 1
for _ in 1 2 3; do
  rm -f "$STATE/horch/telemetry/quota.json"
  ( cd "$S/probe-cwd" && XDG_STATE_HOME=$STATE horch quota --refresh ) >/dev/null 2>&1 || true
done
claude_new=$(find "$HOME/.claude/projects" -maxdepth 1 -name '*horch-quota-*' -newer "$marker" 2>/dev/null | wc -l | tr -d ' ')
claude_all=$(find "$HOME/.claude/projects" -maxdepth 1 -name '*horch-quota-*' 2>/dev/null | wc -l | tr -d ' ')
codex_hit=0
for f in $(find "${CODEX_HOME:-$HOME/.codex}/sessions" -name 'rollout-*.jsonl' -newer "$marker" 2>/dev/null); do
  grep -q "\"cwd\":\"$S/probe-cwd\"" "$f" && codex_hit=$((codex_hit + 1))
done
quota_n=$(XDG_STATE_HOME=$STATE horch quota --json | jq '[.pools[] | select(.source == "get_usage" or .source == "app-server")] | length')
if [ "$quota_n" -lt 2 ]; then fail "L4 probe hygiene" "the probes did not run (only $quota_n real readings)"
elif [ "$claude_new" -eq 0 ] && [ "$codex_hit" -eq 0 ]; then
  pass "L4 probe hygiene" "3 refreshes, both probes read; new Claude probe transcript dirs: 0 (of $claude_all ever); new Codex rollouts in the probe cwd: 0"
else fail "L4 probe hygiene" "$claude_new new Claude probe dirs, $codex_hit new Codex rollouts"; fi

# ---- L5 singleton in herdr ----------------------------------------------------
skip "L5 singleton in herdr" "needs 2 new fleets, which start 2 orchestrators. Operator: (1) herdr workspace list, count workspaces labelled 'horch telemetry' (expect 1) and note the focused pane and the pid in $TEL/collector.json. (2) mkdir -p ~/scratch-a ~/scratch-b; in each run: git init; horch fleet. (3) Re-run: herdr workspace list | grep -c 'horch telemetry' (expect 1); the focused pane is unchanged; the collector.json pid is unchanged. (4) Close both scratch fleets."

# ---- L6 live latency ----------------------------------------------------------
l6() {
  echo "     (L6 starts 1 sonnet worker with a 1-line task: a small paid session)"
  pane=$(horch spawn sonnet 'Reply with the single word PONG. Then run: horch done "pong"' --no-tile --direction down | tail -1)
  rec=""; for _ in 1 2 3 4 5 6 7 8 9 10; do rec=$(record_of_pane "$pane"); [ -n "$rec" ] && break; sleep 1; done
  sid=$(horch sessions --json | jq -r --arg r "$rec" '.[] | select(.record_id == $r) | .session_id // empty')
  echo "$rec" > "$S/l6-record"
  # The origin is the time the first usage line is on disk (polled every
  # 0.2 s). Claude Code writes a line only after its message completes, so
  # the line's own timestamp is seconds older; that lag is outside horch and
  # is printed as INFO only.
  transcript=""; on_disk=""
  for _ in $(seq 1 450); do
    [ -n "$transcript" ] || transcript=$(ls "$HOME"/.claude/projects/*/"$sid".jsonl 2>/dev/null | head -1 || true)
    if [ -n "$transcript" ] && grep -q '"usage"' "$transcript" 2>/dev/null; then on_disk=$(now); break; fi
    sleep 0.2
  done
  first_ts=$(jq -r 'select(.message.usage? and .type == "assistant") | .timestamp' "$transcript" 2>/dev/null | head -1)
  seen=""
  if [ -n "$on_disk" ]; then
    # Only the 2 newest event files: the event is dated today (UTC).
    for _ in $(seq 1 150); do
      if ls "$TEL"/events-*.jsonl | tail -2 | xargs grep -q "\"record_id\":\"$rec\""; then seen=$(now); break; fi
      sleep 0.2
    done
  fi
  if [ -z "$on_disk" ]; then fail "L6 live latency" "no usage line in the transcript of $rec within 90 s"
  elif [ -z "$seen" ]; then fail "L6 live latency" "no event for $rec in the store 30 s after its first usage line was on disk"
  else
    lat=$(perl -e "printf '%.1f', $seen - $on_disk")
    # Limit 5 s: 1 tick (2 s) + the tick (under 0.2 s) + 2 polls of 0.2 s = 2.6 s at worst, with margin.
    if perl -e "exit(($seen - $on_disk) <= 5.0 ? 0 : 1)"; then pass "L6 live latency" "first event in the store ${lat} s after the first usage line was on disk (limit 5 s)"
    else fail "L6 live latency" "first event in the store ${lat} s after the first usage line was on disk; limit is 5 s"; fi
    t0=$(iso_epoch "$first_ts" 2>/dev/null || true)
    if [ -n "$t0" ]; then
      info "INFO L6: the store event came $(perl -e "printf '%.1f', $seen - $t0") s after the line's timestamp $first_ts; the line reached the disk $(perl -e "printf '%.1f', $on_disk - $t0") s after it (Claude Code write lag)"
    fi
  fi
  if wait_done "$rec" 240; then
    sleep 12
    horch cost --json --record "$rec" | jq -c '.rows[0].tokens' > "$S/l6-cost.json"
    store_tokens "$rec" "$TEL" | jq -c 'del(.n)' > "$S/l6-store.json"
    if [ "$(jq -S . "$S/l6-cost.json")" = "$(jq -S . "$S/l6-store.json")" ]; then pass "L6 totals" "store totals equal the transcript totals: $(cat "$S/l6-store.json")"
    else fail "L6 totals" "store $(cat "$S/l6-store.json") vs transcript $(cat "$S/l6-cost.json")"; fi
  else fail "L6 totals" "the worker $rec did not finish within 240 s"; fi
}
if need herdr L6; then l6 || true; fi

# ---- L7 crash recovery --------------------------------------------------------
l7() {
  rec=$(cat "$S/l6-record")
  cpid=$(jq -r .pid "$TEL/collector.json")
  if ! ps -o command= -p "$cpid" | grep -q 'horch telemetry'; then skip "L7 crash recovery" "no live collector in $TEL/collector.json (pid $cpid)"
  else
    before_tokens=$(store_tokens "$rec" "$TEL"); before_dups=$(dup_keys); before_events=$(cat "$TEL"/events-*.jsonl | wc -l | tr -d ' ')
    echo "     (L7 sends kill -9 to the live collector pid $cpid, then runs horch telemetry ensure)"
    kill -9 "$cpid"; sleep 2
    horch telemetry ensure > "$S/l7-ensure.txt" 2>&1 || true
    npid=""; for _ in $(seq 1 15); do npid=$(jq -r .pid "$TEL/collector.json"); [ "$npid" != "$cpid" ] && ps -p "$npid" >/dev/null 2>&1 && break; sleep 1; done
    sleep 6
    after_tokens=$(store_tokens "$rec" "$TEL"); after_dups=$(dup_keys); after_events=$(cat "$TEL"/events-*.jsonl | wc -l | tr -d ' ')
    wsn=$(herdr workspace list | jq '[.result.workspaces[] | select(.label == "horch telemetry")] | length')
    if [ "$npid" = "$cpid" ] || ! ps -p "$npid" >/dev/null 2>&1; then fail "L7 crash recovery" "no new live collector after ensure (pid $npid)"
    elif [ "$before_tokens" != "$after_tokens" ]; then fail "L7 crash recovery" "totals of $rec changed: $before_tokens -> $after_tokens"
    elif [ "$after_dups" != "$before_dups" ]; then fail "L7 crash recovery" "duplicate event keys $before_dups -> $after_dups"
    elif [ "$after_events" -lt "$before_events" ]; then fail "L7 crash recovery" "events fell from $before_events to $after_events"
    elif [ "$wsn" != 1 ]; then fail "L7 crash recovery" "herdr holds $wsn workspaces labelled 'horch telemetry' after the restart; expected 1 (W17)"
    else pass "L7 crash recovery" "new collector pid $npid; totals of $rec unchanged ($after_tokens); duplicate keys $before_dups -> $after_dups; events $before_events -> $after_events; horch telemetry workspaces: $wsn"; fi
  fi
}
# L7 kills the collector that every pane on this Mac shares, so it runs only on request.
if [ "${LIVE_TELEMETRY_KILL_COLLECTOR:-0}" != 1 ]; then
  skip "L7 crash recovery" "opt-in: it sends kill -9 to the shared live collector. Operator: LIVE_TELEMETRY_KILL_COLLECTOR=1 scripts/live/telemetry.sh"
elif need herdr L7; then
  if [ -s "$S/l6-record" ]; then l7 || true; else skip "L7 crash recovery" "L6 recorded no worker"; fi
fi

# ---- L8 gate drill ------------------------------------------------------------
shift_fixture "$FX/quota/claude-exhausted-codex-ok.json" "$S/claude-exhausted-codex-ok.json"
shift_fixture "$FX/quota/all-exhausted.json" "$S/all-exhausted.json"
route=$(HORCH_QUOTA_FILE=$S/claude-exhausted-codex-ok.json horch route researcher | head -1)
case $route in
  SUBSTITUTED:*codex-sol*) pass "L8 route" "$route" ;;
  *) fail "L8 route" "expected SUBSTITUTED to codex-sol, got: $route" ;;
esac
l8() {
  echo "     (L8 starts 1 codex worker with a 1-line task: a small paid session)"
  out=$(HORCH_QUOTA_FILE=$S/claude-exhausted-codex-ok.json horch spawn researcher 'Reply with the single word DONE. Then run: horch done "done"' --no-tile --direction down 2>&1)
  pane=$(printf '%s\n' "$out" | tail -1)
  rec=""; for _ in 1 2 3 4 5 6 7 8 9 10; do rec=$(record_of_pane "$pane"); [ -n "$rec" ] && break; sleep 1; done
  line=$(printf '%s\n' "$out" | grep -m1 '^SUBSTITUTED' || true)
  recjson=$(horch sessions --json | jq -c --arg r "$rec" '.[] | select(.record_id == $r) | {agent, tier, model, role, routing}')
  resolved=$(printf '%s' "$recjson" | jq -r '.routing.resolved // empty')
  paneagent=""; for _ in 1 2 3 4 5 6; do
    paneagent=$(herdr pane get "$pane" 2>/dev/null | jq -r '.result.pane.agent // empty' 2>/dev/null || true)
    [ -n "$paneagent" ] && break; sleep 2; done
  # `via` lives on the telemetry events (spec 7.1); the ledger record keeps routing.resolved.
  via=""; for _ in $(seq 1 30); do
    via=$(grep -h "\"record_id\":\"$rec\"" "$TEL"/events-*.jsonl 2>/dev/null | jq -r '.via // empty' | sort -u | head -1)
    [ -n "$via" ] && break; sleep 1; done
  if [ -z "$line" ]; then fail "L8 spawn" "no SUBSTITUTED line in the spawn output: $(printf '%s' "$out" | head -3 | tr '\n' ' ')"
  elif [ "$(printf '%s' "$recjson" | jq -r .agent)" != codex ] || [ "$resolved" != codex-sol ]; then fail "L8 spawn" "record is not a codex-sol record: $recjson"
  elif [ "$via" != codex-sol ]; then fail "L8 spawn" "no store event with via codex-sol for $rec (via: '${via}')"
  else pass "L8 spawn" "SUBSTITUTED line printed; record $rec has agent codex, teammate researcher, routing.resolved codex-sol; store events carry via $via; herdr reports pane $pane as agent ${paneagent:-<none seen>}"; fi
  wait_done "$rec" 300 || info "L8 worker $rec did not finish within 300 s; close its pane $pane by hand"
}
if need herdr L8; then l8 || true; fi
refused=$(HORCH_QUOTA_FILE=$S/all-exhausted.json horch route opus || true)
resets=$(printf '%s\n' "$refused" | grep -c 'resets')
if printf '%s' "$refused" | head -1 | grep -q '^REFUSED' && [ "$resets" -ge 2 ]; then pass "L8 refusal drill" "REFUSED with 2 reset times on the all-exhausted fixture"
else fail "L8 refusal drill" "$(printf '%s' "$refused" | head -2 | tr '\n' ' ')"; fi

# ---- L9 real gate -------------------------------------------------------------
# The plan text says both pools were exhausted until 2026-10-02. Derive the expectation
# from the real pools instead, so the step stays true after the reset.
states=$(horch quota --json | jq -r '.pools | [.claude.state, .codex.state] | join(",")')
real=$(horch route opus || true); first=$(printf '%s\n' "$real" | head -1)
if [ "$states" = exhausted,exhausted ]; then
  if printf '%s' "$first" | grep -q '^REFUSED' && [ "$(printf '%s\n' "$real" | grep -c resets)" -ge 2 ]; then pass "L9 real gate" "REFUSED with both reset times"
  else fail "L9 real gate" "both pools exhausted but route said: $first"; fi
elif printf '%s' "$first" | grep -q '^\(SPAWN\|SUBSTITUTED\)'; then pass "L9 real gate" "pools $states: $first"
else fail "L9 real gate" "pools $states but route said: $first"; fi

# ---- L10 perf -----------------------------------------------------------------
if need just L10; then
  # Workers share this checkout: a half-edited tree fails to compile. Retry, then SKIP.
  perf_ok=1
  for try in 1 2 3; do
    just verify-perf > "$S/perf.txt" 2>&1 && { perf_ok=0; break; }
    grep -q 'could not compile' "$S/perf.txt" || break
    sleep 60
  done
  if [ "$perf_ok" = 1 ] && grep -q 'could not compile' "$S/perf.txt"; then
    skip "L10 perf (synthetic 1 GB corpus)" "the shared tree did not compile in 3 tries (another worker is mid-edit): $(grep -m1 '^error' "$S/perf.txt")"
  elif [ "$perf_ok" = 0 ]; then pass "L10 perf (synthetic 1 GB corpus)" "$(grep -o 'cold [^,]*, steady [^ ]*' "$S/perf.txt" | tail -1)"
  else fail "L10 perf (synthetic 1 GB corpus)" "just verify-perf failed; see $S/perf.txt"; fi
  cpid=$(jq -r .pid "$TEL/collector.json"); kb=$(ps -o rss= -p "$cpid" 2>/dev/null | tr -d ' ' || true)
  if [ -n "$kb" ]; then
    mib=$(perl -e "printf '%.1f', $kb / 1024"); mb=$(perl -e "printf '%.1f', $kb * 1024 / 1e6")
    if [ "$kb" -lt 146484 ]; then pass "L10 collector RSS (real store, $(du -sh "$TEL" | cut -f1))" "$mib MiB ($mb MB) after $(ps -o etime= -p "$cpid" | tr -d ' '); budget 150 MB"
    else fail "L10 collector RSS (real store)" "$mib MiB ($mb MB) over the 150 MB budget"; fi
  else skip "L10 collector RSS" "no live collector"; fi
fi

rm -rf "$S/oc" "$S/perf-corpus"
echo "result: $fails FAIL"
[ "$fails" -eq 0 ]
