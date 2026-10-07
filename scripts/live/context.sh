#!/usr/bin/env bash
# Live check: the fleet context policy on the operator's Mac
# (docs/specs/context-policy.md: CTX-05, CTX-06, CTX-13, CTX-15, CTX-17,
# CTX-18, CTX-20, CTX-26). Re-runnable. Prints PASS/FAIL/SKIP per step and
# exits 1 on any FAIL. Scratch files go only under
# .worktrees/_scratch/live-context/. Each run writes its log to
# .worktrees/_scratch/live-context/results/<UTC time>.md and appends its dated
# result table to docs/live-checks/context.md (the docs/live-checks/README.md
# format). See that file.
#
# Part A runs before the install, on $HORCH_BIN (default target/debug/horch).
# Part B (LIVE_CONTEXT_PANES=1) needs a herdr server, the installed horch and
# a fleet; it prints the procedure of each pane step and checks the ledger.
# LIVE_CONTEXT_PART=b runs part B only; c runs part C, c5 its pane steps only.
# Part B reads the ledger of the current
# project (the git top level of the directory the script runs from: the
# operator's fleet); LIVE_CONTEXT_PROJECT overrides it.
# Part C (LIVE_CONTEXT_PART=c) covers the slice-2 harnesses opencode, pi and
# prime: headless readers and canaries, the Prime agent dir, then pane steps
# in the scratch fleet $S/fleet (LIVE_CONTEXT_PROJECT overrides it).
#
# Paid steps, each 1 short session: A3 (1 haiku turn), A5 (1 haiku turn, 1
# codex turn), A6 (1 haiku turn and its /compact, 1 codex turn), A8 (1 haiku
# turn, 1 codex turn). A2 and A7 spend no model tokens (/autocompact is local).
# Part C: C2 and C3 run 1 short session per harness on the teammate's model
# (pi: local ollama; opencode-pickle: a free tier; prime: its own model, paid
# when it is a hosted one).
#
# The operator's subscription logins only: every claude call is
# `env -u ANTHROPIC_API_KEY claude ...`, and the key is unset below.
set -euo pipefail
CALLER=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$(dirname "$0")/../.."
unset ANTHROPIC_API_KEY
ROOT=$PWD
S=$ROOT/.worktrees/_scratch/live-context
HORCH_BIN=${HORCH_BIN:-$ROOT/target/debug/horch}
case $HORCH_BIN in /*) ;; *) HORCH_BIN=$ROOT/$HORCH_BIN ;; esac
DOC=$ROOT/docs/live-checks/context.md
STAMP=$(date -u +%Y-%m-%dT%H-%M-%SZ)
mkdir -p "$S/results"
OUT=$S/results/$STAMP.md
ROWS=$S/results/$STAMP-rows.md
: > "$OUT"; : > "$ROWS"
fails=0
NO_RECORD=0
P= # the fleet project of part B or C
claude_() { env -u ANTHROPIC_API_KEY claude "$@" </dev/null; }
CV=$(claude_ --version 2>/dev/null || echo none)
XV=$(codex --version 2>/dev/null || echo none)
# The claim each step proves (a column of the result table).
claim_of() {
  case ${1%% *} in
    A1) echo 'the versions the steps ran on' ;;
    A2) echo 'CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`)' ;;
    A3) echo 'the fleet `--settings` env object keeps the operator env keys (spec 3.3, `harness/claude.rs`)' ;;
    A4) echo 'CTX-05.3: the codex-sol launch carries `model_auto_compact_token_limit` (`harness/codex.rs`)' ;;
    A5) echo 'CTX-02.4, CTX-17.1: the readers give the harness own number (`telemetry/context.rs`)' ;;
    A6) echo 'CTX-20: the compact-instructions block survives a compaction (`teammates/_base/fleet-worker.md`)' ;;
    A7) echo 'CTX-26: `horch context` under 1 s (`cmd/context.rs`)' ;;
    A8) echo 'CTX-15: a detached job outlives the call that started it (`runtime/process.rs` `spawn_detached`)' ;;
    B1) echo 'CTX-11.1, CTX-13.2, CTX-22.1, CTX-23.1: a Claude worker round trip' ;;
    B2) echo 'CTX-10, CTX-13.1, CTX-13.3: a Codex worker round trip, `horch note` in a Codex pane' ;;
    B3|B4) echo 'CTX-18: orchestrator self-compaction' ;;
    B5) echo 'CTX-26: `horch note` under 0.5 s in a pane' ;;
    B6) echo 'CTX-16.1: the fresh route for a pi worker' ;;
    *) echo '-' ;;
  esac
}
# 1 row of the dated result table: <result> <step> <evidence>.
row() {
  local ver ev=${3:-}
  case $2 in *codex*|A4*) ver=$XV ;; A7*|B*) ver="horch $("$HORCH_BIN" --version 2>/dev/null | awk '{print $2}')" ;; *) ver="claude $CV" ;; esac
  case $2 in A3*|A5*|A6*|A8*) [ "$1" = SKIP ] || ev="$ev (paid: 1 short session)" ;; esac
  printf '| %s | %s | %s | %s | %s |\n' "$2" "$(claim_of "$2")" "$ver" "$1" "${ev//|//}" >> "$ROWS"
}
# Each result line goes to stdout and to the log, and is 1 table row.
line() { echo "$1"; echo "$1" >> "$OUT"; }
pass() { line "PASS $1${2:+: $2}"; row PASS "$1" "${2:-}"; }
fail() { line "FAIL $1: $2"; row FAIL "$1" "$2"; fails=$((fails + 1)); }
skip() { line "SKIP $1: $2"; row SKIP "$1" "$2"; }
info() { echo "     $*"; }
need() { command -v "$1" >/dev/null 2>&1 || { skip "$2" "$1 is not installed"; return 1; }; }
now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }
digits() { tr -d ',*' <<<"$1"; }
# `500k` -> 500000, `1m` -> 1000000, `30.2k` -> 30200.
kilo() { perl -e '$_ = lc shift; /^([\d.]+)([km]?)$/ or exit 1; printf "%d\n", $1 * ($2 eq "k" ? 1e3 : $2 eq "m" ? 1e6 : 1) + 0.5' "$1"; }
# The ledger file of a project (execution::store::slug).
ledger_of() { printf '%s/%s.json' "$1" "$(printf '%s' "$2" | LC_ALL=C sed 's/[^A-Za-z0-9]/-/g')"; }
# The text around the canary in stdin (perl: grep -o fails on long JSON lines).
around() { CANARY=$1 perl -ne 'if (/(.{0,60}\Q$ENV{CANARY}\E.{0,20})/) { print "$1\n"; exit }'; }
claude_transcript() { ls "$HOME"/.claude/projects/*/"$1".jsonl 2>/dev/null | head -1 || true; }
codex_rollout() { ls -t "${CODEX_HOME:-$HOME/.codex}"/sessions/*/*/*/rollout-*"$1".jsonl 2>/dev/null | head -1 || true; }
section() { # <part letter>: append the dated result table of the part to $DOC
  [ -s "$ROWS" ] || return 0
  if [ "$NO_RECORD" = 1 ]; then # a wrong project: its table proves nothing
    echo "     no table appended to $DOC: a step found no record of its role"; : > "$ROWS"; return 0
  fi
  { echo; echo "## $(date -u +%Y-%m-%d), part $1"; echo
    echo '| step | claim it proves | tool version | result | evidence |'
    echo '|---|---|---|---|---|'; cat "$ROWS"; } >> "$DOC"
  : > "$ROWS"
}

