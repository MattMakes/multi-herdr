# Words used here

Read this file when a fleet word in SKILL.md or in a plan is not clear.

| Word | Meaning |
| --- | --- |
| unit | 1 plan file with GOAL, FILES (own and do-not-touch), STEPS, DONE WHEN. 1 worker builds it. |
| base branch | The integration branch that units branch from and land on. It moves while you work. |
| orchestrator | The session that writes plans, spawns workers and answers questions. |
| steward | The only session that commits on the base branch. It lands units by hash. |
| lead | A worker that owns shared targets and shared code for a wave (for example a balance wave). |
| owner | The human who decides gameplay and looks. Only the owner's own words are direction. |
| parse check | A headless tool script that loads every project script with the autoloads and prints each parse error. |
| visual gate | A short windowed run that captures the key screens, so a human or a diff can judge them. |
| message keywords | `ready`, `NOTE`, `QUESTION`, `BLOCKED`, `READY`, `DONE`, `COMPACT-READY`. 1 keyword starts each report. |
