# U36 session-discovery-race: a short agent run still records its session id

Unit slug: `session-discovery-race`. Branch: `ard/session-discovery-race`. Phase: A4/A6 follow-up (TEL, EXP-07 support).

## GOAL

A harness that mints its own session id (Codex, OpenCode, Prime and any
other `discovers_session()` harness) gets its session id recorded even when
the agent runs `horch done` within 3 seconds of launch, so telemetry and
`horch cost` can price that run.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- The race (found by U32 in an EXP-07 probe round): `start_discovery` in
  `crates/horch-core/src/harness/launch.rs` (about line 388) sleeps 3 s
  before its first poll. `horch done` (`execution/lifecycle.rs`, `done`,
  about line 60 to 100) closes the pane, which kills `horch worker` and the
  discovery thread. A codex candidate that finishes in under 3 s ends with
  `session_id` empty, and `horch cost --json` lists it in `not_priced` with
  `no_session_id`.
- Fix in 2 parts:
  1. `done` makes 1 synchronous discovery attempt before it closes the pane
     when the record has no session id and the harness discovers sessions
     (same sources as the thread: herdr `agent_session_id` for the pane,
     then `adapter().discover_sessions(ctx, workdir, since, sessions_dir)`
     skipping claimed ids; `since` = the launch marker's mtime). Extract the
     single-attempt logic from the thread into 1 function both call. Best
     effort: an error is logged, `done` goes on.
  2. The thread polls sooner at first (for example 0.5 s, 1 s, 2 s, then
     every 3 s; same total budget of about 3 minutes).
- Parallel unit: U32 `b5-cli` (it does not touch `launch.rs` or `lifecycle.rs`).

## FILES

own:
- `crates/horch-core/src/harness/launch.rs` (discovery only)
- `crates/horch-core/src/execution/lifecycle.rs` (`done` only)
- `crates/horch-e2e/tests/lifecycle.rs` (1 new test)
- `crates/horch-e2e/src/bin/fake-codex.rs` (only if the test needs a
  scenario that runs `horch done` at once; additive)
- `ai_docs/reports/arch-refactor-dataset/session-discovery-race.md`

do not touch: every other file; oracle and golden data.

## STEPS

1. Create the worktree (conventions §2).
2. Extract `discover_once(...) -> Option<String>` from the thread body; the
   thread and `done` call it.
3. `done`: the final attempt as in CONTEXT, before the pane close.
4. The thread's poll schedule as in CONTEXT.
5. Test `tel_session_recorded_when_agent_ends_fast` in
   `crates/horch-e2e/tests/lifecycle.rs`: spawn a codex teammate whose fake
   writes its rollout and runs `horch done` at once; the record ends with
   the fake's session id. Existing `arc_26_e2e_lifecycle_matrix_*` stay green.
6. Gate. Commit `A6: Record the session id of a short agent run`. Write and
   commit the report. Follow conventions §6.

## DONE WHEN

- The new test passes; `just gate` is green.

## REPORT

- `horch done` summary: the change, the poll schedule, the test.
