#!/usr/bin/env bash
# Local acceptance on the operator's Mac (telemetry design, section 17): the
# checks only real harnesses, real ledgers and a real herdr can prove.
# Guides each step and appends the results to
# ai_docs/reports/telemetry-acceptance-<date>.md.
#
# NEVER sets or passes ANTHROPIC_API_KEY (CLAUDE.md). Run from the repo root.
set -uo pipefail
cd "$(dirname "$0")/.."
unset ANTHROPIC_API_KEY
report="ai_docs/reports/telemetry-acceptance-$(date -u +%Y-%m-%d).md"
horch=${HORCH:-horch}
{
  echo "# Telemetry acceptance, $(date -u +%Y-%m-%dT%H:%MZ)"
  echo
  echo "| step | result | evidence |"
  echo "|---|---|---|"
} > "$report"
row() { echo "| $1 | $2 | $3 |" >> "$report"; echo "$1: $2 - $3"; }
ask() { local a; read -r -p "$1 [y/n] " a; [ "$a" = y ]; }

# L1 fixture drift: key paths a reader uses must exist in real files.
claude_file=$(ls -t ~/.claude/projects/*/*.jsonl 2>/dev/null | head -1)
if [ -n "$claude_file" ] && grep -q '"cache_creation"' "$claude_file" && grep -q '"message"' "$claude_file"; then
  row L1 pass "claude usage keys present in $(basename "$claude_file")"
else
  row L1 check "no recent claude transcript, or usage keys missing"
fi
codex_file=$(find "${CODEX_HOME:-$HOME/.codex}/sessions" -name 'rollout-*.jsonl' 2>/dev/null | xargs ls -t 2>/dev/null | head -1)
if [ -n "$codex_file" ]; then
  if grep -q '"token_usage_record"' "$codex_file"; then row L1 pass "codex token_usage_record in $(basename "$codex_file")";
  else row L1 check "codex rollout without token_usage_record (older CLI?)"; fi
fi

# L2 cost parity.
since=$(date -u -v-7d +%Y-%m-%d 2>/dev/null || date -u -d '7 days ago' +%Y-%m-%d)
$horch usage --json --since "$since" > /tmp/horch-usage.json && $horch cost --json --since "$since" > /tmp/horch-cost.json \
  && row L2 run "compare /tmp/horch-usage.json records[] with /tmp/horch-cost.json rows[] per class" \
  || row L2 fail "a command failed"

# L3 probe truth.
$horch quota --refresh
ask "Do the windows match /usage in Claude and /status in Codex within 1 point, same resets?" \
  && row L3 pass "operator confirmed" || row L3 fail "operator saw a difference"

# L4 probe hygiene.
marker=$(mktemp); sleep 1
for _ in 1 2 3; do $horch quota --refresh >/dev/null; sleep 1; done
new=$(find ~/.claude/projects "${CODEX_HOME:-$HOME/.codex}/sessions" -newer "$marker" -name '*.jsonl' 2>/dev/null)
if [ -z "$new" ] || ! grep -l '"type":"user"' $new >/dev/null 2>&1; then row L4 pass "no user message in new transcripts"; else row L4 fail "new transcript with a user message: $new"; fi

# L5-L7 need herdr and a scratch fleet; the operator drives them.
ask "L5: after 'horch fleet' in 2 scratch projects, is there exactly 1 'horch telemetry' workspace and did focus stay put?" \
  && row L5 pass "operator confirmed" || row L5 fail "see herdr workspace list"
ask "L6: did a new sonnet worker's row appear within 5 s of its first response?" \
  && row L6 pass "operator confirmed" || row L6 fail "latency"
ask "L7: after kill -9 of the collector and 'horch telemetry ensure', were totals unchanged?" \
  && row L7 pass "operator confirmed" || row L7 fail "totals changed"

# L8 gate drill, L9 real gate.
fixture=crates/horch-core/tests/fixtures/telemetry/quota/claude-exhausted-codex-ok.json
HORCH_QUOTA_FILE=$fixture $horch route researcher | head -1 | grep -q '^SUBSTITUTED' \
  && row L8 pass "route researcher substitutes codex-sol on the drill fixture" || row L8 fail "no SUBSTITUTED line"
$horch route opus; code=$?
row L9 "exit $code" "route opus on the real pools (REFUSED expected while both are exhausted)"

# L10 perf.
if command -v just >/dev/null; then just verify-perf && row L10 pass "nfr_02 within budget" || row L10 fail "see output"; fi
echo "report: $report"
