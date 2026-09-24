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
6. The top tier is reserved. `horch spawn` refuses Fable and Astra for every worker, whichever flavor orchestrates; a fleet has at most one top-tier session.
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
