# Plan Phase 5 of the fleetd build: write its tickets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p5/MAP.md`
**Own:** `docs/tickets/fleetd-p5/`

## Goal

The folder `docs/tickets/fleetd-p5/` holds the tickets and the `MAP.md` that build Phase 5 of fleetd.
A worker with no other context can do each ticket. Together the tickets make the gate of Phase 5 green.

## Context

- The run `fleetd-build` builds fleetd in the nine phases of the design. You write the tickets
  of 1 phase. You write no code.
- Source of truth: `docs/specs/fleetd/design.md` (FINAL), section "Migration plan": item 7 "Phase 5: one ledger writer", and the
  tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in
  flight", "New work for each phase" and "The numbers of the design". Read also each section of
  the design that the phase names (Requirements, Failure modes and effects, Preservation analysis)
  and the files `docs/specs/fleetd/preservation-*.md`.
- Second source: `ddd/08-code/implementation-plan.md`, "Slice 5 - fleet-runtime: Phase 5, one ledger writer and the operator boundary", and the `design.md` files of the
  contexts in `ddd/08-code/`. Where they do not agree, the design wins.
- The ticket format is `docs/specs/ticket-format.md` (contract C1). Follow it exactly.
- The build rules are in
  `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-build/build-rules.md`.
- Notes for Phase 5:
  - Mechanisms of the Phase 5 item and of the row Phase 5 of "What each gate needs": the worker
    commands (`progress`, `result`, `ask`) write files into the outbox of the attempt and fleetd
    ingests them; each other process opens the ledger read-only, with a new `arch_scan` rule;
    the removal of the `broad_ledger_rule` sandbox escape of Codex; each `horch` command that
    changes state sends a request to fleetd over its command channel, or writes a request file
    when fleetd is down; the `requests` table and `horch request`; the peer-user check of the
    channel; the modes of the request folder; the checks of SR-6 for each worker file and request
    file; the class of each request kind; the operator token; the deny rules of the worker
    sandbox with their profile validation; the peer process check; the refusal of a profile on
    an OS that cannot sandbox it; the `class` and `auth` columns of `requests` (SR-8); the
    Windows transport of the command channel.
  - This phase is the security boundary of the fleet. Each ticket that touches the channel, the
    token, the request files, the ingest of a worker file or a sandbox rule is Review `full`
    with persona `opus-hardening`. Each such ticket has an attack test: a worker in its sandbox
    tries the forbidden act and is refused.
  - REQUIRED items from the earlier phases. Each one is in exactly 1 ticket:
    1. The Windows command-channel pipe: a process of the same user can create a second pipe
       instance with the channel name and intercept a client. One of these fixes is REQUIRED
       before a command uses the channel. Choose 1 and say why in Open questions: (a) 1 pipe
       instance only (`nMaxInstances` 1 with `FILE_FLAG_FIRST_PIPE_INSTANCE`), and a client
       tries again on `PIPE_BUSY`; or (b) the worker sandbox denies the creation of a pipe with
       that name, and the client compares `GetNamedPipeServerProcessId` with the fleetd PID and
       start time of the ledger. The peer process check of the design is necessary in each case.
    2. The unix socket: a process of the same user can remove the socket file in the 0700 folder
       and bind its own. The client checks the peer process (PID and start time of the fleetd
       row), and the worker sandbox denies a write in the run folder.
    3. The server-side operator check (SR-8, A35) for each operator-only command. Today these
       commands trust `HORCH_ROLE`, which a worker controls: `horch job requeue`,
       `horch ledger requeue`, `horch ledger cancel`, `horch ledger waive`, and the operator
       commands that Phases 2, 3 and 4 add (read their `MAP.md` files and list each one). Each
       such command gets a test: a worker that sets or removes `HORCH_ROLE` is refused.
    4. The pane proof of a politely stopped worker is a file,
       `run/stops/<pid>-<start>.json` (task T-414). Decide if it stays a file with the SR-6
       checks of a fleetd-owned file, or moves to a ledger row. Say the choice in Open questions.
    5. The review workers (`codex-reviewer-light`) get "attempt to write a readonly database" on
       `horch ledger done` and `horch ledger progress` today, because their sandbox makes the
       ledger read-only. After this phase each worker report is a file, so this fault must be
       gone. Add an e2e test: a worker whose sandbox makes the ledger read-only reports progress
       and done with no error.
  - Migration and a run in flight: at the update, workers of the old version still write the
    ledger directly. The "run in flight" test of Phase 5 proves that no report is lost and that
    no run is stopped. Read "What a release does to a run in flight" and "Who writes the ledger
    before Phase 5".
  - Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The
    build fleet itself uses the installed `horch`. No worker installs a development build, and
    each test runs the development binary only with temporary folders (build rule 1). Write
    this sentence in each ticket.
  - Size: if the phase needs more than 40 tickets, that is correct. Do not make a ticket larger
    to keep the count low. Put the tickets in waves in `MAP.md` so that the builders of
    different waves own different files. A good order: the channel transport and its checks;
    the requests table and the request files; the commands, 1 group of commands for each ticket;
    the worker outbox files and the ingest; the read-only rule last, when no other writer is left.
  - Phases 1 to 4 are built at the same time as you write. Phase 1 is complete on the branch.
    The tickets of the others are in `docs/tickets/fleetd-p2/`, `docs/tickets/fleetd-p3/` and
    `docs/tickets/fleetd-p4/` (read the Open questions of each `MAP.md` first). A file that one
    of their tickets names and that does not exist yet is not a fault: name it as "from ticket
    NN of fleetd-pN".
  - The schema: do not write a schema number in a ticket. Write "the next schema version" and
    tell the worker to read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`.
  - Decisions that hold: a guard whose fact is unknown does not hold; fleetd closes a pane, and
    removes a tree, only with a fact that proves that it is its own, else it opens a needs-you
    item; what the user reads keeps the words of today, and a new event has a word that says what
    happened; a job starts only through the launcher; each ledger write goes through
    `transition()`; each effect is a committed row first; the scans and the run duties of the
    pass have a work budget and a cursor; OS-specific code is only in `crates/horch-platform`;
    no sandbox for a profile on an OS means no dispatch of that profile and 1 needs-you item.
  - Each ticket that makes a red test of Phase 0 green (FM-15, FM-17, FM-23, FM-24 and the
    others of the gate line) removes its line from the list in
    `crates/horch-e2e/expected-failures/` and owns both files.
  - The Check of each ticket also runs the guard tests:
    `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`. A ticket that
    adds a crate, a type name or a module adds the list entry that these tests need, and owns
    that test file. A ticket that changes a `Cargo.toml` runs the tests of that crate.
  - No test reads the real load, the real limits or the real clock of the machine, and no test
    depends on the speed of the host: each test gives these facts through the test seams
    (`HORCH_MACHINE_FILE`, a test `fleet.toml`, the fleet clock) and waits for an event, not for
    a time.
  - The check of the run is `cargo test --workspace --locked` (see `docs/tickets/fleetd-p0/MAP.md`).
  - Windows and Linux: this Mac compiles only macOS code. Keep new OS code small and in
    `crates/horch-platform`. The Windows transport has no test on this Mac: its ticket says what
    CI must prove, and names each risk.
  - The first ticket of your map is blocked by the gate of Phase 4. Write `T-404` in its
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
     `docs/specs/fleetd/gates/fleetd-p5.md`: each row of the gate, its test name, and the result. Put
     that file in the `Own` of the gate ticket.
3. Each ticket has the sections `## Goal`, `## Requirements` (the ids of the design that it
   makes green), `## Context`, `## Steps`, `## Constraints` and `## Done when`. Copy the
   10 build rules, word for word, into `## Constraints` of each ticket.
4. Write `MAP.md` in the format of contract C1, with `Spec: docs/specs/fleetd/design.md`.
   Under `## Open questions`, list each point where the design and the code of today do not
   agree and the ticket had to choose, with your choice.
5. Check coverage: each mechanism of the row of Phase 5 in "What each gate needs" and in "New work for
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
- Do not change code, `docs/specs/` or `ddd/`. Write only in `docs/tickets/fleetd-p5/`.
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
