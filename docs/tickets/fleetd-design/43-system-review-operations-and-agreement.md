# 43: Systems review of the fleetd design: operations, trust, and the agreement of the documents

<!-- horch-ticket: 1 -->
**Persona:** primary `codex-reviewer`, fallback `architect-reviewer` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/system-review-43-operations-and-agreement.md && grep -q '^## Verdict' docs/specs/fleetd/system-review-43-operations-and-agreement.md && grep -q '^## Answers' docs/specs/fleetd/system-review-43-operations-and-agreement.md && grep -q '^## Findings' docs/specs/fleetd/system-review-43-operations-and-agreement.md && grep -q '^## Checked and sound' docs/specs/fleetd/system-review-43-operations-and-agreement.md && ! grep -q '—' docs/specs/fleetd/system-review-43-operations-and-agreement.md`
**Own:** `docs/specs/fleetd/system-review-43-operations-and-agreement.md`
**Do not touch:** `docs/specs/fleetd/design.md`, `ddd/`, `crates/`

## GOAL

The file `docs/specs/fleetd/system-review-43-operations-and-agreement.md` answers each question of this ticket about the fleetd design with a line of the design, and ranks each real problem that the answers show.

## CONTEXT

- `docs/specs/fleetd/design.md` is the design of a daemon named `fleetd`. It replaces the control plane of a fleet of AI coding agents (`horch`). It has about 1,300 lines. No code exists for it.
- The operator asked for 1 review of the whole design: is it coherent, and is it good systems design and systems engineering?
- 5 reviewers work at the same time. Each has 1 lens and 1 report file. Your lens is: operations, trust, and the agreement of the documents.
- The sections that matter most for your lens:
  - Summary
  - Goals, non-goals and defaults
  - Workspace layout and file protocol
  - Worker profiles and adapters
  - Docs sync and adapter conformance
  - Requirements (SR and OR groups)
  Read the whole design 1 time first. A fault often sits between 2 sections.
- Earlier reviews had a small scope on purpose (`design-review.md`, `design-review-2.md`, `design-review-3.md`, `design-review-4.md` in the same directory). No review looked at the whole design with this lens. Do not repeat a finding that an earlier review closed.
- The decisions behind the design are in `docs/specs/fleetd/amendments.md`. The Amendments table at the top of the design names each decision (D1 to D16).
- Decisions of the operator are final: D1 to D4, D6 to D14 and D16. A problem that is caused by 1 of these decisions gets the rank LOW and the word "decision". Do not propose another design for it.
- Decision D5 and the details that the orchestrator settled (the Amendments table says "orchestrator") can be challenged.
- The current system is described in `docs/architecture.md` and in the 3 files `docs/specs/fleetd/preservation-0*.md`. The source code is in `crates/`. Read it when a question needs a fact of today.
- You are a fresh session on purpose. You are not an author of the design.

## QUESTIONS

1. Trust: which files does a worker write that fleetd reads (`result.json`, question files, progress files, the outbox)? For each: what can a wrong or hostile worker make fleetd do? Is each file checked against a schema, a size limit and the path of its own attempt?
2. The verifier runs the acceptance commands of a task in the sandbox profile of the worker. Who writes those commands, and can a worker change them after the plan is approved?
3. Credentials: does the design keep the rule that no `claude` process gets `ANTHROPIC_API_KEY`, in each launch path (fleetd, the wrapper, the verifier, the judge, docs sync, the conformance launches)? Name each launch path that does not go through the environment scrub. Do not print the value of an environment variable.
4. With no cap on parallel workers (D8): what bounds disk, memory, open files, git lock contention on 1 repository, and the SQLite write rate? Does each bound pause dispatch, and does `horch status` show why?
5. Observability: can the operator answer these from `horch status` and the ledger alone: why is this task not running, who owns this branch, what does the fleet wait for from me, what did fleetd do in the last hour and why?
6. The needs-you list: what puts an item on it, what takes it off, and what stops it from growing without a limit?
7. Agreement inside the design: find sentences in 2 sections that contradict each other on a rule, a number, a state name, a file name or a path. The design had 4 revision rounds, so look at the places that the Amendments table names.
8. Agreement between the design and the DDD model. These rules are in `ddd/` (see `ddd/COVERAGE.md`, section "Decided in round 2", and the step JSON files) and a search did not find them in the design: an idempotency key on each request file; `reset_at` is optional; a worker question escalates after 15 min; supervision is its own operating phase; task-execution and integration are a partnership; 1 needs-you list that ideas owns; the fleet-run wrapper belongs to worker-profiles; plan validation is a domain-model module; docs sync is a `horch` subcommand; a drift becomes a `--direct` idea (the design says only "an idea"). For each: say if the design states it, is silent, or contradicts it. Then name each other rule that is in 1 of the 2 only.
9. Simplicity: name each mechanism of the design that another mechanism already covers, and each one that the design could drop with no failure mode left open. Give at most 3.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read `docs/specs/fleetd/design.md` from its first line to its last.
3. Answer each question. For each answer, give the line or lines of the design. When the design is silent, say "silent". Do not guess what the authors meant.
4. For each answer that shows a problem, write a failure scenario: the concrete order of events that goes wrong, and what the operator then sees or loses. An answer with no failure scenario is not a finding.
5. Rank each finding. CRITICAL: the system as designed loses work, corrupts the ledger, stops for good, or cannot be built as written. HIGH: a builder must guess a rule, and a wrong guess causes a failure of the CRITICAL class. MEDIUM: a real gap with a small effect. LOW: a remark, or a problem of an operator decision.
6. Look 1 time for a problem of your lens that no question asks about. Add it as a finding when it has a failure scenario.
7. Write `docs/specs/fleetd/system-review-43-operations-and-agreement.md` with these sections:
   - `## Verdict`: 1 line, `SOUND` (no CRITICAL and no HIGH), `SOUND WITH GAPS` (no CRITICAL, 1 HIGH or more) or `NOT SOUND` (1 CRITICAL or more). Then 2 or 3 sentences: the state of the design under your lens.
   - `## Answers`: 1 entry for each question: the number, the answer in 1 to 4 sentences, the lines.
   - `## Findings`: each one with an id (`S43-1`, `S43-2`), its rank, the lines, a quote of at most 2 lines, the failure scenario, and the smallest change of the design that closes it. Or the word "None".
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
