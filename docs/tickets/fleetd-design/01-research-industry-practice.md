# 01: Research: industry practice for many concurrent changes to the same files

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/research-01-industry-practice.md`
**Own:** `docs/specs/fleetd/research-01-industry-practice.md`

## GOAL

The file `docs/specs/fleetd/research-01-industry-practice.md` holds sourced facts about how large engineering organizations land many concurrent changes to the same files.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/research-context.md` first. It gives the system, the operator's position, the options A to E and the rules for the findings file.
- Ticket 02 researches merge tools and AI agent systems. Do not research them here.
- Ticket 03 measures this repository. Do not measure it here.
- The orchestrator uses your file to make 1 design decision. A wrong fact causes a wrong design.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file named above.
3. Research question Q1, merge queues. Cover GitHub merge queue, GitLab merge trains, Bors and Homu, Zuul, Uber SubmitQueue, Google TAP, Shopify merge queue, Mergify and Aviator. For each one that has a primary source, find: how it groups changes into a batch, which tests run for 1 change and which run for the batch, and what it does when a batch fails.
4. Research question Q2, test tiers. Find which tests gate 1 change before it lands and which tests run after, on a batch or a release. Find how affected-test selection decides the set of tests. Find a published rule for when E2E tests run.
5. Research question Q3, stacked changes. For Gerrit, Phabricator, Graphite, Sapling and Jujutsu (`jj`), find how each one updates dependent changes when their base changes. Find the documented limits.
6. Research question Q4, less contention on 1 file. Find sourced descriptions of trunk-based development, branch by abstraction and feature flags as methods that let many changes to 1 file land. Find what each method needs from the code structure.
7. Research question Q5, numbers. Find published measurements of merge conflict rate against branch lifetime, change size or the number of concurrent changes. Give the number, the population and the year.
8. Write the findings file. Number the findings F1, F2 and so on. Follow the rules in the context file.
9. Run `test -s docs/specs/fleetd/research-01-industry-practice.md`. It must exit 0.

## CONSTRAINTS

- Use primary sources first: engineering blogs of the company, official documentation, papers. Use a secondary source only when no primary source exists, and say so.
- Do not edit a file other than the one in **Own**.
- Do not recommend an option. The orchestrator decides.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of Q1 to Q5 has at least 1 finding, or the sentence "I could not determine <X>".
- Each finding has evidence and a `verified` or `not verified` label.
- The file starts with `## Summary` and ends with `## What the facts mean for options A to E`.

## REPORT

Record 1 progress note after each question. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the 3 facts that bear most on the decision.
