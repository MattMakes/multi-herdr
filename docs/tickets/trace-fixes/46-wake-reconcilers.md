# 46: The watchdog reconciles every wake that a crash after commit can lose

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- watchdog outbox trf_22 trf_51 && cargo test -p horch -- wake watch trf_51`
**Own:** `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch/src/cmd/taskledger_wake.rs`, `crates/horch/src/cmd/taskledger_watch.rs`

Spec: `docs/specs/trace-fixes.md` (C2, C3, C7, D7, R10; conventions in section 8). Added by the orchestrator after review 31.
Review: `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/31.md`. Read your findings there first; they give file:line, a reproduction and a correction.
Waits for: 44, 45 (they edit taskledger_watch.rs and taskledger_wake.rs).

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Goal

Fix review findings I-03 to I-07. Decision R10 accepted a crash window between a ledger commit and its outbox
write, with the 30 s pass as the backstop. Review 31 found 5 events the backstop does not recover:
`settled` (I-03), `reviews-in` (I-04), `cancelled` dependents (I-05), `held` unlanded blocker (I-06), and the
held-attempt question (I-07).

## Requirements

- **TRF-51**: each 30 s watchdog pass derives these 5 wakes from ledger state and writes each missing one, deduplicated by its C3 key, so a crash after any commit loses no wake. Test names start with `trf_51_`.

## Steps

1. Reconcile from state, not from a missed event. On each pass, for the active run:
   - settled run (the ticket 11 predicate) with no `settled` row for the current board hash;
   - a review group whose reviews are all done while its author still waits, with no `reviews-in` row;
   - a cancelled task with open dependents, with no `cancelled` row for it;
   - a parked (unlanded) task with open dependents, with no `held` row for it;
   - a task held by the attempt limit with no `held`/question row for it.
2. Use the same keys and text builders as the direct wake paths, so a direct wake and a reconciled wake dedupe to 1 row.
3. Remove the "suppressed while any worker is live" condition only where it hides these 5 events.
4. Tests with a fake clock: 1 `trf_51_` test per event: the ledger change commits, the direct wake is skipped, 1 pass writes exactly 1 row, a 2nd pass writes none.

Amendment (report 45): ticket 45 made a replayed outbox row reuse its first mail file (`mail::keyed`, `Mail::enqueue_with_key` in `crates/horch-core/src/messaging/mail.rs`), but a replay of a row whose mail was already published still pushes the line again through `tell_as` (`crates/horch/src/cmd/messaging.rs`, `outgoing.push()`). Make the keyed enqueue tell its caller "already published", and make `tell_as` skip the push then. `crates/horch/src/cmd/messaging.rs` and `mail.rs` are approved outside Own. Add 1 test `trf_51_replayed_published_row_pushes_nothing`.

Amendment (report 47): ticket 47 added a claim reservation (`workers.target_task_id`, `reserved_for_others` in `crates/horch-core/src/taskledger/tasks.rs`). `plan_workers` (`tasks.rs`) and `startable_rewake` (`watchdog.rs`) still count a task that a live, unclaimed worker reserves as startable, so they can ask for 1 extra worker that finds no task. Count such a task as having a worker on its way. `tasks.rs` is approved outside Own. Add 1 test `trf_51_a_reserved_task_needs_no_extra_worker`.

## Done when

The check passes. The report maps I-03 to I-07 to the 5 tests.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/46.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
