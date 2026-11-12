# 02: Research: merge tools, and how AI agent systems handle concurrent edits

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/research-02-merge-tools-and-agent-systems.md`
**Own:** `docs/specs/fleetd/research-02-merge-tools-and-agent-systems.md`

## GOAL

The file `docs/specs/fleetd/research-02-merge-tools-and-agent-systems.md` holds sourced facts about merge tools, about AI resolution of merge conflicts, and about how multi-agent coding systems handle edits to the same file.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/research-context.md` first. It gives the system, the operator's position, the options A to E and the rules for the findings file.
- Ticket 01 researches merge queues, test tiers and stacked changes in large organizations. Do not research them here.
- Ticket 03 measures this repository. Do not measure it here.
- This repository is Rust. A tool that does not support Rust has less value here. State the Rust support of each tool.
- The orchestrator uses your file to make 1 design decision. A wrong fact causes a wrong design.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file named above.
3. Research question Q1, git itself. Find what `git rerere`, the `ort` strategy, `diff3` and `zdiff3` conflict styles, and custom merge drivers (`.gitattributes` `merge=`) do. Find which same-file conflicts each one removes.
4. Research question Q2, structural merge tools. Cover Mergiraf, Spork, IntelliMerge, SemanticMerge, Difftastic and weave. For each: the languages it supports, Rust support, how it installs as a git merge driver, the classes of conflict it resolves, published accuracy, the license and the date of the last release.
5. Research question Q3, Jujutsu (`jj`). Find how `jj` stores a conflict in a commit, how it rebases descendants automatically, and how a later commit resolves the conflict. Find whether `jj` works on a normal git repository next to plain git commands.
6. Research question Q4, AI resolution of conflicts. Find papers and benchmarks on language-model resolution of merge conflicts, for example MergeBERT, DeepMerge, ConflictBench and newer work to 2026. Give the accuracy numbers, the conflict types that fail, and what is known about merges that are textually clean but wrong in behavior.
7. Research question Q5, multi-agent coding systems. Find the documented method of each system for 2 agents that change the same file. Cover Claude Code agent teams and worktrees, Cursor background agents, Codex cloud tasks, Devin, OpenHands, Aider, MetaGPT, ChatDev, Gas Town by Steve Yegge, Conductor, Vibe Kanban, claude-squad and Crystal. Classify each method as: file lock, 1 worktree per agent with merge at the end, a merge queue or merge agent, 1 writer per file, or not documented.
8. Research question Q6, combine at the file. Find prior art where a planner gives several change requests on 1 file to 1 writer as 1 edit. Find evidence for and against this pattern: quality, size limits, and what it does to traceability of each request.
9. Write the findings file. Number the findings F1, F2 and so on. Follow the rules in the context file.
10. Run `test -s docs/specs/fleetd/research-02-merge-tools-and-agent-systems.md`. It must exit 0.

## CONSTRAINTS

- Use primary sources first: official documentation, the source repository, papers. Use a secondary source only when no primary source exists, and say so.
- Do not install a tool on this machine. Read its documentation.
- Do not edit a file other than the one in **Own**.
- Do not recommend an option. The orchestrator decides.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of Q1 to Q6 has at least 1 finding, or the sentence "I could not determine <X>".
- Each finding has evidence and a `verified` or `not verified` label.
- The file starts with `## Summary` and ends with `## What the facts mean for options A to E`.

## REPORT

Record 1 progress note after each question. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the 3 facts that bear most on the decision.
