# Live check: the fleet context policy on this Mac

The live acceptance steps of the context policy
([`specs/context-policy.md`](../specs/context-policy.md)) run against the
installed Claude Code and Codex, the operator's real settings and, in part B,
a real fleet: the native windows the panes really use (CTX-05, CTX-06), the
readers against the harness's own numbers (CTX-02.4, CTX-17), the
compaction-instructions canary (CTX-20), the speed of `horch context`
(CTX-26), the survival of a detached job (CTX-15), and the round trips of
the protocol (CTX-11, CTX-13, CTX-16, CTX-18, CTX-22, CTX-23).

## How to run

```bash
scripts/live/context.sh                                        # part A (before the install)
LIVE_CONTEXT_PANES=1 scripts/live/context.sh                   # part A, then part B
LIVE_CONTEXT_PART=b LIVE_CONTEXT_B1_ROLE=sonnet-1 \
  LIVE_CONTEXT_B2_ROLE=codex-sol-1 scripts/live/context.sh     # part B checks after the pane steps
HORCH_BIN=~/.local/bin/horch scripts/live/context.sh           # another horch binary
```

Part A needs `jq`, `perl`, `python3`, `claude` and `codex` signed in to the
operator's subscriptions, and a built `target/debug/horch` (`HORCH_BIN`).
Part B needs a herdr server, the installed `horch` and a fleet. A missing tool
is `SKIP`. Every `claude` call is `env -u ANTHROPIC_API_KEY claude ...`, and
the script unsets the key at the top (CTX-27).

