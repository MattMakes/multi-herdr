#!/usr/bin/env bash
# Live check: the harness CLIs on this Mac (U-28, U-60..U-64, U-67, U-30 Q1).
# Antigravity (agy) flags, model, resume and trust store; OpenCode --prompt on
# a resume; the Prime status record; pi on the installed Node.
# Result table: docs/live-checks/harnesses.md. The gate never runs this.
#
# Usage: scripts/live/harnesses.sh
#        AGY_READ_CACHE=1 scripts/live/harnesses.sh  (also read agy's
#        last_conversations.json; it lists your own workspaces)
# Cost: 3 small agy turns on the operator's Google login (a 4th is refused
# before it starts) and 2 small opencode turns on a free tier. Both train on
# input: every prompt is dummy text in a scratch repo that holds 1 dummy file.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCR="$ROOT/.worktrees/_scratch/live-harnesses"
AGY_HOME_DIR="$HOME/.gemini/antigravity-cli"
OC_MODEL="${OC_MODEL:-opencode/nemotron-3.5-lightning-free}"
AGY_MODEL="${AGY_MODEL:-gemini-3.8-flash}"
FAILS=0

pass() { echo "PASS $1${2:+: $2}"; }
fail() { echo "FAIL $1: $2"; FAILS=$((FAILS + 1)); }
skip() { echo "SKIP $1: $2"; }

# within <seconds> <cmd...>: run with a time limit (macOS has no timeout(1)).
within() { local s="$1"; shift; perl -e 'alarm shift; exec @ARGV' "$s" "$@"; }

# clean <seconds> <cmd...>: within, and never pass a key to a harness: the
# Anthropic one is banned in this project, and the Google ones move agy off
# the operator's login.
clean() {
  local s="$1"; shift
  env -u ANTHROPIC_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GOOGLE_GEMINI_BASE_URL \
    perl -e 'alarm shift; exec @ARGV' "$s" "$@"
}

# scratch_repo <dir>: a fresh git repo with 1 dummy file.
scratch_repo() {
  rm -rf "$1"
  mkdir -p "$1"
  (cd "$1" && git init -q && echo "hello dummy" >dummy.txt && git add dummy.txt &&
    git -c user.name=live -c user.email=live@example.invalid commit -qm init)
}

# json_get <key>: one top-level field of the JSON object on stdin.
json_get() { python3 -c 'import json,sys; t=sys.stdin.read(); print(json.loads(t[t.index("{"):]).get(sys.argv[1], ""))' "$1"; }

# ---------------------------------------------------------------- Antigravity
if ! command -v agy >/dev/null 2>&1; then
  skip agy "agy is not installed (curl -fsSL https://antigravity.google/cli/install.sh | bash)"
