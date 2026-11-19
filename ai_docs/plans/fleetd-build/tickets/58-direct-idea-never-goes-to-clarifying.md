# An idea of `horch idea add --direct` goes to ticketing in each pass order, never to clarifying

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_handlers_idea && cargo test -p horch-e2e --test p6_idea_flow_binary && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/ideas/application/direct.rs`, `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs`

## Goal

An idea that the operator adds with `horch idea add --direct` reaches `ticketing` and gets its
planned tasks, for each order of the request commit, the file writes and a pass. It never goes to
`clarifying`, and fleetd launches no idea-lead attempt for it.

## Context

Found by T-561 (opus-153) on the run branch at 95ef93f3 (with T-894). It is a race:

- `AddDirectIdea` commits the idea row in state `captured`.
- `DirectAdded::write` writes `plan.yaml` only after that commit
  (`crates/horch-core/src/fleetd/ideas/application/direct.rs`, about line 124). This order is
  right by design rule A24: effects run after the commit.
- The dispatcher step for `captured` (`dispatcher.rs`, about line 223) chooses `ticketing` only
  when `plan.yaml` exists. A pass between the commit and the write moves the idea to `clarifying`,
  and nothing moves it back.
- Evidence: idea I-0001 is in `clarifying`, its folder has `plan.yaml`, `approval.json` and the
  ticket, fleetd launches an idea-lead attempt for it, and no task is planned.

Read first: `horch ledger show T-524`, `T-553`, `T-894`; fleetd-p6 tickets 17 and 51
(`docs/tickets/fleetd-p6/`); in `docs/specs/fleetd/design.md` the states of an idea, the direct
intake (FR-10 or the name there), and rule A24.

## Steps

1. Write 1 test that fails today with no sleep and no luck: the request of a direct idea is
   committed, the dispatcher step runs BEFORE the effects write `plan.yaml`, then the effects run,
   then the dispatcher runs again. The idea must be in `ticketing` and must never be in
   `clarifying`. Check: the test is red.
2. Fix the cause, not the timing. The fact 'this idea is direct' must be in the ledger row that
   the request commits (a field that the schema has today, for example the intake kind or the
   effects object of T-524), and the dispatcher must decide from the ledger, not from a file that
   an effect writes later. While the files of a direct idea are not written yet, the dispatcher
   leaves the idea in `captured` for the next pass. If the schema has no field for this fact, send
   QUESTION with 2 options and end your turn (no schema step without an answer).
3. Check the crash case: fleetd stops after the commit and before the file write. After the
   start, the effects run again (or the repair of the design writes the files), and the idea
   reaches `ticketing`. Add 1 test, or name the test that proves it.
4. Search the dispatcher and the other idea steps for each decision that reads a file which an
   effect writes after a commit. Send 1 NOTE with the list; fix each one that has the same race.
5. Add 1 e2e test to `crates/horch-e2e/tests/p6_idea_flow_binary.rs` with `pass_period_s = 1`:
   10 direct ideas in a row all reach `ticketing`. Check: green 3 times in a row.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit.
- Do not make an assert weaker and do not add an arch exception.
- An e2e test serves a test fleetd (`Harness::serve_fleetd`), makes its workspace with
  `Harness::fleet_workspace`, and sets `HORCH_TEST_DOCS_FIXTURE` to an empty local folder. Each
  test command has a time limit. Run only your own test files. Send `horch ledger progress` each
  5 min in long test runs. Merge the run branch each 30 minutes and before finish.

## Done when

- The tests of steps 1, 3 and 5 pass, and the Check passes.
- The done summary names: the ledger fact that the dispatcher reads, the list of step 4, and each
  file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
