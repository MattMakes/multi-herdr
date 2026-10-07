# Live check: the fleet context policy on this Mac

The live acceptance steps of the context policy
([`specs/context-policy.md`](../specs/context-policy.md)) run against the
installed Claude Code and Codex, the operator's real settings and, in part B,
a real fleet: the native windows the panes really use (CTX-05, CTX-06), the
readers against the harness's own numbers (CTX-02.4, CTX-17), the
compaction-instructions canary (CTX-20), the speed of `horch context`
(CTX-26), the survival of a detached job (CTX-15), and the round trips of
the protocol (CTX-11, CTX-13, CTX-16, CTX-18, CTX-22, CTX-23). Part C does
the same for the slice-2 harnesses OpenCode, pi and Prime (CTX-02, CTX-17,
CTX-20).

## How to run

```bash
scripts/live/context.sh                                        # part A (before the install)
LIVE_CONTEXT_PANES=1 scripts/live/context.sh                   # part A, then part B
LIVE_CONTEXT_PART=b LIVE_CONTEXT_B1_ROLE=sonnet-1 \
  LIVE_CONTEXT_B2_ROLE=codex-sol-1 scripts/live/context.sh     # part B checks after the pane steps
HORCH_BIN=~/.local/bin/horch scripts/live/context.sh           # another horch binary
LIVE_CONTEXT_PART=c LIVE_CONTEXT_PRIME_MODEL=ollama/qwen3.8 \
  scripts/live/context.sh                                      # part C (headless steps, then the pane steps)
LIVE_CONTEXT_PART=c5 LIVE_CONTEXT_C5_PI_ROLE=pi-1 \
  scripts/live/context.sh                                      # part C pane steps only
```