else
  AGY_VERSION="$(agy --version 2>&1 | head -1)"
  pass agy-version "$AGY_VERSION"

  HELP="$(agy --help 2>&1)"
  missing=""
  for f in --prompt-interactive --model --effort --mode --sandbox --dangerously-skip-permissions --conversation; do
    grep -q -- "^ *$f " <<<"$HELP" || missing="$missing $f"
  done
  if [ -z "$missing" ]; then pass agy-flags "all 7 adapter flags are in agy --help"; else fail agy-flags "missing:$missing"; fi

  MODELS="$(cd "$ROOT" && clean 60 agy models 2>&1 || true)"
  if grep -q "^$AGY_MODEL-medium" <<<"$MODELS"; then
    pass agy-model "agy models lists $AGY_MODEL-low|medium|high"
  elif grep -qi "auth" <<<"$MODELS"; then
    skip agy-model "agy needs a login: run agy once and finish the Google sign-in"
  else
    fail agy-model "$AGY_MODEL-medium is not in agy models"
  fi

  SETTINGS="$AGY_HOME_DIR/settings.json"
  if [ -f "$SETTINGS" ]; then
    # Key names and a count only: never print the values.
    shape="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); t=d.get("trustedWorkspaces"); print("provider" if "modelProvider" in d else "", type(t).__name__, all(isinstance(x,str) for x in t) if isinstance(t,list) else "")' "$SETTINGS")"
    case "$shape" in
      provider*) fail agy-no-api-key "settings.json sets modelProvider: agy may need GEMINI_API_KEY" ;;
      *) pass agy-no-api-key "settings.json has no modelProvider" ;;
    esac
    case "$shape" in
      *"list True") pass agy-trust-store "trustedWorkspaces is a list of path strings" ;;
      *) fail agy-trust-store "trustedWorkspaces is not a list of strings ($shape)" ;;
    esac
  else
    skip agy-trust-store "no $SETTINGS: run agy once"
  fi

  REPO="$SCR/agy"
  scratch_repo "$REPO"
  # Print mode: the TUI would stop at the trust question in a new directory.
  # No permission flags: the turn needs no tool.
  first="$(cd "$REPO" && clean 180 agy --model "$AGY_MODEL" --effort medium \
    -p "Remember the word ZEBRA. Reply with the single word PONG." --output-format json 2>/dev/null || true)"
  CID="$(json_get conversation_id <<<"$first" 2>/dev/null || true)"
  if [ "$(json_get status <<<"$first" 2>/dev/null || true)" = "SUCCESS" ] && [ -n "$CID" ]; then
    pass agy-turn "--model $AGY_MODEL --effort medium: SUCCESS, conversation_id minted by agy"
    again="$(cd "$REPO" && clean 180 agy --model "$AGY_MODEL" --effort medium --conversation "$CID" \
      -p "Which word did I ask you to remember? Reply with that word only." --output-format json 2>/dev/null || true)"
    if grep -q ZEBRA <<<"$(json_get response <<<"$again" 2>/dev/null || true)" &&
      [ "$(json_get conversation_id <<<"$again" 2>/dev/null || true)" = "$CID" ]; then
      pass agy-resume "--conversation <id> keeps the id and recalls the first turn"
    else
      fail agy-resume "the resumed turn did not recall the word"
    fi
    plan="$(cd "$REPO" && clean 180 agy --model "$AGY_MODEL" --effort low --mode plan -p "Reply OK." --output-format json 2>/dev/null || true)"
    if [ "$(json_get status <<<"$plan" 2>/dev/null || true)" = "SUCCESS" ]; then
      pass agy-mode-plan "--mode plan is accepted"
    else
      fail agy-mode-plan "--mode plan failed"
    fi
    if python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if sys.argv[2] in d.get("trustedWorkspaces",[]) else 1)' "$SETTINGS" "$REPO" 2>/dev/null; then
      fail agy-print-no-trust "print mode added the scratch repo to trustedWorkspaces"
    else
      pass agy-print-no-trust "print mode runs in an untrusted directory and records no trust"
    fi
  else
    skip agy-turn "agy print mode did not succeed (log in: run agy once)"
  fi

  # Effort on a model id that agy does not list bare: refused (seen on 1.2.17).
  bad="$(cd "$REPO" && clean 60 agy --model gemini-3-1-pro --effort low -p "x" --output-format json 2>/dev/null || true)"
  if grep -q "not supported" <<<"$(json_get error <<<"$bad" 2>/dev/null || true)"; then
    pass agy-effort-refusal "agy refuses --effort for an id it does not list (gemini-3-1-pro)"
  else
    skip agy-effort-refusal "agy accepted or did not answer"
  fi

  # Usage: file names only. A token count would need a file with "usage" or
  # "quota" in its name, or a table column.
  usage="$(find "$AGY_HOME_DIR" -maxdepth 3 \( -iname '*usage*' -o -iname '*quota*' \) 2>/dev/null | head -3)"
  if [ -n "$usage" ]; then
    fail agy-no-usage-file "a usage file now exists; add a reader (U-60): $usage"
  else
    pass agy-no-usage-file "no usage or quota file under ~/.gemini/antigravity-cli ($AGY_VERSION)"
  fi

  if [ "${AGY_READ_CACHE:-0}" = "1" ] && [ -f "$AGY_HOME_DIR/cache/last_conversations.json" ]; then
    # Prints only the value type for the scratch repo.
    kind="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); v=d.get(sys.argv[2]); print(type(v).__name__)' "$AGY_HOME_DIR/cache/last_conversations.json" "$REPO")"
    case "$kind" in
      str | dict) pass agy-cache-format "last_conversations.json maps the scratch repo to a $kind" ;;
      *) fail agy-cache-format "no entry for the scratch repo ($kind)" ;;
    esac
  else
    skip agy-cache-format "set AGY_READ_CACHE=1 to read last_conversations.json"
  fi
  skip agy-fleet-spawn "needs a fleet and a trusted directory; see docs/live-checks/harnesses.md"
