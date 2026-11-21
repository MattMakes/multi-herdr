# Code and comment sweeps run as ledger review tasks

Work that a skill would hand to subagents runs as ledger review tasks that
fleet workers claim, because a fleet pane never starts a subagent. The
code-sweep and comment-sweep finish steps of a task
(`crates/horch-core/src/taskledger/finish/`) both work this way: horch
queues review tasks, a fleet worker claims each one, and the author waits
for them before it lands.

## Consequences

- The code-sweep step (#39, `crates/horch-core/src/taskledger/finish/codesweep.rs`)
  runs the `code-sweep` skill's review through the ledger. Each reviewer
  the skill's triage picks becomes a review task, with the reviewer's
  prompt and the diff in its ticket file. When every reviewer is done,
  horch merges the findings with the skill's verdict rules: any Critical
  finding fails. Important findings fail until each is fixed (a new
  commit, which is reviewed again) or answered with a reason. Minor
  findings never block. Only the orchestrator waives a Critical or an
  Important finding (`horch ledger waive`).
- The code-sweep round state belongs to the task, not the worker: a
  replacement with the same tree reuses the round. A new round cancels the
  open reviews of the old one as superseded. A review cancelled in the
  current round is queued again (#158, #163).
- The code-sweep loop is bounded (trace-fixes TRF-27..TRF-30). The task's
  `Review` mode sets the rounds and reviewers: none, no review. light, 1
  round of 2. full, up to 3 rounds of 3. Rounds 2 and 3 review only the
  change since the last reviewed tree, with the cheaper reviewer persona.
  A finding keeps its `F<n>` id across rounds (the id stands for a hash of
  its file, reviewer and text), so answers and waivers carry. The
  reviewers of a later round accept or reject each answer. After the last
  round, an Important finding without an answer or a waiver becomes a
  QUESTION to the orchestrator instead of another round.
- The comment-sweep step (CMS-01..CMS-06, `docs/specs/comment-sweep.md`,
  `crates/horch-core/src/taskledger/finish/comments.rs`) removes the
  comments a task added before it lands. It is a review task that a
  `comment-remover` worker claims. Unlike a code-sweep reviewer, it edits:
  it deletes comments in the author's work folder and commits there. The
  author waits, as it waits for reviewers. The commit moves the tree, so
  `check` runs again before the land.
- The comment-sweep state belongs to the task, not the worker: a
  replacement reuses it. A later commit of the author is swept too, from
  the last swept tree, at most `MAX_SWEEPS` times in a task. A sweep that
  the orchestrator cancels passes the step: the cancel is its ruling.