Part B reads the ledger of the current project: the git top level of the
directory the script runs from (the operator's fleet). Part C uses the
scratch fleet `.worktrees/_scratch/live-context/fleet`. `LIVE_CONTEXT_PROJECT`
overrides both. The script prints the project of each part. A step whose role
has no record in that project prints its `FAIL` line, and the run appends no
table: a wrong project proves nothing.

Part A needs `jq`, `perl`, `python3`, `claude` and `codex` signed in to the
operator's subscriptions, and a built `target/debug/horch` (`HORCH_BIN`).
Part B needs a herdr server, the installed `horch` and a fleet. Part C needs
`pi`, `prime-agent`, `opencode` and `sqlite3`, and for its pane steps a herdr
server, the installed `horch` and the scratch fleet. A missing tool is `SKIP`. Every `claude` call is `env -u ANTHROPIC_API_KEY claude ...`, and
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

A round trip starts with a `compact-requested` event (`horch compact <role>
--request`), or with a `context-warned` event: a worker that gets the
`horch note` warning writes its handoff without a request. The PASS text
names the start event. Every lookup by role takes the role's record with the
newest `updated_at`, because an old record of the same role stays in the
ledger.

## Part C steps (slice 2: OpenCode, pi, Prime)

C1 to C3 run headless in `.worktrees/_scratch/live-context/c`. The models
come from `teammates/pi.md`, `teammates/prime.md` and
`teammates/opencode-pickle.md`; `LIVE_CONTEXT_PI_MODEL`,
`LIVE_CONTEXT_PRIME_MODEL` and `LIVE_CONTEXT_OPENCODE_MODEL` override them.
Prime lists only the models it can serve (`prime-agent model list`). Prime
on `anthropic/claude-opus-5-5` needs an Anthropic login in Prime: that is an
operator step, and the check never signs Prime in and never uses an API key.
Without that login, set `LIVE_CONTEXT_PRIME_MODEL` to a listed model, such as
`ollama/qwen3.8`. pi and Prime get a scratch agent dir: links to the
operator's entries, and a `settings.json` with
`compaction.keepRecentTokens: 1`, so that 2 short turns can be compacted.
The operator's dirs are not written.

- **C1 versions.** `pi`, `prime-agent`, `opencode`, `herdr`, `sqlite3`, and
  the 3 models.
- **C2 readers (CTX-02, CTX-17).** 2 pi turns, 2 Prime turns, 1
  `opencode run --format json` turn, 1 ledger record for each, then
  `horch context`. pi and Prime: compared with the session's newest
  assistant `totalTokens`; OpenCode: with the newest `step_finish` total of
  `opencode run`. PASS when they are equal.
- **C3 canary (CTX-20, CTX-03).** The first pi and Prime turn is the
  worker's `== Compact instructions ==` block with a `KEEP-CANARY-<random>`
  line. Then 1 compaction with no argument through the RPC mode
  (`{"type":"compact"}`; `-p` reads `/compact` as a prompt). The `compaction`
  line's summary is searched for the canary: `honoured`, `ignored` or
  `not verifiable`. After it, `horch context` must show the row `pending`.
  OpenCode has no headless compact command (`opencode run --command compact`
  answers `Command not found`), so its compaction is a pane step only.
- **C4 Prime agent dir.** The pane's own `PRIME_AGENT_CODING_AGENT_DIR`
  (`<state>/prime/<role>-<uuid>/agent`) has `autoRefine` `{"enabled":false}`,
  and `prime-agent model list` with that dir reports the window that horch
  decided for the pane: 200,000 fleet for `anthropic/claude-opus-5-5`, or
  the operator's value.
- **C5 panes.** For `opencode-pickle`, `pi` and `prime` in the scratch fleet:
  `horch note "x"` is recorded (review finding 2), the warning over the
  threshold, herdr `agent_status` reads `working` and then `idle` or `done`
  during a 20 s task, and the compaction round trip with
  `horch compact <role> --force` (these harnesses are not in `in_place`). With
  `LIVE_CONTEXT_PRIME_MODEL` set, the script writes a roster copy with that
  model in `.worktrees/_scratch/live-context/roster` and prints the
  `HORCH_TEAMMATES_DIR=... horch fleet` line.

When a harness passes its C5 compaction round trip, add it to `in_place:` in
`teammates/_base/context-windows.md` (S2).

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

## 2026-10-07, part B

Recorded by hand from the pane runs and the script's ledger checks; horch at
`4a29c43` or later, installed. The FAIL table of the first part B run came
from a lookup in the wrong project, and is removed. The script now reads the
current project, and sets `HORCH_PROJECT_DIR` to it (a fleet pane exports
its own).

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| B1 claude worker round trip | CTX-11.1, CTX-13.2, CTX-22.1, CTX-23.1: a Claude worker round trip | horch 0.1.0 | PASS | sonnet-11: compact-requested, handoff note, compacted 44,856 -> 19,897 tokens; 1 `[horch] NOTE: sonnet-11 compacted.` line (paid: 1 worker session) |
| B2 codex horch note records | CTX-10, CTX-13.1, CTX-13.3: a Codex worker round trip, `horch note` in a Codex pane | codex-cli 0.160.0, horch 0.1.0 at `0d9f992` (`--add-dir <state root>`) | PASS | codex-sol-8: a compound `horch note` in the Codex pane exits 0 and records the note `x` |
| B2 codex horch note warns | CTX-10, CTX-13.1, CTX-13.3: a Codex worker round trip, `horch note` in a Codex pane | codex-cli 0.160.0 | PASS | codex-sol-9 on a scratch roster copy, `codex/gpt-5.6-sol` 100,000 (threshold 80,000): `horch note` printed the warning at 85,668 tokens and recorded context-warned. Codex writes its token count after each model turn, so the warning comes at the first `horch note` after the turn that crosses the threshold |
| B2 codex worker round trip | CTX-10, CTX-13.1, CTX-13.3: a Codex worker round trip, `horch note` in a Codex pane | codex-cli 0.160.0 | PASS | request path: codex-sol-8, 23,880 -> 7,717 tokens. Warning path: codex-sol-9, context-warned, handoff note, 89,581 -> 7,264 tokens. Force path: `horch compact --force` waited 32 s for idle, then 1 bare `/compact`, 30,189 -> 6,885 tokens. Result files `.worktrees/_scratch/live-context/results/2026-10-07T05-28-30Z.md`, `2026-10-07T05-31-59Z.md`, `2026-10-07T05-33-07Z.md` (paid: 2 worker sessions) |
| B3 orchestrator self-compaction | CTX-18: orchestrator self-compaction | horch 0.1.0 | PASS | orchestrator: 304,556 -> 29,902 tokens; handoff `ai_docs/handoffs/orchestrator-whats-next.md`; result file `.worktrees/_scratch/live-context/results/2026-10-07T05-13-30Z.md` |
| B4 codex orchestrator self-compaction | CTX-18: orchestrator self-compaction | codex-cli 0.160.0 | SKIP | operator step: the Codex orchestrator (`horch fleet` on the Codex flavor) |
| B5 horch note timing | CTX-26: `horch note` under 0.5 s in a pane | horch 0.1.0 | PASS | `time horch note timing` in a worker pane: real 0.060 s |
| B6 pi fresh route | CTX-16.1: the fresh route for a pi worker | horch 0.1.0 | PASS | pi-1: `horch compact` refused in place; pi-1 ran `horch done` with its handoff; pi-2 started with `PRIOR WORK` and resumed |

## 2026-10-07, part C

The headless steps, on the installed horch at `bd7afdd` (u9 `5db062d`), with
`LIVE_CONTEXT_PRIME_MODEL=ollama/qwen3.8`: Prime has no Anthropic login on
this Mac, and Prime with that login is an operator step. Result file
`.worktrees/_scratch/live-context/results/2026-10-07T05-40-32Z.md`. No step
was paid: pi and Prime ran on the local Ollama model, and OpenCode on its
free tier. The C4 and C5 pane steps follow in the next table.

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| C1 versions | the versions and models part C ran on | horch 0.1.0 | PASS | pi 0.99.1, prime-agent 0.9.4, opencode 1.18.35, herdr 0.8.2, sqlite3 3.51.0; models pi ollama/qwen3.8, prime ollama/qwen3.8, opencode opencode/big-pickle |
| C2 pi reader | CTX-02, CTX-17: the slice-2 readers give the harness own number (`telemetry/context.rs`) | pi 0.99.1 | PASS | horch 657, the session's newest totalTokens 657 |
| C2 prime reader | CTX-02, CTX-17: the slice-2 readers give the harness own number (`telemetry/context.rs`) | prime-agent 0.9.4 | PASS | horch 1539, the session's newest totalTokens 1539 |
| C2 opencode reader | CTX-02, CTX-17: the slice-2 readers give the harness own number (`telemetry/context.rs`) | opencode 1.18.35 | PASS | horch 21379, opencode run's newest step_finish total 21379 |
| C3 pi canary | CTX-20: the compact-instructions block survives a pi or Prime compaction (`teammates/_base/fleet-worker.md`) | pi 0.99.1 | PASS | ignored (a result: the keep-list argument of horch compact covers pi and Prime): the summary has no KEEP-CANARY-1d745e8a: - The assistant should respond with only the word "OK."  |
| C3 prime canary | CTX-20: the compact-instructions block survives a pi or Prime compaction (`teammates/_base/fleet-worker.md`) | prime-agent 0.9.4 | SKIP | not verifiable: no compaction line after the RPC compact ({"success": false, "error": "Timed out after 30000ms waiting for the Prime Agent daemon response to \"compact\". Socket: /Users/mascott/projects/multi-herdr/.worktrees/_scratch/live-context/c/d.sock. Daemon log: /Users/mascott/projects/multi-herdr/.worktrees/_scratch/live-context/c/prime-agent/logs/d.sock.26fa19ff.log."}); part C5 compacts a pane |
| C4 prime agent dir | CTX-05: the Prime pane agent dir turns autoRefine off and sets the fleet window (`harness/prime.rs`) | prime-agent 0.9.4 | SKIP | no pane agent dir; spawn prime (C5), then set LIVE_CONTEXT_C5_PRIME_ROLE or LIVE_CONTEXT_C4_AGENT_DIR |
| C5 opencode-pickle pane | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | opencode 1.18.35 | SKIP | operator step; set LIVE_CONTEXT_C5_OPENCODE_ROLE to check the ledger and the pane |
| C5 pi pane | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | pi 0.99.1 | SKIP | operator step; set LIVE_CONTEXT_C5_PI_ROLE to check the ledger and the pane |
| C5 prime pane | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | prime-agent 0.9.4 | SKIP | operator step; set LIVE_CONTEXT_C5_PRIME_ROLE to check the ledger and the pane |

## 2026-10-07, part C, panes

The pane steps, in the operator's fleet (`LIVE_CONTEXT_PROJECT` = this
repo), on horch `bd7afdd`; prime-1 ran `ollama/qwen3.8` from the roster
copy. Result file
`.worktrees/_scratch/live-context/results/2026-10-07T06-04-58Z.md`. The
orchestrator ran the round trips: `--request`, `COMPACT-READY`, a plain
`horch compact` refused (fresh route), then `horch compact --force`. The
warning step (a small `compact_window`) did not run, so the `warns` rows are
SKIP. The pi and Prime round-trip rows carry the orchestrator's evidence.

