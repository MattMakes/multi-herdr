# Plan Phase 2 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p2/MAP.md`
**Own:** `docs/tickets/fleetd-p2/`

## Goal

The folder `docs/tickets/fleetd-p2/` holds the tickets and the `MAP.md` that build Phase 2 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 2 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 4 "Phase 2: leases and capped waits", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 2 - task-execution: Phase 2, leases, capped waits and waiting_quota", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 2:
  - Mechanisms of the row Phase 2 of "What each gate needs": the `leases` table and the lease part
    of I1; renewal only from earned activity; the wrapper heartbeat; the stall ladder and rung 6
    with `not_before` on the fleet clock; `max_wait_s` for `worker_waits` of an interactive
    persona; `waiting_quota`, the required `fallback` persona in the roster and the restart on a
    fallback of another provider; the `questions` table; `horch task requeue` and
    `horch task cancel`; the exit to `cancelled` from each task state that is not terminal, with
    the stop of a live attempt first; the full task state table of 12 states in `transition()`;
    the backfill and the "run in flight" test of Phase 2.
  - Phase 1 is built at the same time as you write. Its tickets are in `docs/tickets/fleetd-p1/`
    (read its `MAP.md`, Open questions, first). The code of Phase 1 is in
    `crates/horch-core/src/fleetd/` and `crates/horch-platform/`. A file that a ticket of Phase 1
    names and that does not exist yet is not a fault: name it as "from ticket NN of fleetd-p1".
  - The schema. The main branch has schema version 8. The fleetd migration of Phase 1 is the next
    version after the version of main. Do not write a schema number in a ticket: write "the next
    schema version" and tell the worker to read `VERSION` in
    `crates/horch-core/src/taskledger/schema.rs`.
  - Decisions that hold from Phase 1 (they are the operator's or the orchestrator's): the load
    HOLD starts at 4.0 for each CPU and the load note of today stays; a guard whose fact is
    unknown does not hold; fleetd closes a pane only with a second fact that proves it, else it
    opens a needs-you item; a process start time is an INTEGER; what the user reads in
    `horch ledger show` and `horch ledger events` keeps the words of today, and a new event has a
    word that says what happened.
  - The design leaves 1 number open: the usage of a subscription pool that counts as "full use".
    Give it a key in `fleet.toml` with a starting value that you state with its reason, and list
    it in MAP.md, Open questions, as a question for the operator.
  - Each ticket that makes a red test of Phase 0 green (FM-3, FM-5, the machine sleep) removes its
    line from the list in `crates/horch-e2e/expected-failures/` and owns both files.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Self-hosting: each test uses temporary folders (build rule 1). No ticket registers a real
    service, and no ticket stops or reaps a real worker of this machine.
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`.
  - The first ticket of your map is blocked by the gate of Phase 1. Write `T-92` in its
    `Blocked by` cell (and in the cell of each other ticket that has no blocker of this map).

## Steps

1. Read the sources above. Then read the real code that the phase changes. Name real paths, real
   functions and real test names in each ticket. Do not guess a path: check it.
2. Split the phase into tickets. Rules for the split:
   - 1 ticket has 1 owner and a clear set of files in `Own`. Two tickets that can run at the
     same time must not own the same file. If 2 tickets need 1 file, give the second a
     `Blocked by` edge on the first.
   - 1 ticket is work that 1 worker completes in 1 session (a guide: at most about 600 changed
     lines, tests included). Split a larger unit.
   - Tests come first in each ticket: the ticket names the tests to write and the reason for which
     each one fails before the code.
   - Each ticket has a real `Check` command that proves it and that runs in less than 20 minutes.
     The check must not need a second OS. Where a result is necessary on Linux or Windows, the
     ticket says that CI gives it, and the worker makes the code compile for that target as far
     as this Mac can check (`cargo check --target ...` when the target is installed).
   - Persona: `opus` for work that needs judgment; `opus-hardening` for races, crash safety
     and security; `qa-engineer` or `sonnet-sweep` only for work that the ticket spells out
     fully. Fallback `codex-sol`. Review: `full` for code that can lose work or data,
     `light` for other code, `none` for docs.
   - The last ticket is the gate ticket of the phase: persona `opus`, blocked by each other
     ticket. It runs the full gate of the phase as the design states it (each requirement and
     failure mode of the gate line has a test that passes and is off the list of expected
     failures; the "run in flight" test of the phase passes), corrects small faults, and writes
     `docs/specs/fleetd/gates/fleetd-p2.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 2 in "What each gate needs" and in "New work for
   each phase" that this phase gives is in the `Requirements` or `Steps` of exactly 1 ticket.
   Put the coverage table at the end of `MAP.md`.
6. Run the Check. Correct each error. Commit.

## Constraints

- Simplified Technical English. Short sentences. Never write the em dash character; use `-`.
- Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send
  BLOCKED.
- Commit messages have no co-author line.
- NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it.
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p2/`.
- Do not add a feature that the design does not name. If the design leaves a point open and the
  phase needs it, send a QUESTION to the orchestrator and end your turn.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

- The Check passes with no error.
- Each ticket names only real paths, has a real Check, and has the 10 build rules.
- The coverage table has no mechanism of the phase without a ticket.

## Report

In the summary of `horch ledger done`: the count of tickets, the waves, which tickets can run
at the same time, the open questions, and each risk that you see for the phase.