fi

# ------------------------------------------------------------------- OpenCode
if ! command -v opencode >/dev/null 2>&1; then
  skip opencode "opencode is not installed"
else
  pass opencode-version "$(opencode --version 2>&1 | head -1)"
  REPO="$SCR/opencode"
  scratch_repo "$REPO"
  export OPENCODE_DISABLE_EXTERNAL_SKILLS=1
  # user_turns <session>: the number of user messages in the session.
  user_turns() {
    (cd "$REPO" && opencode export "$1" 2>/dev/null) | python3 -c 'import json,sys; t=sys.stdin.read(); d=json.loads(t[t.index("{"):]); print(sum(1 for m in d.get("messages",[]) if m.get("info",{}).get("role")=="user"))'
  }
  # sessions: the ids of the sessions in the scratch repo.
  sessions() {
    (cd "$REPO" && opencode session list --format json 2>/dev/null) | python3 -c 'import json,sys; t=sys.stdin.read().strip() or "[]"; [print(r["id"]) for r in json.loads(t) if r.get("directory")==sys.argv[1]]' "$REPO"
  }
  # tui <args...>: the TUI in a pseudo-terminal for 45 s, then killed.
  tui() { (cd "$REPO" && clean 45 script -q /dev/null opencode --pure -m "$OC_MODEL" "$@" </dev/null >/dev/null 2>&1) || true; }

  (cd "$REPO" && clean 120 opencode run --pure -m "$OC_MODEL" "Reply with the single word PONG." </dev/null >"$SCR/opencode-run.log" 2>&1) || true
  SID="$(sessions | head -1)"
  if [ -z "$SID" ]; then
    skip opencode-resume-prompt "opencode run made no session (no network or no free tier)"
  else
    before="$(user_turns "$SID")"
    tui --session "$SID" --prompt "Reply with the single word BANANA."
    sleep 2
    after="$(user_turns "$SID")"
    # Control: the same TUI setup without --session delivers --prompt.
    tui --prompt "Reply with the single word CHERRY."
    sleep 2
    control=""
    for s in $(sessions); do
      [ "$s" = "$SID" ] && continue
      if (cd "$REPO" && opencode export "$s" 2>/dev/null) | grep -q CHERRY; then control=yes; fi
    done
    if [ -z "$control" ]; then
      skip opencode-resume-prompt "control failed: a fresh TUI --prompt did not arrive"
    elif [ "$after" = "$before" ]; then
      pass opencode-resume-prompt "--prompt beside --session is ignored ($before user turns before and after); a fresh --prompt arrives"
    else
      fail opencode-resume-prompt "--prompt beside --session now arrives; type nothing on a resume (resume_prompt_typed)"
    fi
  fi
fi

# ---------------------------------------------------------------------- Prime
if ! command -v prime-agent >/dev/null 2>&1; then
  skip prime-status "prime-agent is not installed"
else
  PV="$(prime-agent --version 2>&1 | head -1)"
  # The status record is what runPs prints: discovered daemons plus uptime.
  PS_JS="$(dirname "$(readlink -f "$(command -v prime-agent)")")/../cli/daemon-ps.js"
  if [ -f "$PS_JS" ] && grep -q 'uptimeSeconds: uptimes.get(daemon.pid)' "$PS_JS" &&
    ! grep -qE 'startedAt|startTime' "$PS_JS"; then
    pass prime-status "prime-agent $PV: status --json has pid, socketPath, uptimeSeconds (ps etimes of that pid), no start time"
  elif [ -f "$PS_JS" ]; then
    fail prime-status "prime-agent $PV changed its status record; re-check may_stop (U-63)"
  else
    skip prime-status "cannot find daemon-ps.js for prime-agent $PV"
  fi
fi

# ------------------------------------------------------------------------- pi
if ! command -v pi >/dev/null 2>&1; then
  skip pi-version "pi is not installed"
elif out="$(within 30 pi --version 2>&1)"; then
  pass pi-version "pi $out on node $(node --version 2>&1)"
else
  fail pi-version "pi --version failed: $(head -1 <<<"$out"); add the HORCH_PI_BIN wrapper (U-30 Q1)"
fi

rm -rf "$SCR/agyhome"
exit $((FAILS > 0))
