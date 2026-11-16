---
name: comment-pruning
description: Run solely on an explicit request for comment pruning, whether aimed at listed files, at a diff, or (argument absent) at whatever the present branch altered relative to main. Strips comments which retell code, serve as ornament, park retired statements or apologise for hacks; then checks that pass severely, repairs the code such comments shielded, proposes swapping human "do not touch" notes for mechanical checks, and closes with a fixed-form report. Never self-start.
---

# Comment pruning

Code should explain itself. A comment that retells the code, draws a banner,
keeps old code alive or apologises for a workaround is removed. The reason
behind a removed comment does not become new prose: it becomes a code change
or an enforced check, or it is reported as open work.

Run this skill only when you were asked for it by name. Do not start it
because you happen to see noisy comments.

The run has 2 hats, and they stay apart:

- The **pruner** deletes comments and raises flags. It writes no application
  code. Its full rules are in `references/pruning-pass.md`.
- The **auditor** (you, the agent that was asked) distrusts the pruner's
  result, checks it, fixes the flagged code and writes the report. Its rules
  are in `references/audit-and-fixes.md`.

The auditor's strict check governs. Never accept the pruner's view just
because it is confident.

## 1. Settle the scope

- Files or a diff were named: those are the scope until the run ends.
- Nothing was named: the scope is everything the current branch changed
  compared with the base branch, plus work that is not committed yet. The
  base branch is `main`, unless the request names a different one.
  `prunecheck scope` (see `references/script-prunecheck.md`) lists the files.
- The scope turns out empty: say so, give the report with zero counts, stop.

Neither hat edits a file beyond the scope, at any step.

## 2. Hand the pruning pass to a second agent

This skill starts no subagent of its own. Send the orchestrator a message
that asks for 1 separate teammate to do the pruning pass. The message holds
only:

- the scope (file list or diff range),
- a pointer to `references/pruning-pass.md` of this skill,
- the size you suggest for that teammate.

Do not restate the pruning rules in the message. The reference file is the
single copy.

Sizing: match the teammate to the job. A handful of files with plain
narration needs a fast, light model at low effort. A large scope, or one full
of suppressions and "do not remove" warnings that need tracing, needs a
stronger model and more thinking time.

If no orchestrator exists, or it answers that no teammate is available, do
the pruning pass yourself as a closed phase: read `references/pruning-pass.md`,
follow it to the letter, write its report, and only then switch to the
auditor hat. Audit your own pass just as strictly.

Note the commit or stash state before the pass starts, so that you can undo
the pass cleanly.

## 3. What the pruning pass does (summary)

Details and examples: `references/pruning-pass.md`.

- Every comment in the scope goes, unless the keep list covers it.
- Keep list: legal and licence headers; a note about odd behaviour that a
  vendor, protocol, platform or third-party library imposes; formatter
  on/off/skip directives; suppressions of a rule that is broken, nitpicking
  or about style only; doc comments that state the contract of a public
  interface; a link to a ticket or design note for a constraint that code has
  no way to state.
- Prose that accounts for odd conduct of project-owned code gets removed;
  a reshape flag then lands on the precise symbol involved.
- For each suppression, the pruner looks up the rule. A rule that guards
  correctness or safety loses its suppression, and the offending symbol gets
  a fix flag.
- Loud comments ("critical", "never delete", "too dangerous to change",
  "later", long defences) are leads to investigate. They prove nothing.
- When in doubt, delete. Never swap a comment for a shorter one.
- The pruner reports 4 things and nothing else: the files it changed, how
  many comments it deleted, its flags (one line of reason each), and what it
  skipped.

## 4. Audit the pass

Read the pruner's report next to the real diff. Use `prunecheck check` for
the mechanical part and `prunecheck suppressions` to find directives that the
pruner overlooked. Then judge the rest yourself.

Reject the pass when any of these holds:

1. Application code changed (anything that is not a comment).
2. A file beyond the scope changed.
3. A comment under the keep list was deleted.
4. A flag gives a reason that is false.
5. A flag blames code that is intentional and whose explaining comment the
   pruner itself kept.

Also look for suppressions in the scope that the report does not mention.

Restoring: put a deleted comment back only when one keep-list case fits
exactly and the evidence sits inside the scope. A reshape flag (surprise in
project code) stays open, and so does a fix flag on a suppressed rule that
protects correctness or safety. Their comments stay deleted.

