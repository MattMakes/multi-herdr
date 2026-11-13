# 39: Systems review of the fleetd design: the control loop and crash safety

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/system-review-39-control-loop.md && grep -q '^## Verdict' docs/specs/fleetd/system-review-39-control-loop.md && grep -q '^## Answers' docs/specs/fleetd/system-review-39-control-loop.md && grep -q '^## Findings' docs/specs/fleetd/system-review-39-control-loop.md && grep -q '^## Checked and sound' docs/specs/fleetd/system-review-39-control-loop.md && ! grep -q '—' docs/specs/fleetd/system-review-39-control-loop.md`
**Own:** `docs/specs/fleetd/system-review-39-control-loop.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/system-review-39-control-loop.md` answers each question of this ticket about the fleetd design with a line of the design, and ranks each real problem that the answers show.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It replaces the control plane of a fleet of AI coding agents (`horch`). It has about 1,300 lines. No code exists for it.
- The operator asked for 1 review of the whole design: is it coherent, and is it good systems design and systems engineering?
- 5 reviewers work at the same time. Each has 1 lens and 1 report file. Your lens is: the control loop and crash safety.
- The sections that matter most for your lens:
  - Architecture and roles
  - Ledger
  - Supervision, liveness and reconciliation
  - Failure modes and effects
  - Requirements (RR and PL groups)
  Read the whole design 1 time first. A fault often sits between 2 sections.
- Earlier reviews had a small scope on purpose (`design-review.md`, `design-review-2.md`, `design-review-3.md`, `design-review-4.md` in the same directory). No review looked at the whole design with this lens. Do not repeat a finding that an earlier review closed.
- The decisions behind the design are in `docs/specs/fleetd/amendments.md`. The Amendments table at the top of the design names each decision (D1 to D16).
- Decisions of the operator are final: D1 to D4, D6 to D14 and D16. A problem that is caused by 1 of these decisions gets the rank LOW and the word "decision". Do not propose another design for it.
- Decision D5 and the details that the orchestrator settled (the Amendments table says "orchestrator") can be challenged.
- The current system is described in `docs/architecture.md` and in the 3 files `docs/specs/fleetd/preservation-0*.md`. The source code is in `crates/`. Read it when a question needs a fact of today.
- You are a fresh session on purpose. You are not an author of the design.

## QUESTIONS

1. PL-5 restarts fleetd when its last full pass is older than 30 s of awake time. Does a thing that takes long run inside 1 pass? Look at the verifier check (its timeout is in `preservation-05-contracts-and-values.md`, row V-10), the fast gate in `land_bundles`, the full gate, trial merges, git fetch and rescue. If a long thing runs inside a pass, a healthy fleetd is restarted in a loop.
2. The design says that a pass is 1 SQLite transaction. Which side effects occur in a pass (start a process, git worktree add, git merge, write a file)? For each: what happens when fleetd crashes after the side effect and before the commit, and after the commit and before the side effect? Is each one detected and repaired on the next pass?
3. 4 things can start or restart fleetd: the service manager, the 30 s check job, each `horch` command and `horch self-update`. Is a second fleetd stopped by a mechanism (a lock, a lease), or only by a convention? What happens to the ledger when 2 run?
4. The command channel: what happens to a request when fleetd is down, when fleetd crashes after it read a request file, and when a client sends the same request 2 times?
5. Is the 5 s pass level-triggered for each kind of fact (process, pane, file, git, lease)? Name each fact that fleetd learns only from an event.
6. Clock and sleep: does each timer (lease, stall ladder, question limit, due_at of a branch row, the hang check) use awake time? Name each one that does not say.
7. Back-pressure: there is no cap on parallel workers (D8). What limits the time of 1 pass when 200 tasks and 200 worktrees exist?

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read `docs/specs/fleetd/design.md` from its first line to its last.
3. Answer each question. For each answer, give the line or lines of the design. When the design is silent, say "silent". Do not guess what the authors meant.
4. For each answer that shows a problem, write a failure scenario: the concrete order of events that goes wrong, and what the operator then sees or loses. An answer with no failure scenario is not a finding.
5. Rank each finding. CRITICAL: the system as designed loses work, corrupts the ledger, stops for good, or cannot be built as written. HIGH: a builder must guess a rule, and a wrong guess causes a failure of the CRITICAL class. MEDIUM: a real gap with a small effect. LOW: a remark, or a problem of an operator decision.
6. Look 1 time for a problem of your lens that no question asks about. Add it as a finding when it has a failure scenario.
7. Write `docs/specs/fleetd/system-review-39-control-loop.md` with these sections:
   - `## Verdict`: 1 line, `SOUND` (no CRITICAL and no HIGH), `SOUND WITH GAPS` (no CRITICAL, 1 HIGH or more) or `NOT SOUND` (1 CRITICAL or more). Then 2 or 3 sentences: the state of the design under your lens.
   - `## Answers`: 1 entry for each question: the number, the answer in 1 to 4 sentences, the lines.
   - `## Findings`: each one with an id (`S39-1`, `S39-2`), its rank, the lines, a quote of at most 2 lines, the failure scenario, and the smallest change of the design that closes it. Or the word "None".
   - `## Checked and sound`: each mechanism that you checked and found correct, 1 line each, with the lines. This section must not be empty.
8. Run the **Check** command of this ticket. It must exit 0.
9. Commit the file.

## CONSTRAINTS

- Give at most 8 findings. Give the 8 that matter most. This is the guard against gold-plating: the operator wants a design that is good enough to build, not a perfect document.
- A finding is a failure that can occur. A preference for another design is not a finding.
- Do not rank a finding CRITICAL or HIGH without a failure scenario and a line of the design.
- Do not report a fault of spelling, style or layout.
- Do not edit a file other than the one in **Own**.
- Do not use the em dash. Use a plain dash.
- Never read, print or use `ANTHROPIC_API_KEY`.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each question has 1 entry in `## Answers`.
- Each finding has 1 rank, lines, a failure scenario and a smallest change.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the verdict, the count of findings for each rank, and each CRITICAL and HIGH finding in 1 sentence.
