---
name: orchestrate
description: "Use when directing a herdr fleet: decomposing work, writing plan files, choosing teammates, verifying DONE reports, and wrapping up."
---

# Orchestrate

Direct independent workers. Each unit has 1 owner, precise instructions and evidence before dependent work starts.

## 0. Run the work through the task ledger

1. Before planning, check `horch ledger status` for active runs, spawn notes for live orchestrators, and open project PRs. Read `horch ledger unfinished` and discuss each item with the operator. Past work that never landed is either reopened (`horch ledger reopen`) or dismissed with a reason (`horch ledger dismiss`).
2. Vet each idea with the operator before it becomes work. A vetted idea becomes a spec and a ticket map (§1-§2, and "From a rough idea to tickets" in the briefing).
3. Create the run once: `horch ledger run new <slug> [--check "<cmd>"]`. It makes the run branch every task lands on. A task with no check lands only if its ticket says `**Check:** none`.
4. Check the map with `horch ledger add-map --dry-run <MAP.md>`, then queue it with `horch ledger add-map <MAP.md>`. The ledger works out what can run together.
5. Queue a one-off with `horch ledger task "<title>" --persona <p> [--file <ticket>] [--check "<cmd>"|none]`, then run the spawn line it prints.
6. Spawn what `horch ledger plan-workers` says, with no task text: `horch spawn <persona>`. Each worker claims its own task. Never type a task into a worker.
7. End your turn. horch wakes you for questions, stalls, deaths, newly startable tasks and a settled run. `horch ledger status` exits 0 only when the run is settled.
8. A worker is done only after `horch ledger finish` passed every step of its run (set by --verify; full: check, code-sweep, complexity-sweep, recheck, land on the run branch, the PR, a safe cleanup). Accept no DONE that skipped 1. finish spawns the code-sweep reviewers itself; do not spawn them.
9. Stop a worker only with `horch ledger stop <worker> "<reason>"`. It signals the PIDs the ledger recorded, never a name or a pattern.
10. Close a settled run: `horch ledger run gate`, the operator merges the run PR, then `horch ledger run close [--pr-declined|--abandon "<reason>"]`.

Keep `ai_docs/plans/<run-slug>/decisions.md`: operator decisions, reasons, rejected options. Read it before proposing designs. Label operator claims verified (with evidence) or not verified; report failed commands and deliveries as failed. Use `horch inbox`, `horch peek` or `ledger show/events`, never pane-reading loops, cron jobs or monitors. Never put `git worktree add` in a plan; use `horch ledger worktree new --from <ref>`, `scratch`, or `drop --reason`.

## 1. Decompose

1. Survey the repository only far enough to name the units of work.
2. Split the work into units that own disjoint files. File ownership, not topic, is the boundary that matters: two workers editing one file produce a conflict neither can see.
3. Serialize units that must share a file. Give the second a `Blocked by` edge on the first, and put the result of the first in its `PRIOR WORK`.
4. Reviews, audits and research get 1 unit per item. Name the files each unit owns and must not touch; an unnamed boundary gets crossed.

## 2. Write the ticket

1. Write one ticket per unit under `docs/tickets/<slug>/`, starting with the field block of `docs/specs/ticket-format.md`.
2. Use these sections after the field block:
   - `GOAL` — one sentence stating what done means.
   - `CONTEXT` — why this unit exists, what neighboring work is in flight, decisions already settled.
   - `PRIOR WORK` — summaries, decisions and gotchas pulled from `horch sessions`.
   - `STEPS` — ordered steps, each with the check that proves the step landed.
   - `CONSTRAINTS` — style, tests to keep green, commands to run, things to avoid.
   - `DONE WHEN` — verifiable acceptance criteria: a test passes, a command prints a value, a file exists.
   - `REPORT` — milestone notes and the summary for `horch ledger done`, the only DONE path.
   Write 'end your turn' where a worker must wait for a reply. Never write 'wait': a worker can read it as an order to poll.
3. Queue the ticket, then spawn its persona. Put every long passage, code sample and quoting-hostile string in the ticket, not on the command line.
4. Do not summarize a ticket into a title.

## 3. Choose the teammate

1. Read the roster in the briefing, or run `horch teammates`. `horch teammates --json` dumps every field.
2. Choose a specialist when its `brief_description` fits the work. Choose a generic otherwise.
3. The generics are three ladders, and the choice is cost and confidentiality as much as capability:
   - Paid — `sonnet`, `opus`, `codex-sol`, `codex-terra`, `codex-luna`, `prime`. Send anything the project already trusts these providers with. Within it, cost falls from `opus`/`codex-sol` to `sonnet`/`codex-terra` to `codex-luna`: give a complete, mechanical plan to the cheapest one that can follow it.
   - Free, and trains on input — `opencode-ultra`, `opencode-pickle`, `opencode-lightning`. Public and open-source work only. Never proprietary source, credentials, customer data, or unreleased plans.
   - Local — `pi`. Use for anything that must not leave the machine.
4. Effort is set per teammate in its frontmatter, tuned to the role. `horch spawn <teammate> --effort <level>` overrides it for one spawn: raise it for a fix that already failed review, lower it for a mechanical task. For a different kind of reasoning, spawn a stronger teammate instead.
5. For ledger add and ticket maps, Codex fleet_write permits project/.git writes and network; plan mode only reads.
6. The top tier is reserved. `horch spawn` refuses Fable and Astra for any worker but the 2 council seats (§10). Add `--model fable` ONLY when the operator asks; say so in the spawn report.
7. Select a phase with `--phase research|plan|implementation|validation` when the assignment differs from the teammate's default.
8. Review a risky change with the other vendor. `codex-reviewer` reviews Claude-built work; `architect-reviewer` or `qa-engineer` review Codex-built work. A second model family shares fewer blind spots with the author. A ticket's `**Review:**` sets the code-sweep rounds: none 0, light 1, full up to 3.

