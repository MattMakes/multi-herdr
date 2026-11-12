# Ubiquitous language: fleetd: the horch fleet control plane

## Shared
**Idea** - A rough request from the operator that 1 fixed state machine moves from captured to merged; only code advances it. _Avoid_: ticket (a ticket enters as a --direct idea), request
**Feature** - 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch, holding 1 main idea and its follow-on ideas. The task kind 'feature' is a different thing (H2). _Avoid_: project, epic
**Sub-Orchestrator** - The interactive Opus 5.5 agent of 1 feature that clarifies, specifies and plans 1 idea at a time, with a fresh process per phase. _Avoid_: orchestrator, lead
**Assumption** - 1 line in assumptions.md, [confirmed] or [assumed], that the operator approves or revises; the sub-orchestrator can also record its own reversible one. _Avoid_: requirement
**Plan** - The validated plan.yaml of 1 idea: tasks, dependencies, checks, paired judges. No task exists outside a committed plan. _Avoid_: ticket list
**Task** - A planned unit of work from plan.yaml with a kind, a check and an owner path set; it survives many attempts (H1). _Avoid_: job, claim
**Attempt** - 1 run of 1 worker on 1 task (A1, A2, ...), launched through the wrapper and ended by fleetd. _Avoid_: run, session
**Lease** - The time-limited hold of an attempt on its task that renews only on earned activity and pauses during sleep and waiting_quota. _Avoid_: claim, lock
**Worktree** - The git working tree that fleetd leases to 1 task before it exists and never removes before its work is landed or rescued. _Avoid_: checkout, workspace
**Rescue Ref** - A verified commit under refs/horch/rescue/ that saves the full state of a tree or an old branch tip before cleanup or rebase. _Avoid_: backup, archive ref (a different ref family)
**Task Branch** - The branch of 1 task that lands on its feature branch through the landing state machine. _Avoid_: worker branch
**Feature Branch** - The branch of 1 feature that collects its landed task branches and reaches the primary branch through 1 pull request. _Avoid_: integration branch
**Primary Branch** - The base branch of a project; main in this repository, not in every project. _Avoid_: main, master, trunk
**Bundle** - 1 candidate made from the feature branch tip plus every ready task branch of that feature, tested once by the fast gate. _Avoid_: batch, merge train
**Fast Gate** - The declared unit-test command of a project that every bundle and every rebased feature branch must pass. _Avoid_: CI, check
**Full Gate** - The fast gate plus the end-to-end tests, run on a feature tip over the current primary branch; a pass goes stale when either tip moves. _Avoid_: CI, e2e run
**Trial Merge** - A merge-tree result between 2 in-flight features, or a feature and the primary branch, recorded in the ledger with no checkout. _Avoid_: dry run
**Resolver Task** - A code-kind task that resolves 1 conflict between stacked features, at most 100 changed lines or 2 files. _Avoid_: merge task
**Judge Task** - A task of kind judge that ranks the misses of 1 judgment-kind task CRITICAL, HIGH, MEDIUM or LOW; it needs no judge itself (H5). _Avoid_: review task, judgment task
**Fallback** - The persona that a worker persona names for use when its provider pool shows a usage limit or an outage. _Avoid_: backup model
