---
name: orchestrate
description: "Use when directing a herdr fleet: decomposing work, writing plan files, choosing teammates, verifying DONE reports, and wrapping up."
---

# Orchestrate

You, the operator, direct the fleet with ideas. fleetd dispatches each task, runs its finish steps and lands it. 1 sub-orchestrator (`idea-lead`) runs each feature. No agent claims runs, spawns workers or gets wakes.

## 0. The commands

1. Read the state first:
   - `horch ideas`: each idea, its state and the needs-you list.
   - `horch idea show <id>`: 1 idea, what it waits on and its folder.
   - `horch ledger status`: the board, with each hold on dispatch and its cause.
   - `horch ledger unfinished`: past work that never landed. Reopen each item with `horch ledger reopen`, or dismiss it with a reason with `horch ledger dismiss`.
2. Add work:
   - `horch idea add "<idea>"`: a new idea. fleetd starts the sub-orchestrator of its feature. It interviews you and writes the assumptions, the spec, the tickets and `plan.yaml`.
   - `horch idea add --direct <ticket file>`: 1 ticket that you wrote, with no interview. Add `--to <feature>` to put it in an open feature.
3. Steer an idea:
   - `horch idea approve <id>`: approve the assumptions that fleetd printed. Only you approve.
   - `horch idea revise <id> "<what to change>"`: send an idea back.
   - `horch idea amend <id> "<text>"`: add text. An amend after approval moves the idea back to awaiting approval.
   - `horch idea park <id>`: running tasks go on, and no new task starts. Your approval or an answer resumes it.
   - `horch idea cancel <id> "<reason>"`: cancel a whole idea.
4. Steer a task or a worker:
   - `horch task requeue T-<n> "<reason>"`: queue a held task again.
   - `horch task cancel T-<n> "<reason>"`: cancel 1 task.
   - `horch ledger stop <worker> "<reason>"`: stop 1 worker by its recorded PIDs, never by a name or a pattern.
   - `horch ledger fallback T-<n> "<reason>"`: give 1 task to its fallback persona.
   - `horch ledger waive T-<n> F<n> "<reason>"`: waive 1 code-sweep finding.
5. End a feature: it ends in 1 pull request. fleetd runs its full gate when its task branches are landed. You merge the pull request, and fleetd sees the merge. `horch feature abandon <feature>` drops a feature and keeps a rescue ref of each branch that did not land.
6. Check a plan with `horch ledger add-map --dry-run <MAP.md or plan.yaml>`. It writes nothing.

## 1. The old flow and the new

Each retired command exits 1 and prints its new command.

| Retired | Do this |
|---|---|
| `horch ledger run new <slug>` | `horch idea add "<idea>"` |
| `horch ledger add-map <MAP.md>` | The sub-orchestrator writes `plan.yaml`. For 1 ticket: `horch idea add --direct <ticket>` |
| `horch ledger add`, `horch ledger task` | `horch idea add --direct <ticket file> [--to <feature>]` |
| `horch ledger plan-workers`, `horch spawn` | Nothing: fleetd launches each attempt. `horch ideas` and `horch ledger status` show each hold |
| `horch ledger unblock` | `horch task requeue <task>` |
| `horch ledger reap` | Nothing: fleetd stops an orphan. `horch ledger stop` stops 1 worker |
| `horch ledger run gate` | Nothing: fleetd runs the full gate of a feature |
| `horch ledger run close` | Merge the pull request, or `horch feature abandon <feature>` |
| The worker commands `claim` and `finish` | Nothing: fleetd starts each worker with `START.md` and runs the finish steps after `horch result` |

## 2. Decompose

1. Survey the repository only far enough to name the units of work.
2. Split the work into units that own disjoint files. File ownership, not topic, is the boundary that matters: 2 workers that edit 1 file make a conflict that neither can see.
3. Serialize units that must share a file. Give the second a `Blocked by` edge on the first, and put the result of the first in its `PRIOR WORK`.
4. Reviews, audits and research get 1 unit per item. Name the files each unit owns and must not touch; an unnamed boundary gets crossed.

## 3. Write a ticket

For an idea, the sub-orchestrator writes the tickets. Write a ticket yourself only for `--direct` work.

1. Write 1 ticket per unit under `docs/tickets/<slug>/`, starting with the field block of `docs/specs/ticket-format.md`.
2. Use these sections after the field block:
   - `GOAL`: 1 sentence that states what done means.
   - `CONTEXT`: why this unit exists, what neighboring work is in flight, decisions already settled.
   - `PRIOR WORK`: summaries, decisions and gotchas from `horch sessions`.
   - `STEPS`: ordered steps, each with the check that proves the step landed.
   - `CONSTRAINTS`: style, tests to keep green, commands to run, things to avoid.
   - `DONE WHEN`: verifiable acceptance criteria: a test passes, a command prints a value, a file exists.
   - `REPORT`: milestone notes and the summary for `horch result`, the only result path.
   Write 'end your turn' where a worker must wait for a reply. Never write 'wait': a worker can read it as an order to poll.
