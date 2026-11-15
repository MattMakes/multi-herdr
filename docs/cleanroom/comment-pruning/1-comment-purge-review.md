# Reviewed comment purge with follow-up fixes and constraint encoding

Spec `spec-comment-purge-review` for the skill `comment-purge-review`.

A user-invoked skill, written as agent instructions together with one helper-agent definition. The main agent hands a scope of files or a diff to a separate helper agent that deletes code comments not covered by a short keep list and flags the code that the deleted comments were excusing. The main agent then audits the helper's report and diff, rejects out-of-bounds work, fixes the accepted flags with the smallest in-scope change, offers to turn human-written constraint comments into enforced checks, and ends with a structured report.

## Purpose

Remove comments that narrate, decorate, preserve dead code or justify workarounds, so that code explains itself; turn the reasons behind removed comments into code changes or enforced checks instead of prose.

## Triggers

- The user invokes the skill explicitly by name; the model does not start it on its own.
- The user names specific files or a diff to clean of comments.
- The user invokes it with no scope, meaning the current change set against the base branch.

## Outputs

- Edited files in scope with comments deleted.
- Code fixes in scope for accepted flags.
- Optional new enforcement (a type, a runtime check, a test or a lint rule) that replaces a constraint comment, when approved.
- A final report listing: deletion count, restored comments, helper reruns, any design sketch, fixes made, enforcement offers, enforcements added, constraints left unenforced, and other open work.

## Capabilities

- reads: Files and diffs inside the caller's scope in the current repository, Surrounding code and version history needed to trace a symbol or check a claim
- runs: git, a helper agent started by the main agent, the project's own linter or type checker, to look up a suppressed rule
- writes: Files inside the scope in the current repository

## Public contract

- constant `main`: Default base branch used to compute the scope when the caller gives none.

## Behaviors

- b1. Given The caller supplies files or a diff, when the skill starts, then that supply is the scope for the whole run.
- b2. Given The caller supplies no scope, when the skill starts, then the scope is the current difference from the base branch, including uncommitted working-tree changes; the base branch defaults to main.
- b3. Given A scope is settled, when the main agent begins work, then it starts one separate helper agent, passes it the scope, and chooses that helper's reasoning depth and speed to suit the size of the job. It does not repeat the helper's rules in the hand-off.
- b4. Given The helper agent has a scope, when it reviews comments, then it deletes every comment in scope that is not covered by the keep list: narration of what code does, decorative banners, commented-out code, and justifications of workarounds are all removed.
- b5. Given A comment in scope, when the helper decides whether to keep it, then it keeps only: legal or licence headers; notes on non-obvious behavior forced by an outside dependency, platform, vendor or protocol the project cannot change; a formatter-ignore directive; lint suppressions whose rule is faulty, pedantic or purely stylistic; documentation comments that define a public interface contract; and links to an issue or design document explaining a constraint that code cannot express.
- b6. Given A comment explains surprising behavior in the project's own code, when the helper processes it, then the comment is deleted and the exact symbol is flagged as needing a reshape (rename, extraction, a type, or restructuring) so the behavior becomes obvious without prose.
- b7. Given A lint or type-checker suppression directive in scope, when the helper processes it, then it looks up what the suppressed rule checks. If the rule catches real defects or guards correctness or safety, the suppression is removed and the exact offending symbol is flagged as needing a fix.
- b8. Given A comment asserts importance, forbids removal, calls a change too risky, defers a problem, or gives a long justification, when the helper processes it, then it treats the wording as a prompt to investigate, not as proof. It reads the surrounding code; if the claim is not evident there it traces the named symbol or call, or asks for the history and reason behind it, or both. The comment is kept only when the claim is a keep-list case about something outside the project's control and is shown true today on a path that is still live. Otherwise it is deleted, the symbol is flagged, and the comment is never rewritten into a shorter version.
- b9. Given The helper cannot tell whether a keep-list case applies, when it decides, then the comment is deleted.
- b10. Given The helper has finished, when it reports, then it reports only: files touched, number of deletions, each flag with a one-line reason, and anything skipped. Every flag names code inside the scope and states a true reason. The helper changes comments only and writes no application code.
- b11. Given The helper's report and resulting diff, when the main agent audits them, then it rejects the report if it contains: edits to application code, changes outside the scope, deletion of a comment the keep list protects, a flag with a wrong reason, or a flag that blames intentional code the helper itself kept. It also checks for suppression directives in scope that the helper missed.
- b12. Given A deletion the main agent considers restoring, when it audits, then it restores the comment only when an exact keep-list case applies and there is proof within the scope. Reshape flags on the project's own surprising code stay actionable and those comments stay deleted. Flags on correctness or safety suppressions stay actionable.
- b13. Given A deletion or a keep that rests on thin evidence around an importance or do-not-remove style comment, when the main agent audits it, then it traces the symbol or asks for its history before accepting. An ambiguous deletion stays deleted. A keep that is refuted or still ambiguous is deleted. A keep survives only with proof it concerns something the project cannot change.
- b14. Given The main agent rejects the helper's report, when it is the first rejection, then it reverts the helper's changes and reruns the helper once, naming the failure.
- b15. Given The rerun is also rejected, when the main agent audits the second report, then it reports the matter as open and the whole skill run ends as a failure.
- b16. Given Accepted flags that are trivial, when the main agent fixes them, then it fixes them directly, for example by deleting an unreachable path, dropping an unused parameter, or calling the proper interface instead of a workaround.
- b17. Given At least one accepted flag needs a structural design, when the main agent plans fixes, then it requests one design sketch covering the whole accepted set and the surrounding code, stops that design step at the sketch, and implements afterwards as a separate step.
- b18. Given A design sketch or accepted flags, when the main agent implements, then it makes the smallest fix that addresses the root cause inside the scope and removes every workaround the flags named. If the root cause lies outside the scope, it makes the smallest in-scope fix and reports the remainder as open. It never adds a guard that only hides a symptom, and never widens the scope or fixes other instances outside it.
- b19. Given A human-written constraint comment that forbids removal, forbids rewording, or requires consulting a named person before a change, when the main agent reaches it, then if it concerns something the project cannot change it is left in place. Otherwise the main agent offers the cheapest in-scope way to enforce the constraint mechanically (a type, a runtime check, a test, or a lint rule in continuous integration) and waits for the user's approval.
- b20. Given An enforcement offer is approved, when the main agent proceeds, then it adds the enforcement first and then deletes the comment.
- b21. Given An enforcement offer is declined or not approved, when the main agent proceeds, then by default the comment is deleted anyway, the constraint is reported as unenforced and open, and the out-of-scope work that would enforce it is sketched. (See open questions: whether a declined constraint comment should stay in place is to be reconsidered.)
- b22. Given The run is unattended or an evaluation run, when an enforcement offer would need approval, then approval must have been given in advance by the caller; without it the offer counts as not approved.
- b23. Given All steps are done, when the main agent finishes, then it gives the final report with every item listed under outputs.

