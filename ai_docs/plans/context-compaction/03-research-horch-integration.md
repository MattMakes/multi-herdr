# Brief 03: where context watching and compaction plug into horch

## GOAL
Write `ai_docs/reports/context-compaction/03-horch-integration.md`. It maps
every horch and herdr surface that a "watch context, find a stopping point,
write whats-next, compact" feature must touch, with file:line references.

## CONTEXT
Read `ai_docs/plans/context-compaction/00-shared-context.md` first.
Brief 01 researches how to measure context per harness. Brief 02 researches how
to trigger compaction per harness. You research horch itself: the Rust code,
the teammate prose, the skills, and the herdr surface. Do not research the
harness internals.

## FILES
- own: `ai_docs/reports/context-compaction/03-horch-integration.md` (create it).
- do not touch: every other file. No source edits.

## PRIOR WORK
- `ai_docs/reports/telemetry-herdr-surface.md` already maps herdr capabilities and
  horch state for a telemetry pane. Reuse it; cite it; verify only what matters here.
- `ai_docs/designs/telemetry-and-balancing.md` is a DRAFT design that overlaps
  (orchestrator as a ledger record, a collector process). Note overlaps.
- Memory: prompts are data. Agent prose lives in `teammates/*.md` and
  `teammates/_base/`; Rust only substitutes placeholders.

## STEPS
Answer each question with file:line references.
1. Orchestrator identity: how does `horch fleet` launch the orchestrator, and
   how could horch learn the orchestrator's own session id and transcript path
   (Claude `--session-id`, Codex discovery reused from workers)? What env vars
   does the orchestrator pane get?
2. Worker identity: which ledger fields and env vars (`HORCH_ROLE`,
   `HORCH_RECORD_ID`, `HORCH_SESSION_ID`, ...) exist, and where the session id is
   filled in for each harness (Codex post-launch discovery included).
3. Idle detection: how can horch know a pane is idle (between turns) versus
   working? Check herdr agent status in `herdr-docs/session-state.md`,
   `herdr-docs/socket-api.md`, `herdr-docs/cli-reference.md` and
   `crates/horch-core/src/herdr.rs`. Say which harnesses herdr classifies.
4. Injection: exactly what `horch tell` and `horch assign` type into a pane
   (prefix, Enter key, bracketed paste), from `messaging.rs`, `mailbox.rs`,
   `paneshell.rs`. Can horch send a raw line such as `/compact ...` without the
   `[role]` prefix? What code change would add that?
5. Command surface: list existing `horch` subcommands that are the natural home
   for (a) a context report (for example extend `horch cost`, `horch sessions`,
   or add `horch context`) and (b) a compact action (for example
   `horch compact <role>`). Name the clap definitions in `crates/horch/src/main.rs`
   and `crates/horch/src/cmd/mod.rs`.
6. Prose surface: which teammate files and `_base` fragments hold the
   orchestrator's loop and the worker lifecycle text, where a "check context,
   compact at a stopping point" rule belongs, and which golden files in
   `crates/horch-core/tests/golden/` change when that prose changes.
7. Skill surface: how `skills/handoff/SKILL.md` reaches workers today (phase
   catalogs in `crates/horch-core/src/skills.rs`, teammate frontmatter), which
   phases include it, and what change makes it available in every phase and to
   the orchestrator. Check `docs/phase-skills.md`.
8. Launch-time backstop: where horch builds per-harness launch args and env
   (`crates/horch-core/src/launch.rs`, `agent.rs`, `codex.rs`, `opencode.rs`,
   `prime.rs`, teammate `env:` maps), so an auto-compact threshold can be set
   per teammate.
9. Tests: list the test files and commands that must stay green
   (`cargo test --workspace`, golden prompts, `horch teammates --check`,
   `horch smoke ...`). Run `CARGO_TARGET_DIR=/tmp/researcher-target cargo test --workspace` once and record the result
   as the baseline.

## CONSTRAINTS
- Read-only except `cargo test`. Do not run `horch fleet`, `horch spawn`,
  `horch tile` or `horch smoke` commands that create panes.

## DONE WHEN
- The report answers steps 1-9 with file:line references.
- The report ends with a "Change map" table: surface | file | what changes | risk.
- The report states the baseline `cargo test --workspace` result.

## REPORT
- `horch note` after steps 3, 6 and 9.
- `horch tell orchestrator "QUESTION: ..."` if blocked. Continue meanwhile.
- `horch done` summary: report path, the change map rows in one line each, and
  the baseline test result.