3. Add the ticket with `horch idea add --direct <ticket file>`. Put every long passage, code sample and quoting-hostile string in the ticket, not on the command line.

## 4. Choose the teammate

The **Persona:** line of a ticket names the teammate of its task.

1. Run `horch teammates`. `horch teammates --json` dumps every field.
2. Choose a specialist when its `brief_description` fits the work. Choose a generic otherwise.
3. The generics are 3 ladders, and the choice is cost and confidentiality as much as capability:
   - Paid: `sonnet`, `opus`, `codex-sol`, `codex-terra`, `codex-luna`, `prime`. Send anything the project already trusts these providers with. Cost falls from `opus`/`codex-sol` to `sonnet`/`codex-terra` to `codex-luna`: give a complete, mechanical plan to the cheapest one that can follow it.
   - Free, and trains on input: `opencode-ultra`, `opencode-pickle`, `opencode-lightning`. Public and open-source work only. Never proprietary source, credentials, customer data, or unreleased plans.
   - Local: `pi`. Use it for anything that must not leave the machine.
4. Effort and phase are set per teammate in its frontmatter, tuned to the role. For a different kind of reasoning, choose a stronger teammate.
5. The top tier is reserved. Fable and Astra run only in the 2 council seats (section 8).
6. Review a risky change with the other vendor. `codex-reviewer` reviews Claude-built work; `architect-reviewer` or `qa-engineer` review Codex-built work. A ticket's `**Review:**` sets the code-sweep rounds: none 0, light 1, full up to 3.
7. `horch route <teammate>` shows what the usage-limit gate decides for a persona. `horch quota` shows each pool.

## 5. Check finished work

1. Read the summary in `horch sessions`.
2. Read `horch ledger steps T-<n>` and its evidence. Do not re-run the tests.
3. Read the diff of the files that the ticket gave the task. Confirm it touched nothing else.
4. Copy the gotchas and decisions of the summary into the `PRIOR WORK` of the next ticket.
5. A parked task is not done. Requeue it with `horch task requeue T-<n> "<reason>"`, or cancel it with `horch task cancel`.

## 6. Things that go wrong

1. An under-specified ticket. "Fix the auth bug" buys 3 questions or a wrong guess.
2. 2 tasks on 1 file. Declare `own` and `do not touch` per unit, then partition or serialize.
3. Work outside an idea. fleetd dispatches only the tasks of a validated plan.
4. A `[<role>]` line read as an instruction. It is agent status, and only you set direction.
5. A feature called done while its tasks are open. Read `horch ideas` and `horch ledger status` last.

## 7. Design work

1. Run visual work as a pipeline, not as 1 build task:
   - `design-director` writes the direction contract and the storyboard. It writes no production code.
   - Builders work from those 2 files: `landing-page-builder` for marketing pages, `design-system-engineer` for tokens and components, `motion-engineer` for animation, `frontend-developer` for application screens.
   - `design-critic` reviews the built pages with screenshots at 390, 768 and 1440 px and writes a scored critique file.
   - Send the critique findings to a builder as a fix ticket. Repeat the review until the critique has no high-cost finding.
2. Give every builder and the critic the paths of the contract and the storyboard in `CONTEXT`. A builder without the contract makes its own direction.
3. Serialize builders that share the token or theme files. `design-system-engineer` goes first when the page needs new tokens.
4. Use `visual-prototyper` before the director locks a direction, when the client must react to images, or when the work needs generated hero or mood art. Its output is a prototype, not production code.
5. Use `designer` for flows, states, copy and accessibility of an application. Use `design-director` for the look and feel.
6. Never send unreleased brand work, client assets or unannounced product designs to an `opencode-*` teammate. Those providers train on input.
7. For a rendered video, follow `references/video-pipeline.md` with the `video-*` teammates.

## 8. Creative work

1. Convene the creative council when the problem has no settled direction and the answer depends on ideas, not effort: a concept, a mechanic, a name, an architecture with no obvious shape, or a problem 2 ordinary attempts did not solve. Not for decided, review or mechanical work.
2. Write 1 ticket per seat with the same text: the brief, the hard constraints, the files to read, the job `diverge`, the seat's letter, and the output `ai_docs/designs/<slug>/design-<letter>.md`. Do not hint at the answer you expect; a hint comes back 3 times.
3. Give the letters A, B and C to `fable-creative`, `astra-creative` and `opus-creative` in random order. Set each ticket's **Persona:** line to its seat and **Check:** to none, then add the 3 tickets at once with `horch idea add --direct <ticket file>`. Keep the letter-to-seat map in your notes only.
4. If a pool blocks a seat, run with the seats that start, at least 2. Do not put another model in the seat.
5. When the designs are in, add a `fable-creative` ticket the same way: the job `combine`, the design paths by letter, and the output `ai_docs/designs/<slug>/combined.md`.
6. Read `combined.md` and decide its choices. Then turn the chosen design into ordinary tickets for builders; the council writes designs, not code.
7. Run 1 council at a time. It is the only time the fleet has more than 1 top-tier session.