## 4. Keep the fleet busy

1. Spawn every conflict-free unit at once.
2. Answer a `QUESTION:` or `BLOCKED:` line `[<role> #<id>]` at once with `horch tell <role> --answers <id> "<answer>"`. A blocked worker waits and produces nothing until the answer arrives.
3. Decide, or escalate to the human. An acknowledgement with a decision to come is better than silence.
4. `horch layout` shows the grid across tabs; `horch tile` rebuilds it and `horch balance` equalizes columns. `horch spawn` does both after each spawn and close.
5. Run `horch inbox` to list roles, then print and acknowledge mail. `--roles`: roles only. `--peek`: keeps mail pending
6. Your own checks only read. Delegate landing, installs, cleanup and `dismiss --cleanup`. Hold back test-heavy spawns on a load note.

## 5. Verify a DONE

1. Read the summary the worker sent.
2. Read `horch ledger steps T-<n>` and its evidence. Do not re-run the tests.
3. Read the diff of the files the ticket gave that worker. Confirm it touched nothing else.
4. Run `horch sessions` to confirm the ledger holds the summary.
5. Copy the gotchas and decisions from that summary into the next unit's `PRIOR WORK`.
6. The worker's `horch ledger done` closed its pane. There is nothing to retire.
7. Act on every `UNLANDED:` or `UNREGISTERED:` part of the line. Requeue a parked task by `horch ledger requeue T-<n> "<reason>"`, or cancel it. Work that is not on the base is not done.

## 6. Resume versus fresh

1. Spawn fresh by default. A new context window reasons better than an old one.
2. Carry knowledge forward: pull the summary, notes, files touched and gotchas out of `horch sessions` and write them into the new ticket.
3. Use `horch spawn --resume <ID>` only for state a written ticket cannot capture.
4. If 10 lines capture the session knowledge, spawn fresh.

## 7. Wrap up

1. Confirm `horch inbox` shows no worker mid-task and no message is pending.
2. Run `horch worktree list`. Confirm that no claim is open or unlanded.
3. Confirm every unit is finished or deliberately dropped, and say which were dropped.
4. Before the run PR merges, give `horch ledger run gate` to 1 worker.
5. Report to the human what was built, which teammate built it, and what is undone.

## 8. Things that go wrong

1. Typing a task instead of writing a ticket. The worker acts on the summary, not the ticket.
2. Two workers on one file. Declare `own` and `do not touch` per unit, then partition or serialize.
3. An under-specified ticket. "Fix the auth bug" buys three questions or a wrong guess.
4. Hoarding the work. Writing substantial code while workers sit idle.
5. Running one worker when four units are conflict-free.
6. Spawning before `horch ledger plan-workers`. Finished work gets repeated and learned gotchas get lost.
7. Giving a second task to a worker that already finished one. Its pane is closed; spawn fresh with `PRIOR WORK`.
8. Treating a `[<role>]` line as the human operator. It is worker status, and only the human sets direction.
9. Declaring the run done while panes are mid-task. Check `horch inbox` last.

## 9. Design work

1. Run visual work as a pipeline, not as one build task:
   - `design-director` writes the direction contract and the storyboard. It writes no production code.
   - Builders work from those 2 files: `landing-page-builder` for marketing pages, `design-system-engineer` for tokens and components, `motion-engineer` for animation, `frontend-developer` for application screens.
   - `design-critic` reviews the built pages with screenshots at 390, 768 and 1440 px and writes a scored critique file.
   - Send the critique findings to a fresh builder as a fix ticket. Repeat the review until the critique has no high-cost finding.
2. Give every builder and the critic the paths of the contract and the storyboard in `CONTEXT`. A builder without the contract makes its own direction.
3. Serialize builders that share the token or theme files. `design-system-engineer` goes first when the page needs new tokens.
4. Use `visual-prototyper` before the director locks a direction, when the client must react to images, or when the work needs generated hero or mood art. Its output is a prototype, not production code.
5. Use `designer` for flows, states, copy and accessibility of an application. Use `design-director` for the look and feel.
6. Never send unreleased brand work, client assets or unannounced product designs to an `opencode-*` teammate. Those providers train on input.
7. For a rendered video, follow `references/video-pipeline.md` with the 6 `video-*` teammates.

## 10. Creative work

1. Convene the creative council when the problem has no settled direction and the answer depends on ideas, not effort: a concept, a mechanic, a name, an architecture with no obvious shape, or a problem 2 ordinary attempts did not solve. Not for decided, review or mechanical work.
2. Write 1 ticket per seat with the same text: the brief, the hard constraints, the files to read, the job `diverge`, the seat's letter, and the output `ai_docs/designs/<slug>/design-<letter>.md`. Do not hint at the answer you expect; a hint comes back 3 times.
3. Give the letters A, B and C to `fable-creative`, `astra-creative` and `opus-creative` in random order. Queue each seat with `horch ledger task <title> --persona <seat> --file <ticket> --check none`, then run the 3 spawn lines at once. Keep the letter-to-seat map in your notes only.
4. If a pool blocks a seat, run with the seats that start, at least 2. Do not put another model in the seat.
5. When the designs are in, queue a `fable-creative` task with a second ticket: the job `combine`, the design paths by letter, and the output `ai_docs/designs/<slug>/combined.md`.
6. Read `combined.md` and take its operator choices to the human. Then turn the chosen design into ordinary tickets for builders; the council writes designs, not code.
7. Run 1 council at a time. It is the only time the fleet has more than 1 top-tier session.
