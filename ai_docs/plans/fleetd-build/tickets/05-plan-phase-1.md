# Plan Phase 1 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p1/MAP.md`
**Own:** `docs/tickets/fleetd-p1/`

## Goal

The folder `docs/tickets/fleetd-p1/` holds the tickets and the `MAP.md` that build Phase 1 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 1 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 3 "Phase 1: `horch fleetd`, supervised and level-triggered", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 1 - fleet-runtime and task-execution", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 1:
  - This is the largest phase so far. Expect 15 to 30 tickets. Split by mechanism of the row
    Phase 1 of "What each gate needs": the schema migration and its backup; `transition()` with
    its table; the `jobs` table, job kinds, run number, `abandoned_run_limit`, `job_backoff_s`
    and `horch job requeue`; the 3-part pass with intended rows; `pass_cursors` and the budgets;
    the fleet clock; the fleetd file lock and generation; the launcher; the `needs_you` table;
    `horch fleetd` and `horch fleetd check`; the service definitions of each OS (through
    `crates/horch-platform`); I4, I5 and the process part of I1; the health guards and pacing;
    the stop of the old watchdog and the backfill of "What a release does to a run in flight".
  - New code goes in the module layout of `ddd/08-code` (`crates/horch-core/src/fleetd/<context>/`
    with `domain/`, `application/`, `adapters/`). The ticket that creates the first module of a
    context adds the fitness tests that guard it in `crates/horch-core/tests/arch_scan.rs` and
    the row in `docs/architecture.md` (the Phase 0 plan left these to the later phases; see
    `docs/tickets/fleetd-p0/MAP.md`, Open questions 3).
  - The platform layer of Phase P is built at the same time as you write. Its tickets are in
    `docs/tickets/fleetd-pp/`; the concern tickets 03 to 09 propose the function names, and the
    module doc of each concern file in `crates/horch-platform/src/` gives the final names when
    it exists. Name a platform function by its concern and its proposed name, and tell the worker
    to read the module doc for the final name.
  - Each ticket that makes a red test of Phase 0 green removes its line from the list in
    `crates/horch-e2e/expected-failures/` (read its README) and puts both files in `Own`.
    Required: the setup of the herdr restart test in `crates/horch-e2e/tests/p0_rescue.rs`
    observes the lock of the old watchdog; the ticket that stops the old watchdog changes that
    setup to the fleetd interface. The test of FM-34 in `p0_supervision.rs` uses a changed binary
    as its trigger (preservation-04 row S10); keep it. `p0_supervision.rs` uses the fault point
    `HORCH_FAULT=hang-pass`; Phase 1 adds it to fleetd.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Self-hosting is the main risk of this phase: Phase 1 adds a daemon, a service definition and
    a schema migration to the tool that runs this fleet. Each ticket must make its tests use
    temporary folders (build rule 1), and no ticket may register a real service. A service
    definition is tested as text (the unit file, the plist, the task XML) and through a stand-in
    for the service manager.
  - The first ticket of your map is blocked by the gate of Phase P. Write `T-32` in its
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
     `docs/specs/fleetd/gates/fleetd-p1.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 1 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p1/`.
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
