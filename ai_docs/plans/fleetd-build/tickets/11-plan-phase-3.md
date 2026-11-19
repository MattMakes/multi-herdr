# Plan Phase 3 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p3/MAP.md`
**Own:** `docs/tickets/fleetd-p3/`

## Goal

The folder `docs/tickets/fleetd-p3/` holds the tickets and the `MAP.md` that build Phase 3 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 3 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 5 "Phase 3: worktree leases and rescue", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 3 - worktree-custody: Phase 3, worktree leases and rescue", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 3:
  - Mechanisms of the row Phase 3 of "What each gate needs" and of the Phase 3 item: the state,
    owner, base and rescue columns of `worktrees`; fleetd (through `horch spawn` until Phase 4) as
    the only code that runs `git worktree add` or `remove`, with the row first; the
    `worktree_add`, `worktree_remove` and `worktree_list` jobs; the worktree reconciler in
    report-only mode, then acting, only after the tests of RR-13 to RR-18 are green; the verified
    rescue to `refs/horch/rescue/`; reuse by the next attempt; I6; `horch worktrees` and
    `horch worktree restore`; the refusals of today's drop, cleanup and triage and the record of
    unlanded work (RR-13 to RR-18); the backfill and the "run in flight" test of Phase 3; the
    worktree disk budget.
  - This phase can lose work if it is wrong. Each ticket that removes, moves or cleans a tree is
    Review `full` with persona `opus-hardening`, and has a test that proves that dirty files,
    untracked files and commits that are not landed survive.
  - Phases 1 and 2 are built at the same time as you write. Their tickets are in
    `docs/tickets/fleetd-p1/` and `docs/tickets/fleetd-p2/` (read the Open questions of both
    `MAP.md` files first). A file that one of their tickets names and that does not exist yet is
    not a fault: name it as "from ticket NN of fleetd-p1" or "of fleetd-p2".
  - The schema: do not write a schema number in a ticket. Write "the next schema version" and
    tell the worker to read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`.
  - Decisions that hold: a guard whose fact is unknown does not hold; fleetd closes a pane, and
    removes a tree, only with a fact that proves that it is its own, else it opens a needs-you
    item; what the user reads keeps the words of today, and a new event has a word that says what
    happened; a job starts only through the launcher.
  - Each ticket that makes a red test of Phase 0 green (FM-25, the full FM-13) removes its line
    from the list in `crates/horch-e2e/expected-failures/` and owns both files.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Self-hosting: each test makes its own temporary git repository and temporary folders (build
    rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a
    `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its
    `.worktrees/` folder. Write this sentence in each ticket.
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`.
  - The first ticket of your map is blocked by the gate of Phase 2. Write `T-273` in its
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
     `docs/specs/fleetd/gates/fleetd-p3.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 3 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p3/`.
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
