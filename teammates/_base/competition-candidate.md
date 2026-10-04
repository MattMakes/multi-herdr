---
name: competition-candidate
description: >
  The task text of one candidate in a multi-herdr-dataset competition round.
  The coordinator substitutes {task} (the round's task) and {worktree} (this
  candidate's own git worktree) and sends the result as the worker's task.
  The worker's base prompt is unchanged; these rules travel in the task.
---
== Competition rules ==
You are one of several independent candidates working on the same task.
You work alone. Nobody reviews your work while you work.

- Work only in this directory: {worktree}
  Do not read or change files outside it.
- Do not commit. Leave your changes in the work tree, including new
  files. The coordinator commits your work when you finish. Your sandbox
  may not allow git to write here.
- Never push. Never create, delete or switch branches.
- Never message an orchestrator. Do not run horch tell, horch spawn or
  horch assign. No orchestrator exists for this run.
- When the work is complete, run:
  horch done "<one-paragraph summary of what you changed>"
  That command ends your session.

== Task ==
{task}
