# Correct the Phase 4 and Phase 5 tickets: a gate runs in a sandbox, never with the rights of fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** docs
**Review:** light
**Check:** `horch ledger add-map --dry-run docs/tickets/fleetd-p4/MAP.md && horch ledger add-map --dry-run docs/tickets/fleetd-p5/MAP.md`
**Own:** `docs/tickets/fleetd-p4/28-gate-jobs.md`, `docs/tickets/fleetd-p4/MAP.md`, `docs/tickets/fleetd-p5/06-platform-sandbox-operator-denials.md`, `docs/tickets/fleetd-p5/43-p5-operator-boundary-e2e.md`, `docs/tickets/fleetd-p5/MAP.md`

## Goal

The tickets of Phases 4 and 5 say that each job that runs code of the project (the `fast_gate`
and `full_gate` jobs, and each other job that runs a project command) runs in a sandbox that
denies the operator path, and Phase 5 has an attack test for it. You change ticket text only. You
write no code.

## Context

- `docs/tickets/fleetd-p4/MAP.md`, Open questions, Q19 says: "How the gates run. Through the
  launcher, with no worker sandbox. Ticket 28." Ticket 28 repeats it.
- This is a hole. A gate runs the tests of the project, and workers write those tests. With no
  sandbox, that code runs with the rights of fleetd. From Phase 5 the rights of fleetd include
  the operator token `<fleet root>/run/operator.token`, the command channel and the request
  folder. So a worker can write a test that sends an operator request during a gate. This goes
  around SR-8.
- The design: SR-4 ("An acceptance command shall run in the sandbox profile of the task's
  worker, never with the rights of fleetd"), the row Verifier of "Architecture and roles", and
  SR-8. The design does not name the sandbox of a gate. The rule of the build is: code that a
  worker wrote never runs with the rights of fleetd.
- Decisions of the build that hold (`docs/tickets/fleetd-p4/MAP.md`, the orchestrator answer
  Q-A): the check sandbox comes from the sandbox concern of `crates/horch-platform` (Seatbelt on
  macOS, bubblewrap on Linux, NotSupported on Windows); NotSupported means no run and 1
  needs-you item, never a silent run with the rights of fleetd.
- The queued tasks of the ledger name these ticket files by path, and a worker reads its ticket
  from the run branch. So a correction of the file is enough. Do not add, remove or rename a
  ticket file, and do not change a title, a `Blocked by` cell or the count of tickets.

## Steps

1. Read ticket 28 of Phase 4, the check sandbox tickets of Phase 4 (17 and 18), and tickets 06,
   38 and 43 of Phase 5.
2. Ticket 28 of Phase 4: replace "no worker sandbox" with a gate sandbox. The gate command runs
   through the launcher in a sandbox of the platform sandbox concern: it can write only the gate
   tree of the feature, its temporary folder and the build cache paths that the project needs; it
   cannot write the fleet root, the ledger, another worktree or the repository of the user
   outside the gate tree. NotSupported: the gate does not run, the feature gets a hold, and 1
   needs-you item is opened. Add the tests to the ticket: a gate command that writes outside its
   paths is refused; NotSupported gives the hold and the item. Add the files that the ticket
   needs to its `Own` only if no parallel ticket of the same wave owns them; if one does, write
   the sandbox as a call of the function that ticket 18 gives, and say so.
3. Check each other Phase 4 job that can run a command or a hook of the project (the rebase job,
   the bundle build, the trial merge, a resolver). If it runs only git with hooks off
   (`core.hooksPath=/dev/null`), say that in 1 line of its ticket only if that ticket is in your
   Own; if not, list it in Open questions of the Phase 4 `MAP.md` for the orchestrator.
4. Phase 4 `MAP.md`: correct Q19, and add 1 line to the coverage table.
5. Ticket 06 of Phase 5: the 3 denials (read of the token file, connect to the channel, write
   into the request folder) apply to the gate sandbox and the check sandbox as they apply to a
   worker sandbox.
6. Ticket 43 of Phase 5: add the attack test: a test of the project that runs in a gate tries
   each of the 3 operator acts, and each one is refused and recorded.
7. Phase 5 `MAP.md`: add the line to Open questions and to the coverage table.
8. Run the Check. Commit.

## Constraints

- Simplified Technical English. Short sentences. Never write the em dash character; use `-`.
- Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- Commit messages have no co-author line.
- NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it.
- Change only the files of your Own. Change no code, no `docs/specs/` and no `ddd/`.
- Keep the 10 build rules in each ticket as they are.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, and no ticket of Phases 4 and 5 says that code of the project runs with no
sandbox.

## Report

In the summary of `horch ledger done`: each file and what changed, and each other job that you
listed in Open questions.
