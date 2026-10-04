# Shared context: context-volume watch and early compaction

Every brief in `ai_docs/plans/context-compaction/` reads this file first.

## The operator's request (verbatim intent)

Make an update that allows the fleet to compact earlier in a conversation.
- Use, and build into horch, the "whats-next" command. In this repository that
  command is the `handoff` skill: `skills/handoff/SKILL.md` ("`handoff` adapts
  `whats-next`", see `skills/README.md`). It writes `ai_docs/handoffs/whats-next.md`
  or an assigned path.
- The orchestrator watches the context volume of every worker AND of itself.
- When a session crosses 300,000 tokens of context, the orchestrator looks for a
  natural stopping point in that session's current stream of work, then compacts it.
- Compaction differs per harness. The orchestrator must know how to compact on
  each harness: Claude Code, Codex, OpenCode, pi, Prime Agent.

## Phases

1. Research (3 parallel researchers, briefs 01, 02, 03). Evidence only.
2. Design and implementation plan (staff-engineer, brief written later).
3. Implementation (workers, briefs written later).

## Facts already established (do not re-research)

- `crates/horch-core/src/usage.rs` already finds each worker's transcript by the
  session id in the ledger and reads CUMULATIVE token totals:
  Claude `~/.claude/projects/*/<sid>.jsonl`; Codex `rollout-*-<sid>.jsonl`
  (`token_count` events); pi `~/.pi/agent/sessions/**/*<sid>*.jsonl`; Prime uses
  the session path the ledger stores; OpenCode is SQLite and is not read.
  Cumulative totals are NOT the current context size. This work needs current
  context occupancy.
- `ai_docs/reports/telemetry-sources.md` lists live token signals per harness
  (transcripts, OTel, statusLine, hooks, Codex SQLite, OpenCode SQLite).
- `ai_docs/reports/telemetry-herdr-surface.md` lists herdr capabilities and
  horch state (ledger, panes, what herdr can see).
- `ai_docs/reports/env-research/compaction-benchmarks.md` gives per-model
  long-context accuracy and each harness's DEFAULT auto-compact trigger
  (Claude Code 967,000; Codex 244,800 of 272,000; Prime 983,616).
- `ai_docs/reports/env-research/claude-code.md` finding 4: Claude Code auto-compact
  levers are `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, settings key `autoCompactWindow`,
  and `--autocompact` (100k-1M). Pair a change with a `# Compact instructions`
  block.
- The orchestrator is not a ledger record today (`Session::Unmanaged`), so its
  own session id is not in the ledger. `ai_docs/designs/telemetry-and-balancing.md`
  (DRAFT) proposes making it one. Do not implement that design; only note overlap.
- Workers talk to the orchestrator only by terminal text injection
  (`horch tell`, `horch assign`). Source: `crates/horch/src/cmd/messaging.rs`,
  `crates/horch-core/src/mailbox.rs`, `crates/horch-core/src/paneshell.rs`.
- Agent prose lives in `teammates/*.md` and `teammates/_base/`. Rust only
  substitutes placeholders. Golden prompt tests: `crates/horch-core/tests/golden_prompts.rs`.

## Findings from round 1 (stopped early on 2026-09-28; carry forward, do not redo)

- Installed versions: Claude Code 2.1.284, Codex 0.157.1, OpenCode 1.18.2,
  pi 0.85.1, Prime Agent 0.9.4.
- Claude Code current context = last non-sidechain assistant message
  `usage.input_tokens + cache_read_input_tokens + cache_creation_input_tokens`,
  deduplicated by `message.id`. Verified: session 7232efa9 read 43,395.
- Claude Code compaction marker, verified in
  `~/.claude/projects/-Users-mascott-projects-multi-herdr/56c891bc-8693-44fc-896d-ce87e0b6f6fa.jsonl`:
  line 388 is a `system` line with `subtype: compact_boundary`,
  `compactMetadata.trigger: auto`, `preTokens 993784`, `postTokens 10486`.
  Line 389 is the user summary line with `isCompactSummary: true`. The last
  assistant message before it reads 991,860; the first after it reads 33,352.
- Claude Code manual command: `/compact <optional custom summarization
  instructions>`. `DISABLE_COMPACT` disables it (binary strings,
  `/tmp/horch-compact-research/claude-2.1.284.txt`).
- pi manual command: `/compact [instructions]` (pi `docs/compaction.md` line 39).
- `horch tell` sends the raw message text, then Enter 2 times
  (`crates/horch-core/src/herdr.rs` `send_line`).
- `horch fleet` launches the orchestrator at `crates/horch/src/cmd/recipes.rs:197-224`
  with `Session::Unmanaged` at `recipes.rs:424`.
- Baseline: `cargo test --workspace` passes 340 of 340.
- GOTCHA: the shared `target/` build fails because a stale build-script output
  points at `/private/tmp/horch-verify/teammates/_template.md`. Build and test
  with `CARGO_TARGET_DIR=/tmp/<your-role>-target cargo test --workspace`.

## Hard rules for every worker

- NEVER read, print, set, export or depend on `ANTHROPIC_API_KEY`. Any `claude`
  command you run is `env -u ANTHROPIC_API_KEY claude ...`.
- Do not start a subagent or background agent.
- The working tree is shared with other workers. Do not run `git checkout`,
  `git switch`, `git stash` or `git reset` in it. Research briefs edit no source.
- Call the installed `horch` on PATH for read-only commands (`horch --help`,
  `horch sessions`). Do not spawn fleet workers.
- Write every report in plain, exact English. Cite a file and line, a URL, or
  a command and its output for each fact. Mark a fact you could not verify as
  UNVERIFIED.
