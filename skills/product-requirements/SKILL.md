---
name: product-requirements
description: Use when a goal, feature request or idea must become a scoped requirement set before anyone designs or builds it - problem statement, users and their current workaround, must/should/not-this-time scope, acceptance criteria as observable behaviour, the first useful slice, success signals and open decisions.
---

# Product requirements

Turn a request into requirements that a planner can design against and a tester can check. You decide what is worth building and in what order. You never decide how it is built: no architecture, no libraries, no data models, no file paths.

## When to use

- The task is a goal or a feature request, and nobody has written down what "done" means.
- Two requests conflict, or the scope keeps growing, and someone must pick.
- A plan or a build exists, and you must check it against what the user needs.
- Not for: implementation plans (`create-plan`), design options (`brainstorm`), visual direction (the design teammates).

## Inputs

- The task text, and any issue, ticket, conversation or document it links.
- The product as it is today. Read the README, the user-facing docs, the CLI help or the UI copy, and the code paths that the request touches. You read code to learn current behaviour, not to design the change.
- Earlier requirement files for the same area, if any (search `ai_docs/` and `docs/`).

## Workflow

1. **Restate the request in one sentence.** Then write the problem under it, in the user's terms, with no solution words. Check: a reader who knows only the product can say who is hurt and how.

2. **Separate the goal from the proposed solution.** The request often names a feature ("add a CSV export"). Ask what the user does with the result ("send numbers to finance every month"). Write both. If the feature does not reach the goal, or a smaller change reaches it, say so. Check: the goal sentence has no UI, API or technology word in it.

3. **Find the users and the current workaround.** Name each kind of user the change touches, and what each does today instead. A workaround that works well is evidence that the problem is small. A workaround that is slow, error-prone or impossible is evidence that the problem is real. Check: each user type has a "today they..." line, from the code, the docs or the task. Mark guesses as `[assumed]`.

4. **Get the facts you cannot find.** You cannot interview users. For a fact that changes scope (who the users are, a deadline, a hard constraint), send one `QUESTION:` to the orchestrator with the options and your recommended default. Continue with the default and mark it `[assumed, asked]`. Do not stop for a fact that only changes wording.

5. **Write the scope as three lists.** Each item has a one-line reason.
   - **Must have:** without it the change does not solve the problem. Keep this list short. If it has more than about seven items, the problem is too big for one piece of work; split it (step 7).
   - **Should have:** clear value, but the first release is still useful without it.
   - **Not this time:** things that someone will ask for, that you deliberately exclude. Name them, so that scope does not grow quietly during the build.
   Check: each request in the task is in exactly one list.

6. **Write acceptance criteria as observable behaviour.** One criterion per behaviour, each with an ID (`AC-1`, `AC-2`), in this form:

   ```
   AC-3  Given <state the user can set up>,
         when <action the user takes>,
         then <result the user can see, measure or read>.
   ```

   Rules:
   - The "then" names something a tester can check from outside: output text, a file, an HTTP status, a screen state, a number. Not "the cache is used" or "the code is clean".
   - Cover the failure paths, not only the happy path: bad input, missing permission, empty state, the limit reached, the dependency down.
   - State numbers. "Fast" becomes "the first page shows in under 2 s on the sample data set". If you do not know the number, write `[number needed]` and put it in open decisions.
   - Every must-have item has at least one criterion. A criterion with no must-have or should-have item behind it is scope creep; remove it.
   Check: a tester can write a test for each criterion without asking you a question.

7. **Cut the first slice.** Find the smallest subset of the must-haves that a real user can use on its own and get value from. It is end to end and thin, not one layer done fully. Name what the slice leaves out and why the user can still benefit. If the work is large, list the next slices in order, with one line on what each adds. Check: the first slice ships something that a user would notice if you removed it.

8. **Settle conflicts.** When two requirements conflict (speed and completeness, two user groups, the request and the existing behaviour), state the conflict in one line, recommend one side, and give the reason. Do not defer both. If the choice is the operator's to make (money, legal, brand, a promise to a customer), recommend, and send the conflict to the orchestrator as a `QUESTION:`.

9. **Define the success signals.** One to three signals that show, after release, that the problem got smaller: a count, a time, an error rate, a support-ticket topic, a manual step that disappears. For each, say where the number comes from today. If nothing measures it today, say so; do not invent a metric. Check: each signal can go up or down because of this change.

10. **List the open decisions and risks.** Each open decision has an owner (the orchestrator, the operator, a named teammate) and your recommended answer. Each risk says what happens to the user if it comes true.

11. **Write the file and report.** Write the requirement set to the path in your task, or to `ai_docs/requirements/YYYY-MM-DD-<topic>.md`. Then send `DONE:` with the path, the number of must-haves and criteria, the first slice in one line, and the open decisions that block planning.

## Output template

```markdown
# <Topic>: requirements

## Problem
<One paragraph, user's terms, no solution words.>

## Goal and proposed solution
- Goal: <what the user needs to be true>
- Proposed: <what was asked for>
- Fit: <does the proposal reach the goal; a smaller alternative, if any>

## Users and today's workaround
| User | Today they... | Pain |
|---|---|---|

## Scope
### Must have
- M1 <item> - <reason>
### Should have
- S1 <item> - <reason>
### Not this time
- N1 <item> - <reason>

## Acceptance criteria
- AC-1 (M1) Given ..., when ..., then ...

## First slice
<What it contains, what it leaves out, why it is still useful.>
Next slices: 2. ... 3. ...

## Success signals
| Signal | Source today | Expected direction |
|---|---|---|

## Conflicts and decisions
| Conflict or decision | Recommendation | Owner | Status |
|---|---|---|---|

## Risks
- <risk> - <effect on the user>

## Assumptions
- [assumed] ...
```

## Pushing back

- "Everything, eventually" is not a priority order. Rank it.
- A request phrased as a solution gets one step 2 pass, even when the requester is senior. Say so politely and specifically.
- "Users want X" with no user, no workaround and no evidence is a hypothesis. Mark it `[assumed]` and make the first slice the cheapest test of it.
- A must-have list that grows during the work goes back through step 5. New items go to "should have" unless they meet the must-have test.
- Do not pad. A small fix needs a problem line, two or three criteria and no slicing. Match the length to the change.

## Review checklist

- [ ] The problem paragraph has no solution words.
- [ ] Every request in the task is in exactly one scope list, with a reason.
- [ ] Every must-have has at least one acceptance criterion; every criterion traces to a scope item.
- [ ] Each criterion is observable from outside, with numbers, and covers failure paths where they exist.
- [ ] The first slice is end to end and useful by itself.
- [ ] Conflicts have a recommendation, not a deferral.
- [ ] Guesses are marked `[assumed]`; scope-changing questions went to the orchestrator.
- [ ] The file contains no architecture, technology choice or file path for the build.