## Errors

- Two rejected helper reports in a row end the run as a failure, with the rejection reason reported as open work.
- A root cause outside the scope is not an error: the in-scope part is fixed and the rest is reported as open.
- Missing approval for an enforcement in an unattended run is treated as a refusal, not as a halt.

## Invariants

- The helper agent changes comments only and never writes application code.
- Nothing outside the scope is edited by either agent.
- Every flag names a symbol inside the scope and states a reason that is true.
- Legal and licence headers are never deleted.
- A deleted comment is never replaced by a shorter comment that says the same thing.
- The helper is rerun at most once per run.
- No enforcement is added without approval.
- An approved enforcement is in place before its comment is deleted.
- A design step produces a sketch only; implementation is a later, separate step.
- Fixes address root causes; symptom-hiding guards are never added.
- The skill starts only on explicit user invocation.

## Test scenarios

- t1. Narration and dead code are removed, a licence header stays. Expected: The header stays. The other 3 comment groups are deleted. The helper report lists the module, the deletion count and no flags. The main agent accepts and the final report shows no restores and no reruns.
- t2. Outside-forced behavior is kept, own-code surprise is flagged. Expected: The provider comment is kept. The retry comment is deleted and the retry helper is flagged for a reshape with a one-line reason. The main agent keeps the flag actionable and does not restore the comment.
- t3. A safety-relevant suppression is removed and fixed. Expected: The line-length suppression stays. The type suppression is removed and the call is flagged. The main agent then fixes the call in scope so the missing record is handled at its root, and reports the fix.
- t4. The helper edits application code and is rejected twice. Expected: First report rejected, changes reverted, helper rerun once with the failure named. Second report rejected. The run ends as a failure and the report lists the issue as open with 1 rerun.
- t5. No scope given. Expected: The scope is all 3 files' changes relative to the default base branch.
- t6. Constraint comment with approval. Expected: The test is added, then the comment is deleted. The final report lists 1 offer and 1 enforcement.
- t7. Constraint comment in an unattended run without pre-approval. Expected: No enforcement is added. The constraint appears in the final report as unenforced and open, with a sketch of the work needed. Whether the comment is deleted follows the resolution of the open question on unapproved constraints.
- t8. Root cause outside the scope. Expected: The smallest in-scope fix is made, the shared utility is not edited, and the remaining work is reported as open.

## Open questions

- No item was identified that must be replaced or removed, so replace_or_remove is empty. The points below are design decisions left to the builder.
- The main agent must not defer wholesale to the helper's view. The strict audit of the helper's output is the governing behavior.
- Several steps depend on supporting abilities that this spec does not define: choosing how a helper agent is sized, explaining the history of a symbol, producing a design sketch, and stating the principles for fixes. The built skill needs to describe these steps in its own words or name locally available equivalents.
- Doubt currently resolves to deletion at both levels (helper and main agent), including for warning comments and for correctness or safety suppressions. This bias is risky. Whether the build keeps delete-on-doubt, or holds doubtful items for the user, is undecided.
- The skill moves from comment removal to application-code changes (removing paths, dropping parameters, swapping interfaces, root-cause fixes) with no approval gate. Whether code fixes should need the user's consent is undecided.
- A human-written constraint comment is deleted even when the user declines its enforcement. Whether a declined constraint should stay in place instead is undecided.
- The helper should speak in a plain voice, with no persona and no fixed opening line.
- The helper must only touch comments, so the build should decide which tools the helper may use so that the comment-only rule is enforced.
- Removing a correctness or safety suppression before the code is fixed can leave the tree failing lint or type checks between steps. Whether the build should pair removal with the fix, or check the tree afterwards, is undecided.
- How the helper's definition is packaged and registered alongside the skill, and that the skill is registered under one consistent name, is undecided.
- What the helper does when the scope is empty, and how the base branch is overridden, are not specified.
