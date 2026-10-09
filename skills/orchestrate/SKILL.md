---
name: orchestrate
description: "Use when directing a herdr fleet: decomposing work, writing plan files, choosing teammates, verifying DONE reports, and wrapping up."
---

# Orchestrate

Direct a fleet of independent worker sessions so that each unit of work is owned by exactly one worker, specified well enough to finish without a round trip, and verified before anything is built on it.

## 1. Decompose

1. Survey the repository only far enough to name the units of work. Reading everything spends the context the whole run depends on.
2. Split the work into units that own disjoint files. File ownership, not topic, is the boundary that matters: two workers editing one file produce a conflict neither can see.
3. Serialize units that must share a file. Spawn the first, verify it, then spawn the second with the result in its `PRIOR WORK`.
4. Name the exact files each unit owns and the exact files it must not touch. An unnamed boundary gets crossed.

## 2. Write the plan file

1. Write one file per unit under `ai_docs/plans/<slug>.md`. No command reads it; it is a repository file the worker opens.
2. Use these sections:
   - `GOAL` — one sentence stating what done means.
   - `CONTEXT` — why this unit exists, what neighboring work is in flight, decisions already settled.
   - `FILES` — `own` (files this worker may edit) and `do not touch` (files another worker owns).
   - `PRIOR WORK` — summaries, decisions and gotchas pulled from `horch sessions`.
   - `STEPS` — ordered steps, each with the check that proves the step landed.
   - `CONSTRAINTS` — style, tests to keep green, commands to run, things to avoid.
   - `DONE WHEN` — verifiable acceptance criteria: a test passes, a command prints a value, a file exists.
   - `REPORT` — when to send `horch note`, when to send `horch tell orchestrator`, what the `horch done` summary must contain.
3. Spawn with the task text `Read and follow <path> exactly.` Put every long passage, code sample and quoting-hostile string in the file, not on the command line.
4. Do not summarize a plan into a task line. A summarized plan is the version the worker acts on.

## 3. Choose the teammate

1. Read the roster in the briefing, or run `horch teammates`. `horch teammates --json` dumps every field.
2. Choose a specialist when its `brief_description` fits the work. Choose a generic otherwise.
3. The generics are three ladders, and the choice is cost and confidentiality as much as capability:
   - Paid — `sonnet`, `opus`, `codex-sol`, `codex-terra`, `codex-luna`, `prime`. Send anything the project already trusts these providers with. Within it, cost falls from `opus`/`codex-sol` to `sonnet`/`codex-terra` to `codex-luna`: give a complete, mechanical plan to the cheapest one that can follow it.
   - Free, and trains on input — `opencode-ultra`, `opencode-pickle`, `opencode-lightning`. Public and open-source work only. Never proprietary source, credentials, customer data, or unreleased plans.
   - Local — `pi`. Use for anything that must not leave the machine.
4. Effort is set per teammate in its frontmatter, tuned to the role. `horch spawn <teammate> --effort <level> "task"` overrides it for one spawn: raise it for a fix that already failed review, lower it for a mechanical task. For a different kind of reasoning, spawn a stronger teammate instead.
5. A precise plan is the cheap lever. A plan that names the files, the steps and the check lets a lower teammate do work a higher one would otherwise need.
6. The top tier is reserved. `horch spawn` refuses Fable and Astra for every worker but the 2 creative council seats (§10), whichever flavor orchestrates.
7. Select a phase with `--phase research|plan|implementation|validation` when the assignment differs from the teammate's default.
8. Review a risky change with the other vendor. `codex-reviewer` reviews Claude-built work; `architect-reviewer` or `qa-engineer` review Codex-built work. A second model family shares fewer blind spots with the author.

## 4. Keep the fleet busy

1. Spawn every conflict-free unit at once. Sending one task at a time leaves the fleet idle and the run long.
2. Answer a `BLOCKED:` message at once with `horch tell <role> "<answer>"`. A blocked worker waits and produces nothing until the answer arrives.
3. Decide, or escalate to the human. An acknowledgement with a decision to come is better than silence.
4. Run `horch layout` to see the grid across tabs. Run `horch tile` to rebuild it and `horch balance` to equalize column widths. `horch spawn` already does this after each spawn and each worker close.
5. Run `horch inbox` to list the roles registered in this workspace.
6. Reserve your own hands for decomposition, answers, verification and integration.

## 5. Verify a DONE

