# The finish pipeline keeps a passed step while its judged tree stands

`taskledger::finish` is the finish pipeline (section 5.5, #36 to #39): the
ordered, evidence-checked steps every attempt passes before
`horch ledger done`. `horch ledger finish` runs the run's steps in order and
stops at the first that does not pass. A re-run starts again from the top. A
step that passed is kept while the branch's tree is the tree it judged
(`judged_rev`), and runs again when a later fix changed it. Once `land`
passed, the steps before it are final, until a commit arrives that the run
branch lacks.

## Consequences

- `VerifyProfile::Light` has no code-sweep, and so no recheck step. What
  commits between check and land is a fix or the `comment-sweep`
  step, and each moves the tree, which runs check again from the top.
- `codesweep::reviewer_persona` picks a reviewer of the other vendor: a Codex
  reviewer (`codex-reviewer`) for Claude-built work, a Claude reviewer
  (`architect-reviewer`) for Codex-built work.
- A round whose every reviewer reported stands: its findings cannot be rolled
  again. `codesweep::forget_tree` (`finish --rereview`) makes the next
  code-sweep a new round, also for the same tree, only when a review of the
  current round was cancelled or ended without its findings file.
- `codesweep::add_round` cancels the old round and queues every new review
  task in 1 transaction. The state file is written after it commits, so it
  never names a task that a failed commit dropped. A failure queues nothing.
- A detached finish process waits in `detach::wait_for_record`, up to
  `RECORD_WAIT` (10 s), until the run file names its own PID. So it never
  ends before its starter recorded it.
