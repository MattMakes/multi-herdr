# D12 lifecycle-gaps: close the last pane and session races

Unit slug: `lifecycle-gaps`. Branch: `ds/lifecycle-gaps`.

## GOAL

`horch done` never fails because the pane is already gone, Prime gets the
same final session-discovery attempt as the other discovering harnesses, and
fake-herdr never reuses a pane id after a close.

## CONTEXT

- Read first: `00-conventions.md`, `ai_docs/reports/design-skills/followups.md`
  (open items), `ai_docs/reports/arch-refactor-dataset/session-discovery-race.md`.
- Gap 1: `crates/horch-core/src/execution/lifecycle.rs` `done`: D10 made a
  failed close succeed when the pane is gone, but `done` still fails when
  its earlier `pane get` (to resolve its own workspace or pane) runs after
  the dataset coordinator closed the pane. A worker that ran `horch done`
  has finished its work: record the summary and the terminal state first,
  then treat a missing pane at any later step as "already closed". Never
  lose the summary.
- Gap 2: `done` calls `launch::discover_now`, but it has no `sessions_dir`,
  so Prime (session file under `--session-dir`) gets no final attempt. Find
  where the worker knows Prime's session dir (the launch request or the
  brief) and make it reachable from `done` (record it on the execution or
  in the brief at launch). Test with fake-prime: a Prime worker that runs
  `horch done` at once still records its session id.
- Gap 3: `crates/horch-e2e/src/bin/fake-herdr.rs` reuses pane ids after
  close. Real herdr ids are unique per server. Make ids monotonic (never
  reused) in the fake's state. Check that no test depends on reuse.

## FILES

own:
- `crates/horch-core/src/execution/lifecycle.rs`, `execution/store.rs`
  (only a field for Gap 2 if needed), `crates/horch-core/src/harness/launch.rs`
  (discovery only), `crates/horch-core/src/messaging/brief.rs` (only if the
  brief carries the session dir), `crates/horch/src/cmd/messaging.rs` (the
  `done` steps only)
- `crates/horch-e2e/src/bin/fake-herdr.rs`, `crates/horch-e2e/src/bin/fake-prime.rs`
- tests: unit tests next to the code; `crates/horch-e2e/tests/lifecycle.rs`
- `ai_docs/reports/design-skills/lifecycle-gaps.md`

do not touch: competition, skills, teammates, oracles. A serialization
change to the brief or ledger must stay backward compatible (serde default)
and must not change a golden; if it does, send `QUESTION:`.

## STEPS

0. Create the worktree (conventions §3).
1. Gap 1 with a unit test (`done` with a `FakeWorkspace` whose pane vanishes
   before each step: summary recorded, exit 0). Gate. Commit.
2. Gap 2 with `tel_session_recorded_when_prime_ends_fast` (e2e). Gate. Commit.
3. Gap 3 with a fake-herdr unit test or an e2e check. Gate. Commit.
4. Report (each gap: cause, fix, test). Follow conventions §7.