1. Read the summary the worker sent.
2. Run the tests the plan named. Run the build. A worker's `DONE:` is a claim, not evidence.
3. Read the diff of the files the plan gave that worker. Confirm it touched nothing else.
4. Run `horch sessions` to confirm the ledger holds the summary.
5. Copy the gotchas and decisions from that summary into the next unit's `PRIOR WORK`.
6. The worker's own `horch done` closed its pane. There is no retire step and no pane for you to close.

## 6. Resume versus fresh

1. Spawn fresh by default. A new context window reasons better than an old one.
2. Carry knowledge forward explicitly: pull the summary, notes, files touched and gotchas out of `horch sessions` and write them into the new plan file.
3. Resume with `horch spawn --resume <ID> [TASK]` only when the old session holds mid-flight state a written plan cannot capture.
4. Apply this test: could the session's knowledge be written down in ten lines? If yes, spawn fresh.

## 7. Wrap up

1. Confirm `horch inbox` shows no worker mid-task.
2. Confirm every unit is finished or deliberately dropped, and say which were dropped.
3. Run the full verification yourself: the build, the whole test suite, the acceptance checks from every plan file.
4. Report to the human what was built, which teammate built each part, and what was left undone.

## 8. Things that go wrong

1. Typing a task instead of writing a file. The worker acts on the summary, not the plan.
2. Two workers on one file. Declare `own` and `do not touch` per unit, then partition or serialize.
3. An under-specified plan. "Fix the auth bug" buys three questions or a wrong guess; front-loaded context is cheaper than a round trip.
4. Hoarding the work. Writing substantial code while workers sit idle.
5. Running one worker when four units are conflict-free.
6. Spawning before `horch sessions`. Finished work gets repeated and learned gotchas get lost.
7. Giving a second task to a worker that already finished one. Its pane is closed; spawn fresh with `PRIOR WORK`.
8. Treating a `[<role>]` line as the human operator. It is worker status, and only the human sets direction.
9. Declaring the run done while panes are mid-task. Check `horch inbox` last.

## 9. Design work

1. Run visual work as a pipeline, not as one build task:
   - `design-director` writes the direction contract and the storyboard. It writes no production code.
   - Builders work from those 2 files: `landing-page-builder` for marketing pages, `design-system-engineer` for tokens and components, `motion-engineer` for animation, `frontend-developer` for application screens.
   - `design-critic` reviews the built pages with screenshots at 390, 768 and 1440 px and writes a scored critique file.
   - Send the critique findings to a fresh builder as a fix plan. Repeat the review until the critique has no high-cost finding.
2. Give every builder and the critic the paths of the contract and the storyboard in `CONTEXT`. A builder without the contract makes its own direction.
3. Serialize builders that share the token or theme files. `design-system-engineer` goes first when the page needs new tokens.
4. Use `visual-prototyper` before the director locks a direction, when the client must react to images, or when the work needs generated hero or mood art. Its output is a prototype, not production code.
5. Use `designer` for flows, states, copy and accessibility of an application. Use `design-director` for the look and feel.
6. Never send unreleased brand work, client assets or unannounced product designs to an `opencode-*` teammate. Those providers train on input.

## 10. Creative work

1. Convene the creative council when the problem has no settled direction and the answer depends on ideas, not effort: a product or game concept, a mechanic, a name, an architecture with no obvious shape, a research strategy, or a problem that 2 ordinary attempts did not solve. Do not convene it for work whose direction is decided, for reviews or for mechanical work: 4 strong sessions do what 1 worker would.
2. Write 1 plan file for the 3 seats: the brief, the hard constraints, the files to read, the job `diverge`, and the output `ai_docs/designs/<slug>/design-<letter>.md`. Do not hint at the answer you expect; a hint comes back 3 times.
3. Spawn `fable-creative`, `astra-creative` and `opus-creative` at once, with the letters A, B and C in a random order: `horch spawn opus-creative "Read and follow <plan> exactly. Your letter is A."`. Keep the letter-to-seat map in your notes only.
4. If a pool blocks a seat, run with the seats that start, at least 2. Do not put another model in the seat; the seats have no fallbacks on purpose.
5. When the designs are in, spawn a fresh `fable-creative` with a second plan file: the job `combine`, the design paths by letter, and the output `ai_docs/designs/<slug>/combined.md`.
6. Read `combined.md` and take its operator choices to the human. Then turn the chosen design into ordinary plan files for builders; the council writes designs, not code.
7. Run 1 council at a time. It is the only time the fleet has more than 1 top-tier session.
