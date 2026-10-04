# G5 fleet-messaging: inbox, done without an orchestrator, resume after a closed pane

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

1. `horch inbox` lists only roles whose pane is still open (or marks closed
   ones clearly; choose and give the reason).
2. A worker in a workspace with no registered orchestrator can still run
   `horch done`: it records done, tells nobody, prints why, and exits 0.
   Codex said "Blocked from notifying the orchestrator" and did not run
   `done`: find out whether horch exited non-zero, printed something that
   reads like a block, or the worker prompt makes `done` depend on the tell.
   Fix the cause.
3. `horch spawn --resume <id>` of a record marked working whose pane is gone
   (herdr no longer has that pane) resumes it, or refuses with the exact
   command that fixes it. Today it refuses "still marked working" and the
   operator must guess `horch ledger done <id> ...`. Prefer: if herdr shows
   the pane gone, mark the record ended with history text "pane closed
   without horch done" and resume. Never resume a record whose pane is open.

## CONTEXT

`ai_docs/reports/finish/acceptance-fleet.md`, "Other findings". The refusal
is `crates/horch-core/src/execution/plan.rs:204`. herdr 0.8.2 commands:
trust `horch --help` and `teammates/_base`, not `~/.claude/skills/herdr-*`
(stale). Worker prompt prose lives in `teammates/*.md` (prompts are data,
not code); if (2) needs a prompt change, change `teammates/_base` text.

## FILES

own: `crates/horch-core/src/messaging/` except `brief.rs` (G3),
`crates/horch/src/cmd/messaging.rs`, `crates/horch-core/src/execution/plan.rs`
(the resume check), `crates/horch-core/src/execution/store.rs` if needed,
`teammates/_base*` (done/tell wording only), `crates/horch-core/tests/messaging.rs`,
the e2e tests for these, `ai_docs/reports/finish/fleet-messaging.md`.

do not touch: `messaging/brief.rs`, `execution/lifecycle.rs`,
`crates/horch/src/cmd/spawn.rs` (G3; if you need a change there, tell the
orchestrator), `skills/`, `harness/`, `competition/`.

## STEPS

1. inbox + test. 2. done without orchestrator: reproduce with a fake
   workspace, fix, test. 3. resume after closed pane + tests (pane gone →
   resumes; pane open → refuses). 4. Targeted checks (messaging tests, the
   plan/lifecycle lib tests, `cargo build --workspace --bins` then the e2e
   tests you touched, clippy -D warnings, rustfmt on your files). COMMITTED.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