| step | claim it proves | tool version | result | evidence |
|---|---|---|---|---|
| C4 prime autoRefine off | CTX-05: the Prime pane agent dir turns autoRefine off and sets the fleet window (`harness/prime.rs`) | prime-agent 0.9.4 | PASS | /Users/mascott/.local/state/horch/prime/prime-1-9b0285c7-728d-4cb7-b203-b1ef00f0ea14/agent/settings.json has autoRefine {"enabled":false} |
| C4 prime window | CTX-05: the Prime pane agent dir turns autoRefine off and sets the fleet window (`harness/prime.rs`) | prime-agent 0.9.4 | PASS | prime-agent model list with PRIME_AGENT_CODING_AGENT_DIR=/Users/mascott/.local/state/horch/prime/prime-1-9b0285c7-728d-4cb7-b203-b1ef00f0ea14/agent: ollama/qwen3.8 context 262.1K; horch decided 262144 operator |
| C5 opencode-pickle horch note records | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | opencode 1.18.35 | PASS | opencode-pickle-2 has 1 note 'x' |
| C5 opencode-pickle horch note warns | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | opencode 1.18.35 | SKIP | no context-warned event on opencode-pickle-2 (step 3 not run) |
| C5 opencode-pickle agent_status | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | opencode 1.18.35 | PASS | herdr read: working done |
| C5 opencode-pickle compaction round trip | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | opencode 1.18.35 | PASS | opencode-pickle-2: compact-requested (tokens 17085 threshold 112000), handoff note, compacted '19090 -> 16045 tokens; handoff ai_docs/handoffs/opencode-pickle-2-whats-next.md' |
| C5 pi horch note records | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | pi 0.99.1 | PASS | pi-3 has 1 note 'x' |
| C5 pi horch note warns | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | pi 0.99.1 | SKIP | no context-warned event on pi-3 (step 3 not run) |
| C5 pi agent_status | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | pi 0.99.1 | PASS | herdr read: working idle |
| C5 pi compaction round trip | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | pi 0.99.1 | SKIP | not verifiable: pi-3 got `/compact` from `horch compact --force` and printed 'Compaction failed: Nothing to compact (session too small)' at about 16,000 tokens (compact-requested at 15,566); no compacted event. pi stays on the fresh route |
| C5 prime horch note records | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | prime-agent 0.9.4 | PASS | prime-1 has 1 note 'x' |
| C5 prime horch note warns | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | prime-agent 0.9.4 | SKIP | no context-warned event on prime-1 (step 3 not run) |
| C5 prime agent_status | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | prime-agent 0.9.4 | SKIP | known gap: herdr does not detect the agent in the pane (agent null), so agent_status stays unknown; the compaction job cannot see the pane go idle, so prime stays on the fresh route |
| C5 prime compaction round trip | CTX-10, CTX-13, CTX-17: a slice-2 worker pane: horch note, herdr agent_status, the compaction round trip | prime-agent 0.9.4 | FAIL | known gap: the job failed at step wait-idle, 'no idle status in 120 s', because herdr reads agent_status unknown for a Prime pane; 1 BLOCKED line, nothing typed; prime-1 then ran `horch done` with its handoff (the fresh route). Prime stays on the fresh route |

Result for S2: `opencode` passes its compaction round trip, so
`teammates/_base/context-windows.md` now has
`in_place: [claude, codex, opencode]`. pi (not verifiable: its session was
too small to compact) and Prime (the agent_status known gap) stay on the
fresh route. Follow-up for the Prime gap: horch reports the Prime pane state
with `herdr pane report-agent`, or herdr learns to detect `prime-agent`.
