# The comment-sweep finish step

A comment is a liability: it drifts from the code and hides a design that
should explain itself. Every task loses the comments it added before it
lands, in every verification profile.

## What it does

`comment-sweep` is a finish step (task-ledger.md section 5.5). It runs right
after `check`. A worker never starts subagents, so the step queues 1 review
task for the `comment-remover` teammate and the author waits, as it waits
for code-sweep reviewers. The teammate deletes the comments on the lines the
task's diff adds or changes, commits in the author's work folder, writes its
report, and ends. Its commit moves the tree, so `check` runs again before
the land.

The ticket holds only the facts of the task: the work folder, the diff, the
changed code files and the report path. How to sweep is the teammate's own
prompt (`teammates/comment-remover.md`).

The sweep state belongs to the task (`comment-sweep.json` in the task's
evidence folder), so a replacement worker reuses it.

## Requirements

| ID | Requirement |
|---|---|
| CMS-01 | The step queues 1 review task for `comment-remover`, with the work folder, the diff, the changed code files and the report path in its ticket. The author waits, and a second `finish` queues nothing more. |
| CMS-02 | When the sweep task is done the step passes with the report as evidence, and a commit of the sweep runs `check` again. |
| CMS-03 | A change with no code file (only prose, data or images) is `not_applicable`, and nothing is queued. |
| CMS-04 | A sweep task that the orchestrator cancels passes the step with the verdict `skipped`. |
| CMS-05 | A commit after a sweep gets 1 more sweep, of the change since the swept tree only. A task has at most 2 sweeps; later commits pass with a note. |
| CMS-06 | The profiles `minimal`, `light` and `full` all run `comment-sweep` right after `check`. |
| CMS-07 | The ledger migration to version 8 keeps every `task_steps` row and accepts the step name `comment-sweep`. |
