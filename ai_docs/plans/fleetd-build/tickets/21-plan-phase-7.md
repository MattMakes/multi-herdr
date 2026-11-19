# Plan Phase 7 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p7/MAP.md`
**Own:** `docs/tickets/fleetd-p7/`

## Goal

The folder `docs/tickets/fleetd-p7/` holds the tickets and the `MAP.md` that build Phase 7 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 7 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 9 "Phase 7: docs sync and conformance, then exec mode", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 7 - worker-profiles: Phase 7, docs sync, conformance, then exec mode", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 7:
  - This is the last phase. Mechanisms of the Phase 7 item and of the row Phase 7 of "What each
    gate needs": `herdr-docs-sync` for each CLI; the conformance suite and the unhealthy mark of
    a profile; fleetd queues the `docs_sync` and `conformance` jobs (the kinds are in the `jobs`
    table from Phase 1, with their keys in `job_limit_s`); a run of these jobs with no repository
    is a valid row, and a daily run starts a new run; the check "Operator boundary" (SR-8) of
    the suite, for each adapter; exec mode as the default worker mode; `context-windows.md` set
    to 256000 for each seat.
  - REQUIRED items from the earlier phases. Each one is in exactly 1 ticket:
    1. After this phase the lists in `crates/horch-e2e/expected-failures/` are empty. The gate
       ticket checks it, and removes the mechanism only if the design says so; if the design
       does not say, the mechanism stays with empty lists.
    2. The final check of the whole build, in the gate ticket: each requirement id and each
       failure mode id of the design (FR, RR, SR, PL, OR, FM, I) has a row in 1 of the gate
       files `docs/specs/fleetd/gates/fleetd-p0.md` to `fleetd-p7.md` with a test that passes.
       The gate ticket writes the table "id, phase, test, result" for each id and names each id
       with no test. An id with no test is a red gate.
    3. The list of each row that is "pending CI" (Windows, and Linux where Docker did not give
       it) in the 8 earlier gate files, as 1 table in the gate file of Phase 7, for the CI run of
       the pull request.
    4. The list of each deviation from the design that the gate files and the `MAP.md` Open
       questions of the 8 phases name, as 1 table in the gate file of Phase 7, for the operator.
    5. The docs for a user: a ticket updates `README.md`, the docs under `docs/` that describe
       the old flow, the skills and the teammate files, so that a new user can install, start
       fleetd, add an idea and read the status with the new commands. `CHANGELOG.md` is
       generated: no ticket edits it by hand.
  - Network: the docs sync reads the public docs of each CLI. No test uses the network: each
    test gives the pages through a local fixture. A ticket never sends project text, a name or
    an email address to an outside service.
  - Exec mode as the default changes how each worker runs. The "run in flight" test of Phase 7
    proves that an interactive worker that is live at the update ends its task. Each persona of
    `teammates/` keeps its fallback rule: a fallback never lowers confidentiality, and a persona
    with a written no-fallback reason keeps it (task T-261).
  - Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No
    worker installs a development build, and each test runs the development binary only with
    temporary folders (build rule 1). Write this sentence in each ticket.
  - Phases 2 to 6 are built at the same time as you write. Phase 1 is complete on the branch.
    The tickets of the others are in `docs/tickets/fleetd-p2/` to `docs/tickets/fleetd-p6/`
    (read the Open questions of each `MAP.md` first). A file that one of their tickets names and
    that does not exist yet is not a fault: name it as "from ticket NN of fleetd-pN".
  - The schema: if the phase needs a schema step, do not write a schema number. Write "the next
    schema version" and tell the worker to read `VERSION` in
    `crates/horch-core/src/taskledger/schema.rs`. Some tests pin the version and undo each step
    by hand (`crates/horch-core/tests/taskledger_schema_v8.rs`, `taskledger_queue.rs`,
    `trf_48_ledger_guard.rs`): the schema ticket owns them.
  - Decisions that hold: a guard whose fact is unknown does not hold; what the user reads keeps
    the words of today, and a new event has a word that says what happened; a job starts only
    through the launcher; only fleetd writes the ledger, through `transition()`; each effect is
    a committed row first; the scans and the duties of the pass have a work budget and a cursor;
    OS-specific code is only in `crates/horch-platform`; no sandbox for a profile on an OS means
    no dispatch of that profile and 1 needs-you item; each operator command is checked on the
    server side; no agent has operator rights.
  - Spend: the fleet has no budget cap and no cap on parallel work (decision D8 of the design).
    Do not add one.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - No test reads the real load, the real limits or the real clock of the machine, and no test
    depends on the speed of the host. No test starts a real agent CLI: each test uses the fake
    agents of `crates/horch-e2e`.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`.
  - The first ticket of your map is blocked by the gate of Phase 6. Write `T-567` in its
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
     `docs/specs/fleetd/gates/fleetd-p7.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 7 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p7/`.
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
