# Plan Phase 6 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p6/MAP.md`
**Own:** `docs/tickets/fleetd-p6/`

## Goal

The folder `docs/tickets/fleetd-p6/` holds the tickets and the `MAP.md` that build Phase 6 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 6 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 8 "Phase 6: the idea dispatcher and sub-orchestrators", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 6 - ideas and integration: Phase 6, the dispatcher, several features and the judge pair", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 6:
  - Read the whole Phase 6 item and each sentence of it. Mechanisms: the `ideas` state machine
    and its schema step; `horch idea add` (with the `--direct` fast lane and `--to`), `ideas`,
    `show`, `note`, `approve`, `revise` and `park`; the dispatcher loop of fleetd and invariant
    I7; the `idea-lead` persona (brief, spec-writer and ticket-writer steps for 1 idea in its own
    interactive pane, a fresh process for each phase); `plan.yaml` in place of `MAP.md`, with
    Markdown tickets in format C1; the plan validator from `add-map --dry-run` and
    `scripts/check-req-coverage.sh` (schema, DAG, coverage, profiles, citations to approved
    assumptions); the end of the long-lived orchestrator conversation; 1 feature branch for each
    feature and several features at the same time, in place of 1 non-solo run for each
    workspace; the idea level; the `trial_merges` table; stacks; the rules of the feature level
    (1 sub-orchestrator for each feature, I7, I13, a merge with ideas in flight, the exit of
    feature `open` to `merged`); invariant I3; the judge pair, and the rule that a cancel never
    leaves a judgment task with no judge task; the question flow with the sub-orchestrator as
    the owner of a question.
  - REQUIRED items from the earlier phases. Each one is in exactly 1 ticket:
    1. Remove the orchestrator exemption of Phase 5 (`docs/tickets/fleetd-p5/MAP.md`, Open
       questions, Q2): after this phase no agent has operator rights. Add the attack test: an
       agent in the old orchestrator seat sends an operator request and is refused.
    2. Retire each request kind of Phase 5 that a Phase 6 command replaces (the request-kinds
       table of `docs/tickets/fleetd-p5/MAP.md`). The table of the design is the result.
    3. FM-9 is green in this phase, with the judge pair (Phase 4 gave the part test for a code
       kind and the structural check).
    4. "Ideas finish clean": the removal of the rescued trees of an idea when it merges or is
       cancelled (`docs/tickets/fleetd-p3/MAP.md`, Open questions, Q8, moved this here). Phase 3
       used "the run is done or abandoned" as the stand-in for an idea; replace the stand-in.
    5. The stand-in of Phase 4 "each task of the run is done or cancelled" for an idea
       (`docs/tickets/fleetd-p4/MAP.md`, Q20): replace it with the idea state.
  - What a user does today must still work, or have a clear new command. Make 1 table in
    `MAP.md`: each command and each skill step of today's orchestrator flow (`horch ledger run
    new`, `add-map`, `task`, `plan-workers`, `horch spawn`, `waive`, `run gate`, `run close`, the
    skill `horch:orchestrate`), and what replaces it. A ticket updates the skills, the teammate
    files and the docs that name the old flow. The preservation files
    `docs/specs/fleetd/preservation-*.md` list the behaviors that must stay.
  - A run in flight: at the update, a run of the old kind (1 orchestrator, 1 run branch) is live.
    The "run in flight" test of Phase 6 proves that it ends cleanly or moves to a feature with
    no lost task. Read "What a release does to a run in flight".
  - This phase can lose work if it is wrong (a merge with ideas in flight, stacks, trial merges,
    a cancel). Each ticket that moves a ref, merges, cancels or changes the owner of a question is
    Review `full` with persona `opus-hardening`.
  - Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself
    uses. No worker installs a development build, and each test runs the development binary only
    with temporary folders (build rule 1). Write this sentence in each ticket.
  - Size: if the phase needs more than 50 tickets, that is correct. Do not make a ticket larger
    to keep the count low. Put the tickets in waves in `MAP.md` so that the builders of
    different waves own different files.
  - Phases 2 to 5 are built at the same time as you write. Phase 1 is complete on the branch.
    The tickets of the others are in `docs/tickets/fleetd-p2/` to `docs/tickets/fleetd-p5/`
    (read the Open questions of each `MAP.md` first). A file that one of their tickets names and
    that does not exist yet is not a fault: name it as "from ticket NN of fleetd-pN".
  - The schema: do not write a schema number in a ticket (the design says "v9" for this phase;
    that number is old). Write "the next schema version" and tell the worker to read `VERSION`
    in `crates/horch-core/src/taskledger/schema.rs`. Some tests pin the version and undo each
    step by hand (`crates/horch-core/tests/taskledger_schema_v8.rs`, `taskledger_queue.rs`,
    `trf_48_ledger_guard.rs`): the schema ticket owns them.
  - Decisions that hold: a guard whose fact is unknown does not hold; fleetd closes a pane, and
    removes a tree, only with a fact that proves that it is its own, else it opens a needs-you
    item; what the user reads keeps the words of today, and a new event has a word that says what
    happened; a job starts only through the launcher; each ledger write goes through
    `transition()` and, from Phase 5, only fleetd writes the ledger; each effect is a committed
    row first; the scans and the duties of the pass have a work budget and a cursor; OS-specific
    code is only in `crates/horch-platform`; no sandbox for a profile on an OS means no dispatch
    of that profile and 1 needs-you item; each operator command is checked on the server side.
  - Spend: the fleet has no budget cap and no cap on parallel work (decision D8 of the design).
    Do not add one.
  - Each ticket that makes a red test of Phase 0 green removes its line from the list in
    `crates/horch-e2e/expected-failures/` and owns both files. After this phase the lists have
    only the lines of Phase 7.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - No test reads the real load, the real limits or the real clock of the machine, and no test
    depends on the speed of the host: each test gives these facts through the test seams
    (`HORCH_MACHINE_FILE`, a test `fleet.toml`, the fleet clock) and waits for an event, not for
    a time. No test starts a real agent CLI: each test uses the fake agents of `crates/horch-e2e`.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`.
  - The first ticket of your map is blocked by the gate of Phase 5. Write `T-498` in its
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
     `docs/specs/fleetd/gates/fleetd-p6.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 6 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p6/`.
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
