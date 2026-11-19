# Plan Phase 4 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p4/MAP.md`
**Own:** `docs/tickets/fleetd-p4/`

## Goal

The folder `docs/tickets/fleetd-p4/` holds the tickets and the `MAP.md` that build Phase 4 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 4 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 6 "Phase 4: push dispatch and the result gate", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 4 - task-execution, integration and worker-profiles: Phase 4, push dispatch, the verifier and landing of 1 feature", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 4:
  - This is the largest phase. Read the whole Phase 4 item and each sentence of it. Mechanisms:
    `horch spawn` binds a task before the pane exists and writes `task.yaml` and `START.md`, and
    `CLAIM_TASK` goes away; the Stop hooks for Claude Code, Codex and Antigravity; `horch result`
    and `result.json`; a question file in place of `result.json`, and the outcomes `asked` and
    `quota` burn no attempt; the verifier runs the check and the sweeps (code-sweep rounds,
    findings, the answers file, waivers and verification profiles carry over unchanged, V-14) and
    moves the task to `done`; the landing state machine lands the task branch; the tables
    `features`, `branches`, `bundles`, `bundle_members`, `gate_runs` and `remote_tips`, with the
    `git fetch` and `git push` jobs; each run is 1 feature row and its run branch is the feature
    branch; the feature state machine of that 1 feature and the 2 gates; invariants I2 and I8 to
    I12; the `ingested` table with the checks of SR-6 for `result.json`; the 2 classes of rescue
    refs with `horch rescue drop`; `horch feature abandon`; the bundle state `landed`; the
    deletion of the candidate refs; the resolver and fix attempts of a task branch with their
    lease and `repair_attempts`; `horch branch abandon`; the structural check of a judgment kind.
  - Size: if the phase needs more than 40 tickets, that is correct. Do not make a ticket larger
    to keep the count low. Put the tickets in waves in `MAP.md` (dispatch and result; verifier;
    landing and integration; the tests of the gate) so that the builders of different waves own
    different files.
  - REQUIRED items from the earlier phases. Each one is in exactly 1 ticket:
    1. Tighten the FM-2 test in `crates/horch-e2e/tests/p0_dispatch.rs`: the fake agent records
       at process entry that its task is bound and that `task.yaml` exists.
    2. Fix the format of the gate declaration: `[gates]` with `fast` and `full` in `fleet.toml`
       (the Phase 0 test `p0_landing` uses it; the design leaves the format open).
    3. SR-4: the `task_check` executor runs only in the sandbox profile of the worker. The
       Phase 1 executor is fail-closed without a profile. No sandbox means no worker and no check.
    4. The health guards compare numbers of the same scope (system limit with system count, user
       limit with user count). A fact that is "not supported" gives no hold.
    5. The `task_check` job has a fixed limit of 1200 s in
       `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (`time_limit_s`). Give
       each job kind that has no limit yet (`fast_gate`, `full_gate`, `trial_merge`,
       `bundle_build`, `rebase`) its key in `job_limit_s` of `fleet.toml`, with the number of the
       design, or send QUESTION if the design gives no number.
  - This phase can lose work if it is wrong. Each ticket that moves a ref, lands, pushes, deletes
    a branch or a ref, or changes who runs the finish steps is Review `full` with persona
    `opus-hardening`, and has a crash test (before and after the commit) where the design asks.
  - Self-hosting danger: this phase changes `horch spawn`, the claim and the finish steps. The
    build fleet itself uses the installed `horch` for these. No worker installs a development
    build, and each test runs the development binary only with temporary folders (build rule 1).
    Write this sentence in each ticket.
  - Phases 1, 2 and 3 are built at the same time as you write. Their tickets are in
    `docs/tickets/fleetd-p1/`, `docs/tickets/fleetd-p2/` and `docs/tickets/fleetd-p3/` (read the
    Open questions of each `MAP.md` first). A file that one of their tickets names and that does
    not exist yet is not a fault: name it as "from ticket NN of fleetd-pN".
  - The schema: do not write a schema number in a ticket. Write "the next schema version" and
    tell the worker to read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`.
  - Decisions that hold: a guard whose fact is unknown does not hold; fleetd closes a pane, and
    removes a tree, only with a fact that proves that it is its own, else it opens a needs-you
    item; what the user reads keeps the words of today, and a new event has a word that says what
    happened; a job starts only through the launcher; each ledger write goes through
    `transition()`; each effect is a committed row first; the run duties of part 2 of the pass
    have a work budget and a cursor; OS-specific code is only in `crates/horch-platform`.
  - Before Phase 5 there is no command channel. An operator-only command (`horch feature
    abandon`, `horch branch abandon`, `horch rescue drop`) has the client-side role check of
    today. The server-side operator check (SR-8) is Phase 5. Say this in each such ticket.
  - Each ticket that makes a red test of Phase 0 green (FM-2, FM-6, and the others of the gate
    line) removes its line from the list in `crates/horch-e2e/expected-failures/` and owns both
    files.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - No test reads the real load, the real limits or the real clock of the machine: each test
    gives these facts through the test seams (`HORCH_MACHINE_FILE`, a test `fleet.toml`, the
    fleet clock).
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`.
  - The first ticket of your map is blocked by the gate of Phase 3. Write `T-302` in its
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
     `docs/specs/fleetd/gates/fleetd-p4.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 4 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p4/`.
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