Thin evidence: when a deletion or a keep of a loud comment rests on little,
trace the symbol or read its history before you accept it. Outcomes:

| Case | Result |
| --- | --- |
| Deletion, still unclear after tracing | stays deleted |
| Keep, claim disproved | delete it |
| Keep, still unclear | delete it |
| Keep, proven to be about an external fact | stays |

Rejection handling:

- First rejection: undo all changes of the pass, then ask for the pass once
  more and name what went wrong.
- Second rejection: stop. The run has failed. Report the rejection reason as
  open work, with 1 rerun counted. There is never a second rerun.

## 5. Fix what the accepted flags name

Details: `references/audit-and-fixes.md`.

- Simple flags: fix them right away. Typical cases are dead branches that
  can go, parameters nobody reads, and a detour around an interface that can
  be replaced by the proper call.
- If one or more flags need structural thought: write 1 design sketch that
  covers all accepted flags together with the code around them. The sketch
  step ends with the sketch. No file changes during it. Implementation is the
  next, separate step.
- Implement the least change that cures the root cause within the scope, and
  remove each workaround that a flag named.
- Root cause lives beyond the scope: make the least fix that is possible
  inside the scope, leave the other code alone, and report the rest as open.
- Never add a guard whose only effect is to hide the symptom. Never grow the
  scope. Never fix sibling cases elsewhere.
- No approval is asked for these code fixes. They are listed in the report.
- After the fixes, run the linter and the type checker of the project on
  the scope when they exist. A failure that remains is open work in the report.

## 6. Constraint comments written by people

A constraint comment forbids deleting or rewording something, or demands
that a named person is asked before a change.

- It is about an external fact that the project has no power over: leave it.
- Otherwise: work out the cheapest mechanical enforcement that fits in the
  scope. Candidates are a test, a type, a lint rule wired into CI, or a
  runtime assertion. Send the offer to the orchestrator as a message and wait
  for the answer. There is no direct prompt to a human in this skill.
- Approved: add the enforcement first. Delete the comment after it is in.
- Declined, or no approval: add nothing. Delete the comment, list the
  constraint as unenforced and open, and sketch the work beyond the scope
  that would enforce it.
- Unattended and evaluation runs: an approval counts only when the caller
  gave it up front. Without one, treat the offer as not approved and carry
  on. Do not halt.

Never add an enforcement that nobody approved.

## 7. Final report

Always end with these fields, also when the run failed. Use `0` or `none`
for an empty field. Template: `references/audit-and-fixes.md`.

1. Comments deleted (count)
2. Comments restored
3. Pruning-pass reruns (0 or 1)
4. Design sketch (or none)
5. Fixes made
6. Enforcement offers
7. Enforcements added
8. Constraints left unenforced
9. Other open work

## What this skill touches

Hosts: none. The skill needs no network.

Reads:

- Scoped source files plus their diffs within this repo, so it can locate and weigh
  comments.
- Code around the scope and the version history, only to trace a symbol or
  test what a comment claims.

Writes:

- Scoped files within this repo, receiving: comment deletions, flag
  fixes and approved enforcements. Nothing else is written. `prunecheck` is
  extracted to a temporary folder and writes no project file.

Programs:

- `git`: computes the default scope, shows the diff of the pass, undoes a
  rejected pass, and shows history for tracing.
- A second agent, requested through the orchestrator: does the pruning pass,
  so that deletion and audit are done by different heads.
- The linter or type checker that the project already uses: tells what a
  suppressed rule checks, and confirms the scope after fixes.
- `python3`: runs the bundled `prunecheck` helper. It is optional. Without
  it, do the same checks by reading the diff.

## Limits of this build

- No subagent is spawned by the skill. The second agent comes from the
  orchestrator, or the pass is done as a closed phase by the same agent.
- No human approval prompt. Offers go to the orchestrator.
- `prunecheck` is a heuristic over common languages. It reports what it
  cannot classify. It never decides whether a comment was worth keeping.

## Files

| File | Holds |
| --- | --- |
| `references/pruning-pass.md` | the pruner's complete rules and report form |
| `references/audit-and-fixes.md` | audit checklist, tracing, fix rules, sketch form, offer form, report template |
| `references/scenarios.md` | 8 worked scenarios with the expected result |
| `references/script-prunecheck.md` | the helper program and its tests |
