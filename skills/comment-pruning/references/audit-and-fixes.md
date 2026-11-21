# Audit, fixes, enforcement and report

For the agent that was asked to run the skill. You own the result. The
pruner's report is a claim to verify.

## Audit checklist

Work through this list with the pruner's report and the diff of the pass
open together.

1. Run `prunecheck check` on the diff, with the scope as `--scope`. A
   `reject` verdict with `code-change`, `comment-added`, `out-of-scope`,
   `protected-header`, `formatter-directive` or a file-level finding is a
   rejection. For `unclear` findings, read the lines and decide yourself.
2. Compare the deletion count in the report with the diff.
3. For each deleted comment, ask: did a keep-list case cover it? If yes,
   that is a rejection reason (and the comment is a restore candidate).
4. For each flag, open the symbol. Is it in the scope? Is the stated reason
   true? A flag with a false reason is a rejection reason.
5. For each kept comment that marks code as intentional, check that no flag
   blames that same code.
6. Run `prunecheck suppressions` on the files of the scope. Every directive
   must be either kept for a keep-list reason or removed with a flag. A
   directive the report is silent about is a miss: judge it yourself by the
   rules of `pruning-pass.md`, and name the miss if you reject.

## Restore rules

- Restore only with both: an exact keep-list case, and proof located in the
  scope.
- A reshape flag (surprise in project code) is never answered by
  restoring the comment. The flag stays open for step 5 of the skill.
- A fix flag on a correctness or safety suppression is never answered by
  restoring the suppression. The flag stays open.

## Tracing a symbol and reading its history

Use this when a loud comment was deleted or kept on thin grounds.

- Trace: find the definition, then every caller or reader of the symbol.
  Check whether the path is still reachable and whether the claimed
  behaviour happens.
- History: read the log of the lines in question and the commit messages
  that introduced them. `git log -L` for a line range, `git blame` for
  authorship and `git log -S` for the first appearance of a string are the
  usual tools. If history is not available, ask the orchestrator what is
  known about the symbol.

Doubt after tracing always ends in deletion. A keep needs positive proof
that the matter is external to the project.

## Rejecting

1. Undo the whole pass: restore the files of the scope to the state you
   noted before the pass.
2. Ask for the pass again, 1 time, and state the failure in a sentence
   (for example: "the last pass renamed a function; change comments only").
3. Audit the second result with the same checklist.
4. If it fails again: undo it, stop all later steps, and give the final
   report with the rejection reason under open work and reruns at 1. The run
   counts as failed.

## Fixing accepted flags

Principles, in order:

1. Cure the cause. Ask what made the workaround or the surprise necessary,
   and change that.
2. Stay small. Pick the least change that cures it.
3. Stay inside. The scope does not grow, and similar cases in other files
   are left alone.
4. Remove the workaround itself, every one a flag named.
5. No masking. A null check, a retry, a catch-all or a default value that
   only makes the symptom vanish is forbidden.

Simple flags are fixed directly, with no sketch. Examples: remove a branch
that no caller reaches; delete an argument that nothing reads; replace a
hand-made detour with the interface made for the purpose.

Pair each removed correctness or safety suppression with its fix in this
step, then run the linter or type checker again, so the tree does not stay
broken.

When the cause is beyond the scope (a shared helper, a dependency wrapper),
do the least that helps within the scope, do not edit the other file, and
write the rest under open work with the file and symbol named.

## Design sketch

Needed as soon as a single accepted flag calls for structural design.
Write 1 sketch for the full set of accepted flags, not 1 per flag, and read
the code around the flags first. Form:

```text
Design sketch
Flags covered: <list>
Current shape: <2-4 lines>
Target shape: <names, types, boundaries>
Steps: <ordered, each within the scope>
Left for later (beyond the scope): <list or none>
```

The sketch step ends when the sketch is written. Change no file while you
sketch. Implementation follows as its own step.

## Enforcement offers

For each constraint comment by a person that is not about an external fact,
send the orchestrator 1 message:

```text
Enforcement offer
Comment: <file and line, quoted briefly>
Constraint: <what must not change>
Proposed check: <test | type | CI lint rule | runtime assertion>, in <file>
Cost: <rough size>
Reply "approve" or "decline".
```

Choose the cheapest option that really fails when the constraint is broken
and that fits in the scope.

| Answer | Action |
| --- | --- |
| Approve | add the check, confirm it passes, then delete the comment |
| Decline | add nothing, delete the comment, report the constraint as unenforced and open, sketch the enforcing work beyond the scope |
| No answer possible (unattended or evaluation run, no approval given beforehand) | same as decline; continue the run |

An approval given up front by the caller of an unattended run counts as
"approve" for the offers it names.

## Final report template

```text
Comment pruning report
Result: <completed | failed>
Scope: <files or diff, and base branch if derived>
1. Comments deleted: <count>
2. Comments restored: <list or none>
3. Pruning-pass reruns: <0 | 1>
4. Design sketch: <sketch or none>
5. Fixes made: <file :: symbol :: what changed>
6. Enforcement offers: <count and list>
7. Enforcements added: <count and list>
8. Constraints left unenforced: <list, each with a sketch of the needed work>
9. Other open work: <root causes beyond the scope, failing checks, rejection reason, deletions made on doubt>
```
