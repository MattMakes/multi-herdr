# Merges, edits and claims

Read this file when you merge a moving base into a worktree, edit a file with a script, check a report, or the fleet CLI rejects a flag.

## Merges in a worktree

1. Check `git status --short`. Save uncommitted work with `git diff > /tmp/<unit>.patch` and
   restore those files to the committed version.
2. If the merge refuses on an untracked `.uid` or plan copy, compare it with the incoming
   file, delete your copy, and merge again.
3. Read both hunks of each conflict. Keep both sides when they are additive. Write down any
   order rule (the second line reads what the first sets) and send it to the steward. For a
   `.uid` conflict, take the base's id. For a lead-owned file, take the owner's side.
4. Make no commit while `MERGE_HEAD` exists, because the commit completes that merge.
5. Check that the merge commit holds none of your draft.
6. Import, run the parse check, the focused tests and the full suite on the merged head.
7. Re-apply the patch per file: `git apply --3way --include=<file>`. A whole-patch apply
   fails on 1 file and applies nothing.
8. If the base moves again, list the new commits that touch your files. Dry-run with
   `git merge-tree` instead of a second merge.
9. If Godot rewrote a file you do not own (a bus layout, the project file), restore it.

## Asserted scripted edits

A `sed` or a heredoc can change the wrong row or a second match without a sound.

1. Read the file. Copy the exact old text with its exact indentation.
2. Replace with a helper that asserts the old text occurs exactly once. The helper
   in [script-rep.md](script-rep.md) does this for a list of pairs. Copy the block into a
   file.
3. Apply all pairs in memory, then write the file once. If an assert fires, the file stays
   unchanged.
4. When an assert fires, print the line with `cat -et` and count the tabs. Or pick a longer
   anchor when the anchor matched 2 places.
5. Match on a unique row id, never on a value that other rows share.
6. Use the editor tool for tricky multi-line edits or nested quotes.
7. Run `git diff` after a bulk edit. Read the new value back before you measure or commit.

## Verify claims before you report

A wrong number in a DONE travels into the next plan.

1. List every number and every "X causes Y" sentence in your report.
2. Check each one with a command against a log or a file. Remove what you cannot confirm.
3. Recompute 1 or 2 headline rows from the raw traces. When a result is far better than all
   others, look for a leak (for example free levels written into a real save).
4. Send a correction at once when an earlier report was wrong.

The orchestrator side (treat DONE as a claim, liveness evidence) is in
[fleet-roles.md](fleet-roles.md#verify-done-claims-orchestrator-side).

## Orchestration CLI drift

The prompt can name flags that the installed fleet CLI does not have.

1. When the CLI rejects a flag, read its usage line and run the plain form. Do not block.
2. When `done --unlanded` or `done --handoff` is rejected, run plain `done` with the reason
   in the summary.
3. If `worktree land` is unknown, report that the branch is committed and not landed.
4. If `done` fails twice on a registry file, do not edit that file. Note it and tell the
   orchestrator.