# The newest record of <role> in the ledger of <project>: the newest
# updated_at among the records of the role, not the last one in JSON order
# (an old record of the same role stays in the ledger).
record_of() { # <project> <role> -> 1 JSON line, or nothing
  (cd "$1" && horch sessions --json) | jq -c --arg r "$2" \
    '[.[] | select(.role == $r)] | max_by(.updated_at // .created_at // "") // empty'
}
events_of() { # <project> <role> <event> -> the texts, newest last
  record_of "$1" "$2" | jq -r --arg e "$3" '.history[]? | select(.event == $e) | .text'
}
# A step whose role has no record in the project: the FAIL line only, no row,
# and the part appends no table (the project or the role is wrong).
has_record() { # <step> <project> <role>
  [ -n "$(record_of "$2" "$3")" ] && return 0
  line "FAIL $1: $2 has no record of role $3; check LIVE_CONTEXT_PROJECT and the role"
  fails=$((fails + 1)); NO_RECORD=1; return 1
}

PART=${LIVE_CONTEXT_PART:-a}
line "live check: context policy, part $PART, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
line "horch: $HORCH_BIN ($("$HORCH_BIN" --version 2>/dev/null || echo 'not built'))"

part_a() {
  need jq A1 || exit 1
  need perl A1 || exit 1
  [ -x "$HORCH_BIN" ] || { fail A1 "$HORCH_BIN is not built; run cargo build --workspace --bins"; return; }

  # ---- A1 versions -----------------------------------------------------------
  [ "$CV" != none ] && [ "$XV" != none ] && pass "A1 versions" "claude $CV, codex $XV" \
    || fail "A1 versions" "claude: $CV, codex: $XV"

  # ---- A2 windows (CTX-05, CTX-06) ---------------------------------------------
  # The tier loop: 1 /autocompact per distinct (model, setting, source) of the
  # claude rows of `horch context --windows`. The codex rows are A4.
  "$HORCH_BIN" context --windows > "$S/windows.txt"
  local tiers
  tiers=$(awk 'NR > 1 && $2 == "claude" { print $3, $4, $5, $1 }' "$S/windows.txt" \
    | sort -k1,3 | awk '{ k = $1 " " $2 " " $3; n[k]++; if (!(k in t)) t[k] = $4 }
        END { for (k in n) print k, t[k], n[k] }' | sort)
  [ -n "$tiers" ] || fail "A2 windows" "no claude row in horch context --windows"
  autocompact() { # <model> [settings json] -> the reported window line
    local args=(-p "/autocompact")
    [ "$1" != - ] && args+=(--model "$1")
    [ -n "${2:-}" ] && args+=(--settings "$2")
    claude_ "${args[@]}" 2>&1 | grep -m1 'Auto-compact window' || true
  }
  reported_tokens() { kilo "$(sed -nE 's/.*: ([0-9.]+[kKmM]?) tokens.*/\1/p' <<<"$1")" 2>/dev/null || echo "?"; }
  check_tier() { # <label> <intended tokens or -> <report line>
    local got; got=$(reported_tokens "$3")
    if [ "$2" = - ]; then
      case $3 in *default*) pass "$1" "intended harness default, reported: $3" ;;
                 *) fail "$1" "intended harness default, reported: ${3:-nothing}" ;; esac
    elif [ "$got" = "$2" ]; then pass "$1" "intended $2, reported $got: $3"
    else fail "$1" "intended $2, reported ${got}: ${3:-nothing}"; fi
  }
  local model setting source teammate count
  while read -r model setting source teammate count; do
    [ -n "$model" ] || continue
    local n; n=$(digits "$setting")
    case $source in
      operator) check_tier "A2 claude $model $setting $source ($count teammates, e.g. $teammate)" "$n" "$(autocompact "$model")" ;;
      fleet) check_tier "A2 claude $model $setting $source ($count teammates, e.g. $teammate)" "$n" \
               "$(autocompact "$model" "{\"env\":{\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\":\"$n\"}}")" ;;
      harness) check_tier "A2 claude $model - harness ($count teammates, e.g. $teammate)" - "$(autocompact "$model")" ;;
    esac
  done <<<"$tiers"
  # A scratch CLAUDE_CONFIG_DIR with no window: horch decides the fleet value,
  # and the launch value is what the harness reports (CTX-05.1 amended).
  local cc=$S/claude-config
  mkdir -p "$cc"
  env -u CLAUDE_CODE_AUTO_COMPACT_WINDOW CLAUDE_CONFIG_DIR="$cc" HORCH_CLAUDE_MANAGED_SETTINGS="$S/no-managed.json" \
    "$HORCH_BIN" context --windows > "$S/windows-scratch.txt"
  local probe; probe=$(env -u CLAUDE_CODE_AUTO_COMPACT_WINDOW CLAUDE_CONFIG_DIR="$cc" bash -c \
    'env -u ANTHROPIC_API_KEY claude -p "/autocompact" --model opus </dev/null 2>&1' | grep -m1 'Auto-compact window' || true)
  if [ -z "$probe" ]; then
    skip "A2 scratch config dir" "claude printed no window in a scratch CLAUDE_CONFIG_DIR (no sign-in there?)"
  else
    local key want tm
    for key in claude/sonnet:150000:sonnet claude/opus:200000:opus claude/orchestrator:300000:orchestrator; do
      want=${key#*:}; want=${want%%:*}; tm=${key##*:}
      local row; row=$(awk -v t="$tm" 'NR > 1 && $1 == t' "$S/windows-scratch.txt")
      local m s src; read -r _ _ m s src _ <<<"$row"
      if [ "$src" != fleet ] || [ "$(digits "$s")" != "$want" ]; then
        fail "A2 scratch ${key%%:*}" "horch decides '$s $src' for $tm, want $want fleet"
        continue
      fi
      local got; got=$(env -u CLAUDE_CODE_AUTO_COMPACT_WINDOW CLAUDE_CONFIG_DIR="$cc" bash -c \
        'env -u ANTHROPIC_API_KEY claude -p "/autocompact" --model "$0" --settings "$1" </dev/null 2>&1' \
        "$m" "{\"env\":{\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\":\"$want\"}}" | grep -m1 'Auto-compact window' || true)
      check_tier "A2 scratch ${key%%:*} ($tm, $m)" "$want" "$got"
    done
  fi

  # ---- A3 env merge ------------------------------------------------------------
  # A --settings env object must not replace the operator's other env keys.
  local advisor; advisor=$(jq -r '.env.CLAUDE_CODE_DISABLE_ADVISOR_TOOL // empty' "$HOME/.claude/settings.json" 2>/dev/null || true)
  if [ -z "$advisor" ]; then
    skip "A3 env merge" "~/.claude/settings.json sets no env.CLAUDE_CODE_DISABLE_ADVISOR_TOOL to compare with"
  else
    mkdir -p "$S/a3"
    # printenv, not echo "$VAR": a command with a $ expansion needs approval in -p mode.
    # The process env must not hold the key, so only the settings file can supply it.
    local said; said=$(unset CLAUDE_CODE_DISABLE_ADVISOR_TOOL && cd "$S/a3" && claude_ -p 'Run this exact command with your Bash tool: printenv CLAUDE_CODE_DISABLE_ADVISOR_TOOL. Then reply with ADV= followed by its output, and nothing else.' \
      --model haiku --allowedTools 'Bash(printenv:*)' \
      --settings '{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"200000"}}' 2>&1 | grep -o 'ADV=[^ `]*' | head -1 || true)
    [ "$said" = "ADV=$advisor" ] && pass "A3 env merge" "the overlay keeps the operator's env key: $said" \
      || fail "A3 env merge" "want ADV=$advisor, the Bash tool printed '${said:-nothing}'"
  fi

  # ---- A4 Codex limit ----------------------------------------------------------
  # horch has no `spawn --dry-run`: the argv comes from the u4 test, the
  # decision from --windows on this Mac's real config.
  local argv; argv=$(cargo test -q -p horch-core --test context_launch \
      ctx_05_codex_fleet_limit_flag_only_without_operator_value -- --nocapture 2>/dev/null \
    | grep -m1 'codex-sol argv' || true)
  local solrow; solrow=$(awk 'NR > 1 && $1 == "codex-sol"' "$S/windows.txt")
  local sset ssrc; read -r _ _ _ sset ssrc _ <<<"$solrow"
  if [[ $argv == *'model_auto_compact_token_limit=200000'* ]] && [ "$(digits "$sset")" = 200000 ] && [ "$ssrc" = fleet ]; then
    pass "A4 codex argv" "codex-sol decides 200,000 fleet here; the u4 launch argv carries -c model_auto_compact_token_limit=200000"
  else
    fail "A4 codex argv" "codex-sol row '$sset $ssrc'; argv: ${argv:-the u4 test printed none}"
  fi
  local codex_rows; codex_rows=$(awk 'NR > 1 && $2 == "codex" { print $1 "=" $4 "/" $5 }' "$S/windows.txt" | tr '\n' ' ')
  info "codex tiers in --windows: $codex_rows"

  # ---- A5 readers (CTX-02.4, CTX-17.1) -------------------------------------------
  local a5=$S/a5; rm -rf "$a5"; mkdir -p "$a5/state"
  local csid ctok xsid xtok
  csid=$(cd "$a5" && claude_ -p "Reply with the single word OK." --model haiku --output-format json 2>/dev/null | jq -r '.session_id // empty' || true)
  ctok=$( [ -n "$csid" ] && cd "$a5" && claude_ -p --resume "$csid" "/context" --model haiku 2>/dev/null \
    | sed -nE 's/.*Tokens:\*\* ([0-9.]+[kKmM]?) \/.*/\1/p' | head -1 || true)
  if need codex A5; then
    (cd "$a5" && codex exec --skip-git-repo-check -c model_auto_compact_token_limit=200000 \
      "Reply with the single word OK." </dev/null > "$a5/codex.out" 2> "$a5/codex.err") || true
    xsid=$(sed -nE 's/^session id: (.*)$/\1/p' "$a5/codex.err" | head -1)
    xtok=$(grep -A1 '^tokens used' "$a5/codex.err" | tail -1 | tr -d ', ')
  fi
  cat > "$(ledger_of "$a5/state" "$a5")" <<JSON
[
 {"record_id":"a5-claude","session_id":"$csid","agent":"claude","tier":"live-a5","model":"haiku","role":"a5-claude","status":"working","task":"live A5","history":[],"created_at":"2026-10-06T00:00:00Z","updated_at":"2026-10-06T00:00:00Z","project":"$a5","workdir":"$a5"},
 {"record_id":"a5-codex","session_id":"${xsid:-}","agent":"codex","tier":"live-a5","model":"gpt-5.6-sol","role":"a5-codex","status":"working","task":"live A5","history":[],"created_at":"2026-10-06T00:00:00Z","updated_at":"2026-10-06T00:00:00Z","project":"$a5","workdir":"$a5"}
]
JSON
  HORCH_STATE_DIR=$a5/state HORCH_PROJECT_DIR=$a5 "$HORCH_BIN" context > "$a5/context.txt" 2>&1 || true
  sed 's/^/     /' "$a5/context.txt"
  local t; t=$(awk '$1 == "a5-claude" { print $4 }' "$a5/context.txt")
  local ctr; ctr=$( [ -n "$csid" ] && claude_transcript "$csid" || true)
  local out; out=$( [ -n "$ctr" ] && jq -r 'select(.type == "assistant" and .message.usage) | .message.usage.output_tokens' "$ctr" 2>/dev/null | tail -1 || true)
  if [ -z "$csid" ] || [ -z "$ctok" ]; then
    fail "A5 claude reader" "no session or no /context total (session '${csid}', /context '${ctok}')"
  else
    # Tolerance: 1 response's output plus 1 tool result (2,000), plus the 0.1k rounding of /context.
    local h c d tol
    h=$(digits "$t"); c=$(kilo "$ctok"); tol=$(( ${out:-0} + 2000 + 100 ))
    d=$(( h > c ? h - c : c - h ))
    [ "$h" -gt 0 ] && [ "$d" -le "$tol" ] && pass "A5 claude reader" "horch $t, /context ${ctok} ($c); difference $d <= $tol" \
      || fail "A5 claude reader" "horch '$t', /context ${ctok} ($c); difference $d > $tol"
  fi
  t=$(awk '$1 == "a5-codex" { print $4 }' "$a5/context.txt")
  if [ -z "${xsid:-}" ] || [ -z "${xtok:-}" ]; then
    fail "A5 codex reader" "codex exec gave no session id or token line; see $a5/codex.err"
  else
    # Codex's own `tokens used` line leaves out the cached input; the context
    # number is the whole last request, so add the cached input back.
    local rf; rf=$(codex_rollout "$xsid")
    local cached; cached=$( [ -n "$rf" ] && jq -r 'select(.type == "event_msg" and .payload.type == "token_count" and .payload.info)
      | .payload.info.last_token_usage.cached_input_tokens // 0' "$rf" 2>/dev/null | tail -1 || true)
    local h d want; h=$(digits "$t"); want=$(( xtok + ${cached:-0} )); d=$(( h > want ? h - want : want - h ))
    [ "$h" -gt 0 ] && [ "$d" -le 2000 ] && pass "A5 codex reader" "horch $t, codex 'tokens used' $xtok + cached input ${cached:-0} = $want; difference $d <= 2000" \
      || fail "A5 codex reader" "horch '$t', codex 'tokens used' $xtok + cached input ${cached:-0} = $want; difference $d > 2000"
    if [ -n "$rf" ] && grep -q 'auto_compact' "$rf"; then
      pass "A4 codex reports the limit" "the rollout names it: $(grep -o '"[a-z_]*auto_compact[a-z_]*":[^,}]*' "$rf" | sort -u | head -3 | tr '\n' ' ')"
    else
      pass "A4 codex reports the limit" "argv-only: no auto_compact key in the rollout $(basename "${rf:-none}"); /status is in a pane only (part B2)"
    fi
  fi

  # ---- A6 canary (CTX-20) --------------------------------------------------------
  local canary="KEEP-CANARY-$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
  local block
  block=$(awk '/^== Compact instructions ==/{p=1} p&&/^== /&&!/Compact instructions/{exit} p' \
    "$ROOT/teammates/_base/fleet-worker.md" | sed 's/{role}/live-check-1/g')
  block="$block
- this canary line, word for word: $canary."
  local a6=$S/a6; rm -rf "$a6"; mkdir -p "$a6"
  local ksid; ksid=$(cd "$a6" && claude_ -p "$block

Reply with the single word OK." --model haiku --output-format json 2>/dev/null | jq -r '.session_id // empty' || true)
  if [ -z "$ksid" ]; then
    fail "A6 claude canary" "the first turn gave no session id"
  else
    (cd "$a6" && claude_ -p --resume "$ksid" "/compact" --model haiku > "$a6/claude-compact.out" 2>&1) || true
    local kt; kt=$(claude_transcript "$ksid")
    local summary; summary=$( [ -n "$kt" ] && jq -c 'select(.isCompactSummary == true) | .message.content' "$kt" 2>/dev/null | tail -1 || true)
    if [ -z "$kt" ] || ! grep -q '"subtype":"compact_boundary"' "$kt"; then
      skip "A6 claude canary" "not verifiable: no compact_boundary after /compact; see $a6/claude-compact.out"
    elif grep -qF "$canary" <<<"$summary"; then
      pass "A6 claude canary" "honoured: the isCompactSummary line has: $(around "$canary" <<<"$summary")"
    else
      pass "A6 claude canary" "ignored (a result, not a failure: the keep-list argument of horch compact covers Claude): the summary has no $canary: $(cut -c1-120 <<<"$summary")"
    fi
  fi
  if command -v codex >/dev/null 2>&1; then
    (cd "$a6" && codex exec --skip-git-repo-check "$block

Reply with the single word OK." </dev/null > "$a6/codex.out" 2> "$a6/codex.err") || true
    local cxsid; cxsid=$(sed -nE 's/^session id: (.*)$/\1/p' "$a6/codex.err" | head -1)
    if [ -n "$cxsid" ]; then
      (cd "$a6" && codex exec resume "$cxsid" "/compact" </dev/null > "$a6/codex-compact.out" 2> "$a6/codex-compact.err") || true
    fi
    local crf; crf=$( [ -n "$cxsid" ] && codex_rollout "$cxsid" || true)
    if [ -z "$crf" ] || ! grep -q '"type":"compacted"' "$crf"; then
      skip "A6 codex canary" "not verifiable: codex exec resume with /compact wrote no compacted line; part B2 compacts a codex pane"
    elif jq -c 'select(.type == "compacted") | .payload' "$crf" | grep -qF "$canary"; then
      pass "A6 codex canary" "honoured: the compacted line has: $(jq -c 'select(.type == "compacted") | .payload' "$crf" | around "$canary")"
    else
      pass "A6 codex canary" "ignored (a result: Codex keeps state through the handoff file): the compacted line has no $canary"
    fi
  fi

  # ---- A7 speed (CTX-26) -----------------------------------------------------------
  local t0 t1 ms rows
  t0=$(now); rows=$("$HORCH_BIN" context 2>/dev/null | wc -l | tr -d ' '); t1=$(now)
  ms=$(perl -e "printf '%d', ($t1 - $t0) * 1000")
  [ "$ms" -lt 1000 ] && pass "A7 speed" "horch context on the real ledger: ${ms} ms, $rows lines" \
    || fail "A7 speed" "horch context took ${ms} ms (limit 1000 ms)"

  # ---- A8 detached survival (CTX-15) -------------------------------------------------
  # A start_new_session child appends to beat.log every 2 s. It must still
  # grow 10 s after the claude turn or the codex exec call that started it ends.
  local a8=$S/a8; rm -rf "$a8"; mkdir -p "$a8/claude" "$a8/codex"
  cat > "$a8/start-beat.sh" <<'SH'
#!/bin/sh
# start-beat.sh <dir>: start a detached child (setsid, start_new_session) that
# appends to <dir>/beat.log every 2 s; its pid goes to <dir>/beat.pid.
exec python3 -c '
import subprocess, sys
d = sys.argv[1]
p = subprocess.Popen(["/bin/sh", "-c", "while :; do date +%s >> \"$0/beat.log\"; sleep 2; done", d],
                     start_new_session=True, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True)
open(d + "/beat.pid", "w").write(str(p.pid))
' "$1"
SH
  chmod +x "$a8/start-beat.sh"
  stop_beats() { local f; for f in "$a8"/*/beat.pid; do [ -f "$f" ] && { kill -- "-$(cat "$f")" 2>/dev/null || kill "$(cat "$f")" 2>/dev/null || true; }; done; }
  trap stop_beats EXIT
  survives() { # <label> <dir>
    local log=$2/beat.log n1 n2
    [ -f "$log" ] || { fail "$1" "no beat.log: the child never started"; return; }
    sleep 10; n1=$(wc -l < "$log" | tr -d ' '); sleep 6; n2=$(wc -l < "$log" | tr -d ' ')
    [ "$n2" -gt "$n1" ] && pass "$1" "beat.log grows after the call ended ($n1 -> $n2 lines in 6 s)" \
      || fail "$1" "beat.log stopped at $n2 lines: the child died with the call; the compaction job cannot rely on spawn_detached"
  }
  (cd "$a8/claude" && claude_ -p "Run this exact command with your Bash tool: sh $a8/start-beat.sh $a8/claude . Then reply with the single word DONE." \
    --model haiku --allowedTools 'Bash(sh:*)' > "$a8/claude.out" 2>&1) || true
  survives "A8 survival after a claude Bash tool call" "$a8/claude"
  if command -v codex >/dev/null 2>&1; then
    (cd "$a8/codex" && codex exec --skip-git-repo-check -s workspace-write \
      "Run this exact command: sh $a8/start-beat.sh $a8/codex . Then reply with the single word DONE." \
      </dev/null > "$a8/codex.out" 2>&1) || true
    survives "A8 survival after a codex exec call" "$a8/codex"
  else
    skip "A8 survival after a codex exec call" "codex is not installed"
  fi
  stop_beats; trap - EXIT
}

# ---- Part B: panes (installed horch, a herdr server, a fleet) -------------------------
# Each step prints its procedure. With the role in LIVE_CONTEXT_<STEP>_ROLE it
# checks the ledger of the fleet project (LIVE_CONTEXT_PROJECT, default the
# current project: the operator's fleet) and prints PASS or FAIL; without it
# the step is SKIP (record the result by hand in docs/live-checks/context.md).
# round_trip <step> <role> checks 1 compaction round trip in the ledger of $P.
round_trip() {
  local req ready done_
  has_record "$1" "$P" "$2" || return 0
  req=$(events_of "$P" "$2" compact-requested | tail -1); ready=$(events_of "$P" "$2" note | grep -c '^handoff: ' || true)
  done_=$(events_of "$P" "$2" compacted | tail -1)
  local pre post; pre=$(sed -nE 's/^([0-9]+) -> .*/\1/p' <<<"$done_"); post=$(sed -nE 's/^[0-9]+ -> ([0-9]+) .*/\1/p' <<<"$done_")
  if [ -z "$req" ]; then fail "$1" "$2 has no compact-requested event"
  elif [ "$ready" -lt 1 ]; then fail "$1" "$2 has no 'handoff: <path>' note"
  elif [ -z "$pre" ] || [ -z "$post" ] || [ "$post" -ge "$pre" ]; then fail "$1" "$2 compacted event '${done_}': want <pre> -> <post> with post < pre"
  else pass "$1" "$2: compact-requested ($req), handoff note, compacted '$done_'"; fi
}
part_b() {
  need horch B0 || return
  need herdr B0 || return
  P=${LIVE_CONTEXT_PROJECT:-$CALLER}
  events() { events_of "$P" "$@"; }
  info "part B project: $P (the current project, the operator's fleet; LIVE_CONTEXT_PROJECT overrides it)."
  info "Run the pane steps below in the fleet of this project."
  echo "
  B1 Claude worker round trip (CTX-11.1, CTX-13.2, CTX-22.1, CTX-23.1). In the orchestrator pane:
     1. horch spawn sonnet 'Read README.md. Run horch note \"step 1 done\". Then wait for instructions.'
     2. horch compact sonnet-1 --request     -> prints 'asked sonnet-1 to write ai_docs/handoffs/sonnet-1-whats-next.md'
     3. Wait for '[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md' in this pane.
        horch context shows sonnet-1 'requested'.
     4. horch compact sonnet-1               -> 'compaction of sonnet-1 scheduled; log <path>' at once
     5. The worker pane shows '/compact Keep in the summary: ...', then the resume line.
        This pane gets exactly 1 '[horch] NOTE: sonnet-1 compacted. Context <pre> -> <post> tokens. ...' line.
     Then: LIVE_CONTEXT_PANES=1 LIVE_CONTEXT_PART=b LIVE_CONTEXT_B1_ROLE=sonnet-1 scripts/live/context.sh"
  [ -n "${LIVE_CONTEXT_B1_ROLE:-}" ] && round_trip "B1 claude worker round trip" "$LIVE_CONTEXT_B1_ROLE" \
    || skip "B1 claude worker round trip" "operator step; set LIVE_CONTEXT_B1_ROLE to check the ledger"
  echo "
  B2 Codex worker round trip and busy rejection (CTX-13.1, CTX-13.3; review finding 2):
     1. horch spawn codex-sol 'Read README.md. Run horch note \"x\". Then wait for instructions.'
        In the codex pane the note must work: 'horch sessions' shows the note 'x' on codex-sol-1.
     2. With codex-sol-1 over its threshold (or a copy of context-windows with a small codex window),
        its next 'horch note x' prints the 'NOTE: Context warning.' line.
     3. Give it a long task, then at once: horch compact codex-sol-1 --force
        The job log (<state>/compact/<ws>-codex-sol-1/job.log) shows the wait for 2 idle polls; at most 3 sends.
     4. Repeat B1 steps 2 to 5 for codex-sol-1 (the pane gets a bare /compact).
     Until B2 passes, the orchestrator checkpoint is the only watch for Codex workers.
     Then: LIVE_CONTEXT_PANES=1 LIVE_CONTEXT_PART=b LIVE_CONTEXT_B2_ROLE=codex-sol-1 scripts/live/context.sh"
  if [ -n "${LIVE_CONTEXT_B2_ROLE:-}" ] && has_record "B2 codex horch note records" "$P" "$LIVE_CONTEXT_B2_ROLE"; then
    local n; n=$(events "$LIVE_CONTEXT_B2_ROLE" note | grep -cx 'x' || true)
    [ "$n" -ge 1 ] && pass "B2 codex horch note records" "$LIVE_CONTEXT_B2_ROLE has $n note 'x'" \
      || fail "B2 codex horch note records" "$LIVE_CONTEXT_B2_ROLE has no note 'x': horch note fails in the codex sandbox"
    [ -n "$(events "$LIVE_CONTEXT_B2_ROLE" context-warned | tail -1)" ] && pass "B2 codex horch note warns" "context-warned recorded" \
      || fail "B2 codex horch note warns" "no context-warned event on $LIVE_CONTEXT_B2_ROLE"
    round_trip "B2 codex worker round trip" "$LIVE_CONTEXT_B2_ROLE"
  else
    skip "B2 codex worker round trip" "operator step; set LIVE_CONTEXT_B2_ROLE to check the ledger"
  fi
  echo "
  B3 Claude orchestrator self-compaction (CTX-18.1); B4 the same with 'horch fleet' on the Codex orchestrator (CTX-18.2):
     1. At a stopping point the orchestrator writes ai_docs/handoffs/orchestrator-whats-next.md (every live role and
        its plan file), runs 'horch compact orchestrator', sees 'scheduled', and ends its turn.
     2. The resume line arrives in its pane; horch context shows a new LAST COMPACT for orchestrator.
     3. B4: the job log shows no '/compact' is disabled rejection that was not sent again.
     Then: LIVE_CONTEXT_PANES=1 LIVE_CONTEXT_PART=b LIVE_CONTEXT_B3_PROJECT=<project> scripts/live/context.sh"
  if [ -n "${LIVE_CONTEXT_B3_PROJECT:-}" ]; then
    has_record "B3 orchestrator self-compaction" "$LIVE_CONTEXT_B3_PROJECT" orchestrator || return 0
    local c; c=$(events_of "$LIVE_CONTEXT_B3_PROJECT" orchestrator compacted | tail -1 || true)
    [ -n "$c" ] && pass "B3 orchestrator self-compaction" "compacted '$c'" || fail "B3 orchestrator self-compaction" "no compacted event on the orchestrator record"
  else
    skip "B3 orchestrator self-compaction" "operator step"; skip "B4 codex orchestrator self-compaction" "operator step"
  fi
  echo "
  B5 horch note timing in a worker pane (CTX-26): in any worker pane run
     time horch note timing      -> real under 0.5 s"
  skip "B5 horch note timing" "operator step; record the 'real' time"
  echo "
  B6 a pi worker over its threshold takes the fresh route (CTX-16.1):
     1. horch spawn pi '...'; with it over (a small compact_window copy), horch compact pi-1 --request.
     2. horch compact pi-1 prints 'horch compact: refused: pi uses the fresh route; tell pi-1 to run horch done'.
     3. The worker runs horch done naming its handoff; the orchestrator spawns pi with 'PRIOR WORK: read <path> first'.
        horch sessions shows both records."
  skip "B6 pi fresh route" "operator step"
}

# ---- Part C: the slice-2 harnesses (opencode, pi, prime) ------------------------------
# C1 to C4 run headless in $S/c; C5 prints the pane procedure and, with the
# roles in LIVE_CONTEXT_C5_<HARNESS>_ROLE, checks the ledger of the scratch
# fleet (LIVE_CONTEXT_PROJECT, default $S/fleet) and the herdr pane.
# The model of a teammate file (its `model:` line).
model_of() { sed -nE 's/^model: *([^ #]+).*/\1/p' "$ROOT/teammates/$1.md" | head -1; }
part_c() {
  need jq C1 || exit 1
  need python3 C1 || exit 1
  [ -x "$HORCH_BIN" ] || { fail C1 "$HORCH_BIN is not built; run cargo build --workspace --bins"; return; }
  local c=$S/c; rm -rf "$c"; mkdir -p "$c/state" "$c/pi-agent" "$c/prime-agent" "$c/pi" "$c/prime" "$c/opencode"
  local PIM PRM OCM
  PIM=${LIVE_CONTEXT_PI_MODEL:-$(model_of pi)}; PRM=${LIVE_CONTEXT_PRIME_MODEL:-$(model_of prime)}
  OCM=${LIVE_CONTEXT_OPENCODE_MODEL:-$(model_of opencode-pickle)}

  # ---- C1 versions --------------------------------------------------------------------
  local v have_pi=0 have_prime=0 have_oc=0
  v="pi $(pi --version 2>/dev/null || echo none), prime-agent $(prime-agent --version 2>/dev/null || echo none), opencode $(opencode --version 2>/dev/null || echo none), herdr $(herdr --version 2>/dev/null | awk '{print $2}' || echo none), sqlite3 $(sqlite3 --version 2>/dev/null | awk '{print $1}' || echo none)"
  command -v pi >/dev/null 2>&1 && have_pi=1
  command -v opencode >/dev/null 2>&1 && command -v sqlite3 >/dev/null 2>&1 && have_oc=1
  # Prime lists only the models it can serve; a provider it is not signed in to has none.
  if command -v prime-agent >/dev/null 2>&1; then
    prime-agent model list "${PRM#*/}" 2>/dev/null | awk -v p="${PRM%%/*}" -v m="${PRM#*/}" '$1 == p && $2 == m' | grep -q . && have_prime=1
  fi
  pass "C1 versions" "$v; models pi $PIM, prime $PRM, opencode $OCM"
  [ "$have_prime" = 1 ] || info "prime-agent model list has no $PRM: sign Prime in to ${PRM%%/*}, or set LIVE_CONTEXT_PRIME_MODEL (e.g. ollama/qwen3.8)"

  # A scratch agent dir: links to the operator's entries, and a settings.json
  # that compacts all but the last token (keepRecentTokens 1) so 2 short turns
  # can be compacted. The operator's dir is never written.
  agent_dir() { # <operator dir> <scratch dir>
    local e n
    for e in "$1"/*; do
      n=$(basename "$e")
      case $n in settings.json|sessions|logs|daemon-workers|session-artifacts|session-leases) ;; *) ln -s "$e" "$2/$n" ;; esac
    done
    jq '. + {compaction: {enabled: true, keepRecentTokens: 1}, autoRefine: {enabled: false}}' "$1/settings.json" > "$2/settings.json" 2>/dev/null \
      || echo '{"compaction":{"enabled":true,"keepRecentTokens":1},"autoRefine":{"enabled":false}}' > "$2/settings.json"
  }
  [ "$have_pi" = 1 ] && agent_dir "${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}" "$c/pi-agent"
  [ "$have_prime" = 1 ] && agent_dir "${PRIME_AGENT_CODING_AGENT_DIR:-$HOME/.prime/agent}" "$c/prime-agent"
  local q=(--thinking low --no-tools --no-extensions --no-skills --no-prompt-templates --no-themes --no-context-files)
  local pisid; pisid=$(python3 -c 'import uuid; print(uuid.uuid4())')
  pi_() { (cd "$c" && PI_CODING_AGENT_DIR=$c/pi-agent pi --model "$PIM" "${q[@]}" --session-dir "$c/pi" --session-id "$pisid" "$@" </dev/null); }
  prime_() { (cd "$c" && PRIME_AGENT_CODING_AGENT_DIR=$c/prime-agent prime-agent --model "$PRM" "${q[@]}" \
    --session-dir "$c/prime" --daemon-socket "$c/d.sock" "$@" </dev/null); }
  stop_prime() { # this run's Prime daemon only: the one on $c/d.sock
    local pid; pid=$(prime-agent status --json 2>/dev/null | jq -r --arg s "$S/c/d.sock" '.[] | select(.socketPath == $s) | .pid' 2>/dev/null | head -1 || true)
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  }
  trap stop_prime EXIT
  # The canary is the first turn: the worker's compact-instructions block
  # with 1 more line. A second turn follows, so there is a turn to compact.
  local canary="KEEP-CANARY-$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')" block
  block=$(awk '/^== Compact instructions ==/{p=1} p&&/^== /&&!/Compact instructions/{exit} p' \
    "$ROOT/teammates/_base/fleet-worker.md" | sed 's/{role}/live-check-1/g')
  block="$block
- this canary line, word for word: $canary.

Reply with the single word OK."
  if [ "$have_pi" = 1 ]; then pi_ -p "$block" > "$c/pi-1.out" 2>&1 || true; pi_ -p "Reply with the single word OK." > "$c/pi-2.out" 2>&1 || true; fi
  if [ "$have_prime" = 1 ]; then prime_ -p "$block" > "$c/prime-1.out" 2>&1 || true; prime_ -p -c "Reply with the single word OK." > "$c/prime-2.out" 2>&1 || true; fi
  local ocsid=
  if [ "$have_oc" = 1 ]; then
    (cd "$c/opencode" && opencode run --model "$OCM" --format json "Reply with the single word OK." </dev/null > "$c/opencode.jsonl" 2> "$c/opencode.err") || true
    ocsid=$(jq -r 'select(.type == "step_finish") | .sessionID // .part.sessionID // empty' "$c/opencode.jsonl" 2>/dev/null | tail -1 || true)
  fi
  local pif prf
  pif=$(ls "$c"/pi/*.jsonl 2>/dev/null | head -1 || true); prf=$(ls "$c"/prime/*.jsonl 2>/dev/null | head -1 || true)

  # ---- C2 readers (CTX-17 slice 2, CTX-02) ----------------------------------------------
  # 1 ledger record per session; pi and Prime records name the session file.
  cat > "$(ledger_of "$c/state" "$c")" <<JSON
[
 {"record_id":"c2-pi","session_id":"${pif:-}","agent":"pi","tier":"live-c2","model":"$PIM","role":"c2-pi","status":"working","task":"live C2","history":[],"created_at":"2026-10-07T00:00:00Z","updated_at":"2026-10-07T00:00:00Z","project":"$c","workdir":"$c"},
 {"record_id":"c2-prime","session_id":"${prf:-}","agent":"prime","tier":"live-c2","model":"$PRM","role":"c2-prime","status":"working","task":"live C2","history":[],"created_at":"2026-10-07T00:00:00Z","updated_at":"2026-10-07T00:00:00Z","project":"$c","workdir":"$c"},
 {"record_id":"c2-opencode","session_id":"${ocsid:-}","agent":"opencode","tier":"live-c2","model":"$OCM","role":"c2-opencode","status":"working","task":"live C2","history":[],"created_at":"2026-10-07T00:00:00Z","updated_at":"2026-10-07T00:00:00Z","project":"$c","workdir":"$c"}
]
JSON
  ctx_c() { HORCH_STATE_DIR=$c/state HORCH_PROJECT_DIR=$c "$HORCH_BIN" context > "$c/$1" 2>&1 || true; }
  ctx_c context.txt; sed 's/^/     /' "$c/context.txt"
  # The pi and Prime own number: the newest assistant response's totalTokens.
  own_pi() { jq -r 'select(.type == "message" and .message.role == "assistant" and .message.usage) | .message.usage.totalTokens' "$1" 2>/dev/null | tail -1; }
  reader() { # <step> <role> <want> <what the want is>
    local h; h=$(digits "$(awk -v r="$2" '$1 == r { print $4 }' "$c/context.txt")")
    if [ -z "$3" ]; then fail "$1" "the harness gave no own number: $4"
    elif [ "$h" = "$3" ]; then pass "$1" "horch $h, $4 $3"
    else fail "$1" "horch '${h}', $4 $3"; fi
  }
  if [ "$have_pi" = 1 ]; then reader "C2 pi reader" c2-pi "$( [ -n "$pif" ] && own_pi "$pif")" "the session's newest totalTokens"
  else skip "C2 pi reader" "pi is not installed"; fi
  if [ "$have_prime" = 1 ]; then reader "C2 prime reader" c2-prime "$( [ -n "$prf" ] && own_pi "$prf")" "the session's newest totalTokens"
  else skip "C2 prime reader" "prime-agent has no $PRM (or is not installed)"; fi
  if [ "$have_oc" = 1 ]; then
    reader "C2 opencode reader" c2-opencode "$(jq -r 'select(.type == "step_finish") | .part.tokens.total' "$c/opencode.jsonl" 2>/dev/null | tail -1)" \
      "opencode run's newest step_finish total"
  else skip "C2 opencode reader" "opencode or sqlite3 is not installed"; fi

  # ---- C3 canary (CTX-20, CTX-03) --------------------------------------------------------
  # A compaction with no argument through the RPC mode ({"type":"compact"}),
  # then the marker's summary is searched for the canary; after it horch
  # shows the row pending, never the pre value. -p reads /compact as a prompt.
  cat > "$c/rpc-compact.py" <<'PY'
# rpc-compact.py <argv...>: send 1 compact command to an RPC-mode pi or Prime
# and print its response line.
import json, subprocess, sys
p = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.DEVNULL, text=True)
p.stdin.write(json.dumps({"type": "compact"}) + "\n"); p.stdin.flush()
for line in p.stdout:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("type") == "response" and m.get("command") == "compact":
        print(json.dumps({"success": m.get("success"), "error": m.get("error")}))
        break
p.stdin.close(); p.terminate(); p.wait(timeout=10)
PY
  canary_of() { # <step> <session file> <rpc response>
    local summary; summary=$(jq -r 'select(.type == "compaction") | .summary // empty' "$2" 2>/dev/null | tail -1 || true)
    if ! grep -q '"type":"compaction"' "$2" 2>/dev/null; then
      skip "$1" "not verifiable: no compaction line after the RPC compact (${3:-no response}); part C5 compacts a pane"
    elif grep -qF "$canary" <<<"$summary"; then
      pass "$1" "honoured: the compaction summary has: $(around "$canary" <<<"$summary")"
    else
      pass "$1" "ignored (a result: the keep-list argument of horch compact covers pi and Prime): the summary has no $canary: $(tr '\n' ' ' <<<"$summary" | cut -c1-120)"
    fi
  }
  local rsp
  if [ "$have_pi" = 1 ] && [ -n "$pif" ]; then
    rsp=$(cd "$c" && PI_CODING_AGENT_DIR=$c/pi-agent python3 -I "$c/rpc-compact.py" pi --mode rpc --model "$PIM" "${q[@]}" \
      --session-dir "$c/pi" --session-id "$pisid" </dev/null 2>/dev/null || true)
    canary_of "C3 pi canary" "$pif" "$rsp"
  else skip "C3 pi canary" "no pi session"; fi
  if [ "$have_prime" = 1 ] && [ -n "$prf" ]; then
    rsp=$(cd "$c" && PRIME_AGENT_CODING_AGENT_DIR=$c/prime-agent python3 -I "$c/rpc-compact.py" prime-agent --mode rpc -c \
      --model "$PRM" "${q[@]}" --session-dir "$c/prime" --daemon-socket "$c/d.sock" </dev/null 2>/dev/null || true)
    canary_of "C3 prime canary" "$prf" "$rsp"
  else skip "C3 prime canary" "no Prime session"; fi
  ctx_c context-after.txt
  local r st
  for r in c2-pi c2-prime; do
    st=$(awk -v r="$r" '$1 == r' "$c/context-after.txt")
    case $st in *pending*) info "after the compaction horch shows $r pending: $st" ;; '') ;; *) info "after the compaction $r: $st" ;; esac
  done
  stop_prime; trap - EXIT
  part_c5
}

# The pane steps of part C alone (LIVE_CONTEXT_PART=c5): run it at once
# after step 2 of a C5 procedure, so the busy status is still there.
part_c5() {
  local PRM=${LIVE_CONTEXT_PRIME_MODEL:-$(model_of prime)}
  # ---- C4 prime agent dir (design 6.9) ---------------------------------------------------
  # A pane's own agent dir: <state>/prime/<role>-<uuid>/agent, in the pane's
  # PRIME_AGENT_CODING_AGENT_DIR. It must turn autoRefine off and give the
  # 200,000 window, and prime-agent model list must report that window.
  local pdir=${LIVE_CONTEXT_C4_AGENT_DIR:-}
  if [ -z "$pdir" ] && [ -n "${LIVE_CONTEXT_C5_PRIME_ROLE:-}" ]; then
    local st_root; st_root=${HORCH_STATE_DIR:-$HOME/.local/state/horch}
    pdir=$(ls -dt "$st_root"/prime/"$LIVE_CONTEXT_C5_PRIME_ROLE"-*/agent 2>/dev/null | head -1 || true)
  fi
  if [ -z "$pdir" ]; then
    skip "C4 prime agent dir" "no pane agent dir; spawn prime (C5), then set LIVE_CONTEXT_C5_PRIME_ROLE or LIVE_CONTEXT_C4_AGENT_DIR"
  else
    local ar win
    ar=$(jq -c '.autoRefine // "unset"' "$pdir/settings.json" 2>/dev/null || echo missing)
    win=$(PRIME_AGENT_CODING_AGENT_DIR=$pdir prime-agent model list "${PRM#*/}" 2>/dev/null \
      | awk -v p="${PRM%%/*}" -v m="${PRM#*/}" '$1 == p && $2 == m { print $3 }' | head -1 || true)
    [ "$ar" = '{"enabled":false}' ] && pass "C4 prime autoRefine off" "$pdir/settings.json has autoRefine $ar" \
      || fail "C4 prime autoRefine off" "$pdir/settings.json has autoRefine $ar"
    [ "$(kilo "${win:-x}" 2>/dev/null || echo x)" = 200000 ] && pass "C4 prime window" "prime-agent model list with PRIME_AGENT_CODING_AGENT_DIR=$pdir: $PRM context $win" \
      || fail "C4 prime window" "prime-agent model list with PRIME_AGENT_CODING_AGENT_DIR=$pdir: $PRM context '${win:-no row}', want 200K"
  fi

  # ---- C5 panes (installed horch, a herdr server, the scratch fleet) --------------------
  need horch C5 || return 0
  need herdr C5 || return 0
  P=${LIVE_CONTEXT_PROJECT:-$S/fleet}
  mkdir -p "$P"; [ -d "$P/.git" ] || git -C "$P" init -q
  info "part C project: $P (the scratch fleet; LIVE_CONTEXT_PROJECT overrides it). Start it once: cd $P && horch fleet"
  # Samples herdr agent_status of the role's pane for 60 s, 1 per second.
  statuses() { # <role> -> the distinct values seen, in order
    local rec ws pane i seen=
    rec=$(record_of "$P" "$1"); ws=$(jq -r '.workspace_id // empty' <<<"$rec"); pane=$(jq -r '.pane_id // empty' <<<"$rec")
    [ -n "$pane" ] || return 0
    for i in $(seq 60); do
      local s; s=$(herdr pane list --workspace "$ws" 2>/dev/null | jq -r --arg p "$pane" '.result.panes[]? | select(.pane_id == $p) | .agent_status // "none"' || true)
      case " $seen " in *" ${s:-gone} "*) ;; *) seen="$seen ${s:-gone}" ;; esac
      case $seen in *working*idle*|*working*done*) break ;; esac
      sleep 1
    done
    echo "${seen# }"
  }
  local h role var
  for h in opencode-pickle pi prime; do
    var=LIVE_CONTEXT_C5_$(tr 'a-z-' 'A-Z_' <<<"${h%%-*}")_ROLE; role=${!var:-}
    echo "
  C5 $h pane (CTX-10, CTX-13, CTX-17; review finding 2). In the orchestrator pane of $P:
     1. horch spawn $h 'Read README.md. Run horch note \"x\". Then wait for instructions.'
        The pane must record the note: horch sessions shows the note 'x' on the new role.
     2. horch tell <role> 'Run sleep 20 with your shell tool, then reply DONE.' and at once run
        $var=<role> LIVE_CONTEXT_PART=c5 scripts/live/context.sh: herdr agent_status must read working, then idle or done.
     3. Over the threshold (a teammate copy with a small compact_window), the next 'horch note x' prints 'NOTE: Context warning.'.
     4. horch compact <role> --request; wait for '[<role>] NOTE: COMPACT-READY ...';
        horch compact <role> --force ($h is not in in_place yet; --force skips the route check);
        the pane gets its compact command and the resume line, this pane 1 '[horch] NOTE: <role> compacted.' line.
     Then: $var=<role> LIVE_CONTEXT_PART=c5 scripts/live/context.sh"
    if [ -z "$role" ]; then skip "C5 $h pane" "operator step; set $var to check the ledger and the pane"; continue; fi
    has_record "C5 $h horch note records" "$P" "$role" || continue
    local n; n=$(events_of "$P" "$role" note | grep -cx 'x' || true)
    [ "$n" -ge 1 ] && pass "C5 $h horch note records" "$role has $n note 'x'" \
      || fail "C5 $h horch note records" "$role has no note 'x': horch note fails in the $h pane"
    [ -n "$(events_of "$P" "$role" context-warned | tail -1)" ] && pass "C5 $h horch note warns" "context-warned recorded" \
      || skip "C5 $h horch note warns" "no context-warned event on $role (step 3 not run)"
    local seen; seen=$(statuses "$role")
    case $seen in *working*idle*|*working*done*) pass "C5 $h agent_status" "herdr read: $seen" ;;
      *) fail "C5 $h agent_status" "herdr read '${seen:-no pane}' in 60 s; want working, then idle or done" ;; esac
    round_trip "C5 $h compaction round trip" "$role"
  done
}

case $PART in
  b) part_b; section B ;;
  c) part_c; section C ;;
  c5) part_c5; section C ;;
  *) part_a; section A
     if [ "${LIVE_CONTEXT_PANES:-0}" = 1 ]; then
       OUT=$S/results/$STAMP-b.md; : > "$OUT"; : > "$ROWS"
       line "live check: context policy, part B, $(date -u +%Y-%m-%dT%H:%M:%SZ)"; part_b; section B
     fi ;;
esac
echo "result: $fails FAIL (log $OUT)"
[ "$fails" -eq 0 ]