Scratch files go under `.worktrees/_scratch/live-context/`. Each run writes
its log to `.worktrees/_scratch/live-context/results/<UTC time>.md` and
appends its dated result table to the end of this file, in the
[result-file format](README.md#result-file-format); old tables stay. This
file is the only one outside the scratch dir that the script writes. Part A
takes about 4 minutes.

Paid steps, each 1 short session: A3 (1 haiku turn), A5 (1 haiku turn and
1 Codex turn), A6 (1 haiku turn and its `/compact`, 2 Codex turns), A8 (1
haiku turn and 1 Codex turn). A2 and A7 spend no model tokens
(`/autocompact` is a local command).

## Part A steps

- **A1 versions.** `claude --version` and `codex --version`.
- **A2 windows (CTX-05, CTX-06).** The tier loop: for each distinct
  (model, setting, source) of the Claude rows of `horch context --windows`,
  `claude -p "/autocompact" --model <model>` (source `operator`) or the same
  with `--settings '{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"<n>"}}'`
  (source `fleet`). PASS when the reported window equals SETTING. Then, in a
  scratch `CLAUDE_CONFIG_DIR` with no window and no process window, horch
  must decide the fleet values (`sonnet` 150,000, `opus` 200,000,
  `orchestrator` 300,000), and `/autocompact` with that launch value must
  report `150k`, `200k`, `300k` (the amended CTX-05.1).
- **A3 env merge.** 1 haiku turn with a `--settings` env object that holds
  only the window runs `echo $CLAUDE_CODE_DISABLE_ADVISOR_TOOL` with its
  Bash tool. PASS when it prints the operator's value: the overlay keeps the
  operator's other env keys.
- **A4 Codex limit.** horch has no `spawn --dry-run`, so the argv is the one
  the u4 test `ctx_05_codex_fleet_limit_flag_only_without_operator_value`
  prints, and the decision is the `codex-sol` row of `--windows` on this
  Mac's real `config.toml`. A5's rollout tells whether Codex reports the
  limit anywhere; else the report is `argv-only`.
- **A5 readers (CTX-02.4, CTX-17.1).** 1 haiku session and 1 `codex exec`
  session in the scratch dir, 2 ledger records that point at them, then
  `horch context`. Claude: compared with the `/context` total; Codex: with
  the `tokens used` line of `codex exec` plus the cached input of its newest
  `token_count` (Codex's own line leaves the cached input out). PASS within
  1 response's output plus 1 tool result (2,000 tokens).
- **A6 canary (CTX-20).** The worker's `== Compact instructions ==` block
  with 1 more line, `KEEP-CANARY-<random>`, is the first prompt; then a
  compaction with no argument (`claude -p --resume <id> "/compact"`;
  `codex exec resume <id> "/compact"`). The summary (Claude: the
  `isCompactSummary` line; Codex: the `compacted` line) is searched for the
  canary: `honoured`, `ignored` or `not verifiable`, with the evidence.
  `ignored` is a result, not a FAIL.
- **A7 speed (CTX-26).** `horch context` on the operator's real ledger:
  under 1 s.
- **A8 detached survival (CTX-15).** A `start_new_session` child appends to
  `beat.log` every 2 s. It is started (1) by a haiku turn with its Bash tool
  and (2) by a `codex exec` call. 10 s after each call ends, `beat.log` must
  still grow. A FAIL means the compaction job cannot rely on
  `spawn_detached`: report it at once.

## Part B steps (after the install)

The script prints the exact procedure of each step and then, with the role in
`LIVE_CONTEXT_B1_ROLE`, `LIVE_CONTEXT_B2_ROLE` or the project in
`LIVE_CONTEXT_B3_PROJECT`, checks the ledger events.

- **B1 Claude worker round trip** (CTX-11.1, CTX-13.2, CTX-22.1, CTX-23.1).
- **B2 Codex worker round trip and busy rejection** (CTX-13.1, CTX-13.3).
  Also `horch note "x"` in the Codex worker pane: the note is recorded, and
  over the threshold it prints the warning (review finding 2). Until B2
  passes, the orchestrator checkpoint is the only watch for Codex workers.
- **B3 Claude orchestrator self-compaction** (CTX-18.1); **B4** the Codex
  orchestrator (CTX-18.2).
- **B5** `time horch note x` in a worker pane: under 0.5 s (CTX-26).
- **B6** a `pi` worker over its threshold takes the fresh route (CTX-16.1).

If B1 or B2 FAILs at the compaction step, take that harness out of
`in_place:` in `teammates/_base/context-windows.md` (1 line), and commit it
with this file.

## Results

Each run appends 1 dated table below, newest last. Part B runs after the
final gate and `just install`.

## 2026-10-07, part A

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| A1 versions | the versions the steps ran on | claude 2.1.292 (Claude Code) | PASS | claude 2.1.292 (Claude Code), codex codex-cli 0.160.0 |
| A2 claude - 500,000 operator (1 teammates, e.g. orchestration-worker) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 500000, reported 500000: Auto-compact window for Opus 5.5: 500k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 claude fable 500,000 operator (2 teammates, e.g. orchestration-orchestrator) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 500000, reported 500000: Auto-compact window for Fable 5.1: 500k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 claude opus 500,000 operator (48 teammates, e.g. apple-platform-developer) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 500000, reported 500000: Auto-compact window for Opus 5.5: 500k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 claude sonnet 500,000 operator (13 teammates, e.g. app-release-preparer) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 500000, reported 500000: Auto-compact window for Sonnet 5.5: 500k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 scratch claude/sonnet (sonnet, sonnet) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 150000, reported 150000: Auto-compact window for Sonnet 5.5: 150k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 scratch claude/opus (opus, opus) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 200000, reported 200000: Auto-compact window for Opus 5.5: 200k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A2 scratch claude/orchestrator (orchestrator, fable) | CTX-05, CTX-06: each Claude tier runs the window `horch context --windows` decides (`harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | intended 300000, reported 300000: Auto-compact window for Fable 5.1: 300k tokens (from CLAUDE_CODE_AUTO_COMPACT_WINDOW) |
| A3 env merge | the fleet `--settings` env object keeps the operator env keys (spec 3.3, `harness/claude.rs`) | claude 2.1.292 (Claude Code) | PASS | the overlay keeps the operator's env key: ADV=1 (paid: 1 short session) |
| A4 codex argv | CTX-05.3: the codex-sol launch carries `model_auto_compact_token_limit` (`harness/codex.rs`) | codex-cli 0.160.0 | PASS | codex-sol decides 200,000 fleet here; the u4 launch argv carries -c model_auto_compact_token_limit=200000 |
| A5 claude reader | CTX-02.4, CTX-17.1: the readers give the harness own number (`telemetry/context.rs`) | claude 2.1.292 (Claude Code) | PASS | horch 30,388, /context 30.2k (30200); difference 188 <= 2256 (paid: 1 short session) |
| A5 codex reader | CTX-02.4, CTX-17.1: the readers give the harness own number (`telemetry/context.rs`) | codex-cli 0.160.0 | PASS | horch 17,548, codex 'tokens used' 5260 + cached input 12288 = 17548; difference 0 <= 2000 (paid: 1 short session) |
| A4 codex reports the limit | CTX-05.3: the codex-sol launch carries `model_auto_compact_token_limit` (`harness/codex.rs`) | codex-cli 0.160.0 | PASS | argv-only: no auto_compact key in the rollout rollout-2026-10-06T21-54-11-01a114b6-4335-7743-bfef-46c271674c96.jsonl; /status is in a pane only (part B2) |
| A6 claude canary | CTX-20: the compact-instructions block survives a compaction (`teammates/_base/fleet-worker.md`) | claude 2.1.292 (Claude Code) | PASS | honoured: the isCompactSummary line has: ck-1-whats-next.md\n- Canary line (preserve word for word): KEEP-CANARY-1d481a27\n\nIf you need spec (paid: 1 short session) |
| A6 codex canary | CTX-20: the compact-instructions block survives a compaction (`teammates/_base/fleet-worker.md`) | codex-cli 0.160.0 | SKIP | not verifiable: codex exec resume with /compact wrote no compacted line; part B2 compacts a codex pane |
| A7 speed | CTX-26: `horch context` under 1 s (`cmd/context.rs`) | horch 0.1.0 | PASS | horch context on the real ledger: 121 ms, 3 lines |
| A8 survival after a claude Bash tool call | CTX-15: a detached job outlives the call that started it (`runtime/process.rs` `spawn_detached`) | claude 2.1.292 (Claude Code) | PASS | beat.log grows after the call ended (6 -> 9 lines in 6 s) (paid: 1 short session) |
| A8 survival after a codex exec call | CTX-15: a detached job outlives the call that started it (`runtime/process.rs` `spawn_detached`) | codex-cli 0.160.0 | PASS | beat.log grows after the call ended (7 -> 10 lines in 6 s) (paid: 1 short session) |
