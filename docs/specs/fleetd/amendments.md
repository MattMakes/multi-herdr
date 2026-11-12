# Amendments to the fleetd design

Source: `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/source-design.md` ("the source").
Decisions: `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/decisions.md` (D1 to D13).
The operator confirmed each amendment below in a design review on 2026-11-11 and 2026-11-12.
Line numbers are lines of the source. They are hints. Find each place by its text.

Each amendment has: the decision, the new content, and the places in the source to change.
When an amendment and the source disagree, the amendment wins.
When an amendment does not cover a part of the source, keep that part word for word.

## Terms to use everywhere

- "primary branch": the base branch of a project. Do not write `main` for it, except in an example of this repository.
- "feature": 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch. It holds 1 main idea and its follow-on ideas.
- "feature branch": the branch that every task of a feature lands on.
- "task branch": the branch of 1 worker's task.
- "fast gate": the unit tests of a project. "full gate": the fast gate plus the end-to-end tests.
- "resolver task": a task that resolves a conflict between 2 features.
- "judge task": a task that reviews a non-code result.

## A0. Front matter

- Keep the title and the author line. Add under it: "Amended 2026-11-12 after a design review. See Amendments."
- Add a section `## Amendments` after `## Summary`. It has 1 table: `#`, `Decision`, `Sections changed`. 1 row for each of D1 to D13.
- Rewrite the `## Summary` so that it is true after all amendments. Keep its 5 numbered decisions, with these changes:
  - Decision 1: `horch fleetd` runs under the service manager of the OS (launchd, systemd or a Windows service) through 1 platform layer.
  - Decision 4: replace "Ideas cannot merge" with: an idea belongs to exactly 1 feature, and the ledger records it.
  - Add decision 6: every task lands on a feature branch through a landing state machine in the ledger, and no branch is lost.
  - Add decision 7: there is no spend budget and no cap on parallel work. Opus 5.5 at medium effort is the default worker.

## A1. Three operating systems behind 1 platform layer (D1)

New content:

- fleetd supports macOS, Linux and Windows as first-class hosts. The release pipeline already builds 5 targets for the 3 OSes.
- Core fleetd code has no `cfg(target_os)`, `cfg(unix)` or `cfg(windows)`. It calls 1 platform interface. Each OS has 1 module that implements it (for example `platform/mac.rs`, `platform/linux.rs`, `platform/windows.rs`; the exact names are free).
- The interface covers these concerns: service install and restart; process facts (alive, start time, children, process group); stop and kill of a process group; file watching; sandboxing; sleep and wake; the transport of the fleetd command channel (a Unix socket, or a named pipe on Windows); the machine probe (free disk, load).
- Where an OS has no equivalent, the module returns an explicit "not supported" value. Core code decides what to do with it. It never fails silently.
- Lease time counts only while the machine is awake. A sleep must not expire a lease.
- The layer is for new fleetd code. Migration of the 178 existing OS-specific sites in production code is a separate decision. Cite `docs/specs/fleetd/preservation-06-wiring-and-platform.md`.
- The Linux details of the source (`systemd`, `Restart=always`, `WatchdogSec=30`, `/proc`, `inotify`, bubblewrap, `SIGINT` then `SIGKILL`) become the Linux module's implementation.

Places: the defaults table (line 47); a new requirement in `## Requirements` (give it the id PL-1 to PL-4 in a new group "Platform"); `## Architecture and roles` (the fleetd row at line 203, the wrapper at lines 206 and 217, and a new row "Platform layer"); `## Supervision, liveness and reconciliation` (the sleep rule); add a table "What each OS provides" built from sections 7 and 8 of `preservation-06-wiring-and-platform.md`; `## Open questions` (the host question is answered: all 3).

## A2. Usage limits and provider outages (D2)

New content:

- Every worker persona names a fallback persona. The roster refuses to load a persona without one.
- A worker that shows a usage limit or a provider outage is not stuck. fleetd does not burn an attempt for it.
- If the fallback persona is on another provider, fleetd restarts the task at once on a new worker of the fallback persona, on the same worktree.
- If not, the task goes to the state `waiting_quota`. Its lease is paused. It leaves the state when the pool resets.
- If both pools are limited, the task waits. fleetd does not go to a third persona.
- Today no code enforces "never reap a usage-limited worker". The rule is only in the orchestrator briefing (`preservation-04-behavior.md`, row S6). fleetd enforces it in code.

Places: a new requirement RR-11; the task state list in `## Ledger` (line 416) and the lease text; `## Worker profiles and adapters` (the `fallback` field, required); the stall ladder in `## Supervision`; FM-5 (line 787) is rewritten so that it agrees with this; Migration Phase 2.

## A3. Everything is an idea, with a fast lane (D3)

New content:

- `horch idea add --direct <ticket file>` takes a ticket that the operator wrote. It skips research and the interview. It makes a plan with 1 task. The plan still passes plan validation and still needs a check.
- There is 1 state machine and 1 way in. There is no task without an idea.

Places: `## Ideas: the dispatcher and sub-orchestrators` (the state table and the command list); a new requirement FR-12; Migration Phase 6.

## A4. What "done" means for a judgment task (D4)

New content:

- Each task declares a `kind`. Code kinds: `feature`, `quality`, `refactor`, `security`. Judgment kinds: `research`, `design`, `docs`, `report`, `creative`.
- For a judgment kind, the verifier runs a structural check (the file exists, the schema or the required sections are valid).
- Plan validation refuses a plan in which a judgment task has no paired judge task. The judge runs in a different session from the author, on a different provider when the roster has one.
- The judge ranks each miss:

| Rank | Effect |
| --- | --- |
| CRITICAL | Blocks. A fresh session, not the author and not the judge, fixes it. Then a fresh judge checks only the earlier CRITICAL items and the change. |
| HIGH | Does not block alone. The sub-orchestrator of the feature fixes it or dismisses it with a written reason. |
| MEDIUM, LOW | Never block. |

- "Good enough" passes. The second judge cannot raise new MEDIUM or LOW items. This is the guard against gold-plating.
- After 2 fix rounds, a result that still has a CRITICAL item goes to the operator.

Places: FR-4 (line 101) and RR-3 (line 116) are amended; the Verifier row (line 208); the plan schema examples in `## Workspace layout and file protocol` (lines 295 to 345: add `kind` and the judge task); FM-9 (line 791); a new failure mode "a document passes a structural check but is wrong".

## A5. Code review (D6)

New content: for a code kind, the task's check runs, then today's review scripts run unchanged: the code-sweep rounds, the findings F<n>, the answers file, the waivers and the verification profiles. Keep their limits (`preservation-05-contracts-and-values.md`, row V-14).

Places: the Verifier row (line 208); Migration Phase 4.

## A6. Features, landing and integration (D5, D10, D11, D12)

Add a new section `## Features, landing and integration` after `## Worktrees: leased, reconciled, never lost`. It has these parts.

### Features

- A feature is 1 sub-orchestrator, 1 feature branch and 1 pull request to the primary branch.
- fleetd creates the feature branch when the sub-orchestrator first writes its assumptions. The sub-orchestrator gives the name. fleetd checks that it is kebab-case and unique. fleetd writes the ledger row before it creates the branch.
- `horch idea add --to <feature>` adds an idea to an open feature. Its tasks land on that feature branch. With no `--to`, the idea starts a new feature.
- A feature is an intake funnel until its pull request merges. After the merge, a follow-up idea starts a new feature.
- The sub-orchestrator of a feature plans 1 idea at a time, with a fresh process for each phase. Tasks of several ideas of 1 feature run in parallel.

### Landing a task branch on its feature branch

- Every branch is a ledger row before it exists. Each row has a landing state, 1 owner and a time limit. Past the limit it escalates. No state is silent.

| State | Owner | Leaves when |
| --- | --- | --- |
| `building` | worker | the worker writes its result |
| `verifying` | fleetd | the check, and for a code kind the review, pass |
| `ready` | fleetd | fleetd puts it in the next bundle |
| `in_bundle` | fleetd | the bundle passes the fast gate and the feature branch moves forward |
| `landed` | nobody | terminal |
| `conflicted` | the same worker | its rebase conflict is resolved (today's behavior) |
| `gate_failed` | sub-orchestrator | its triage task lands |
| `abandoned` | operator only | terminal; a rescue ref is kept |

- The bundle: when several task branches of 1 feature are `ready` together, fleetd builds 1 candidate from the feature branch tip plus each ready task branch in queue order. Each task's commits are kept. The fast gate runs once on the candidate. On a pass, the feature branch moves forward to it. On a failure, fleetd bisects the bundle, sends the failing task to `gate_failed`, and the others go on.
- No human approves a landing on a feature branch.
- Keep these behaviors of today's land path (`preservation-04-behavior.md`): the ownership check under the land lock (F9), the reset after a red land (F7), a passed step stands only for the tree it judged (F5), the time limits on git network calls (F11), and the rule that 2 running tasks of 1 feature never own overlapping paths (L6).

### No work is lost

fleetd checks these on every pass against git. Add them to the invariant list at line 477 as I8 to I13.

1. A task is `landed`, and a feature is `merged`, only when its changes are in the target branch by content. This holds for a squash merge.
2. A feature branch and the primary branch move only by fast-forward to a candidate that passed its gate. A landing cannot overwrite another.
3. Before each rebase or forced update of a branch, fleetd writes a rescue ref of the old tip.
4. A fleet branch in git that has no ledger row is adopted as an orphan row and reported. It is never deleted.
5. A branch is deleted only when it is landed, merged or rescued. A `git stash` entry that names the branch blocks the deletion (`preservation-04-behavior.md`, row W6).
6. A sub-orchestrator exists for each feature that is not merged and not abandoned.

### Between features

- Parallel is the default. Any number of features can change the same file. A shared file name never makes a feature wait.
- Early detection: when a feature branch tip or the primary branch moves, fleetd makes a trial merge of each in-flight feature against the primary branch and against each other in-flight feature. It uses `git merge-tree --write-tree`, with no checkout. The result is a fact in the ledger.
- Stack on evidence: when the trial merge of 2 features conflicts, fleetd puts the younger feature on top of the older one. The conflict becomes 1 resolver task for a fresh agent. The agent gets the conflict in `zdiff3` form, the requirements of both features for those files, and the unit tests of both. It makes 1 commit for each requirement. The task lands on the younger feature's branch.
- Continuous rebase: when the primary branch moves, fleetd rebases each feature branch, the bottom of a stack first. After a clean rebase the fast gate runs again. A conflict becomes a resolver task. `rerere` is on.
- At plan time the sub-orchestrator gets the diffs of the files that in-flight features already changed. It can propose to build on one of them. The proposal is in the assumptions that the operator approves.
- The full gate runs once for a feature, when its tasks are landed, on its tip over the current primary branch. Failures become triage tasks of the feature. If either tip moves after a pass, the pass is stale and the gate runs again before the feature is marked ready.
- Each project declares its fast gate and its full gate. Plan validation refuses a plan for a project that did not declare both. This repository has no unit-only command today (`research-03-this-repo.md`, F10).
- The pull request: a project setting `pr` is `gh` or `manual`. `manual` pushes the feature branch and shows the compare URL. A stacked feature's pull request has the lower feature's branch as its base. A project setting says who merges; the default is the operator.
- A feature is `merged` when its changes are in the primary branch. fleetd reads this from git, not from the GitHub API.
- Starting values, to tune from data: a resolver task covers at most 100 changed lines or 2 files, else fleetd splits it by file.
- Not in version 1: a syntax-aware merge driver, and 1 pull request for several features. Record both as follow-ups with the reason (`research-02-merge-tools-and-agent-systems.md`, F13).
- Give the evidence in 1 short table: hot files change in 46% to 54% of ideas; 2.7% of 376 landed tasks hit a conflict; about 3% of clean merges break the build or the tests; 1 agent that builds 2 features beats 2 agents plus a merge by about 30%. Cite the research files and finding ids from D5 in the decisions file.

Places besides the new section: `## Ledger` (a `features` table, `ideas.feature_id`, a `branches` table with the landing state, the gate results with the 2 tips they judged); FR-2 (line 99) becomes "1 sub-orchestrator for each feature; 1 process never plans 2 ideas at the same time"; the idea state table (line 569, `integrating`) and line 575; `roll_up_ideas` at line 706; Phase 0 "two stacked ideas"; `## Open questions` (the question "One run, one branch and one PR per idea" is answered by this).

## A7. Exec mode is the default (D7)

New content: a worker runs headless: it starts, does its task and exits. A persona can be marked interactive. Sub-orchestrators are interactive. An interactive persona is also the way for the operator to pair with a worker in a pane.

Places: the defaults table; `## Worker profiles and adapters` (a `mode: exec | interactive` field); `## Open questions` (answered); Migration Phase 7 stays where it is.

## A8. No budget, no cap, and the model guide (D8)

New content:

- The purpose of the system is to use the subscription usage fully, and to get as much work done as possible, responsibly.
- There is no spend budget. Remove `max_tokens` from the budget examples (lines 311 and 522). Keep `max_minutes` as a hang guard. Keep the 256k context limit (G5) as a quality rule.
- There is no cap on features, sub-orchestrators or workers. Remove `max_sub_orchestrators` (line 577) and `horch idea bump` (line 395 and each other place).
- fleetd still stops an idle sub-orchestrator process after 30 min (line 594). The reason is now memory, not a slot. An open question or an approval starts it again.
- Health guards are not caps. fleetd pauses new dispatch only while free disk or machine load is past a safety floor, and resumes by itself. On Windows the machine probe returns unknown today (`preservation-06-wiring-and-platform.md`, row S12).
- Pacing: fleetd reads the usage window of each pool and dispatches so that the usage is consumed by the reset.
- The model guide, checked by plan validation: Opus 5.5 at medium effort is the default. Higher effort needs a stated reason. A Sonnet task must be marked as "no decisions". Fable needs the operator's flag.

Places: `## Goals, non-goals and defaults` (a new goal G6 and the defaults table); `## Ideas` (lines 577 and 594); `## Worker profiles and adapters`; a new requirement OR-4 (pacing) and OR-5 (health guards); `## Open questions` (the pane-count question is answered).

## A9. Rollout: automatic and immediate (D9)

New content:

- fleetd is a subcommand of the single `horch` binary. The CLI and the daemon cannot have different versions.
- `horch self-update` installs a new release and restarts the running daemon on the new binary at once. This is safe because all state is in the ledger and in files, and the loop derives everything again on its next pass. Today the watchdog already restarts itself when its binary changes (`preservation-04-behavior.md`, row S10).
- A CLI that finds an older daemon restarts it.
- Harness updates keep today's rule: a release must be more than a week old (`update_cooldown_days`).
- Remove the sentence "New behaviour ships behind a run-level flag, becomes the default once its gate passes for a week of real runs, and the old path is then deleted" (line 820). New text: a phase is released when its fault-injection tests pass, and it is on for every user at once. The tests are the only gate.
- Schema migration is forward-only and runs at daemon start. Keep today's safety (`preservation-05-contracts-and-values.md`, rows L-3, L-4, L-5): a newer schema is refused, a backup file is written before each migration, and a development build does not migrate the shared ledger.

Places: `## Migration plan` (the lead paragraphs); a new requirement OR-6; a new failure mode "daemon and CLI of different versions".

## A10. Details that the orchestrator settled

- Verifier: an acceptance command runs in the sandbox profile of the task's worker, never with the rights of fleetd. Place: the Verifier row (line 208) and SR group.
- Command channel: each `horch` command that changes state sends a request to fleetd over its channel. When fleetd is down, the command writes a request file, and fleetd reads it at start. Place: `## Architecture and roles`, Migration Phase 5.
- Waits: an exec worker that asks a question exits. Its task parks with no process and no time limit. The 1 h cap on a declared wait (line 515) applies only to an interactive persona; at the cap the process exits and the task parks in the same way. Place: line 515, line 594, `## Open questions` (answered).
- Knowledge: project facts (today `horch note --fact`) go to every sub-orchestrator. Each feature keeps its own decisions file. Place: `## Workspace layout and file protocol`.
- Plan format: `plan.yaml` replaces today's `MAP.md`. A ticket stays a Markdown file in today's format C1 (`docs/specs/ticket-format.md`), and `plan.yaml` names it. Place: `## Workspace layout and file protocol`, Migration Phase 6.
- Adapters: OpenCode, pi and Prime carry over with Claude Code, Codex and Antigravity (`preservation-06-wiring-and-platform.md`, row S15). Place: `## What herdr and the agent CLIs give us`, `## Worker profiles and adapters`.
- Versions: add 1 note above the CLI table (line 167): on 2026-11-11 this machine ran herdr 0.9.4-preview, Claude Code 2.1.322 and Codex CLI 0.167.0; the table was written from the documents of the versions it names, and Phase 7 verifies it again. Do not change a fact in the table.

## A11. How a worker's question is resolved (D13)

Replace the paragraph "Questions from workers go to the sub-orchestrator, not to you" (line 596) with this flow:

1. The worker writes 1 question file, commits its work in progress, and exits. The task parks. The other tasks of the feature go on.
2. The question is a ledger row with an owner (the sub-orchestrator of the feature), a state and a time limit.
3. On each pass, fleetd starts the owner if it is not running. Delivery repeats until an answer is recorded.
4. The answer is 1 of 4 cases:

| Case | What occurs |
| --- | --- |
| The approved assumptions or the spec cover it | The sub-orchestrator answers and cites the line. fleetd checks that the citation exists. |
| The answer changes an approved assumption | It goes to the operator as a `revise`. Only the affected tasks pause. |
| The question shows a flaw in the plan | The sub-orchestrator replans the task. |
| Nothing approved covers it, and it changes no assumption | The sub-orchestrator decides when the code or the spec makes the answer clear. It records the answer as a new assumption marked as its own decision. The operator can reverse it. Only product direction, scope or a trade-off that the operator must own goes to the operator. |

5. fleetd resumes the worker's session with the answer. If the resume fails, a fresh worker starts from the question, the answer and the committed work.
6. The answer goes to the decision log of the feature and to every later task of the feature.
7. Past its time limit, a question escalates to the operator in `needs-you`, with a herdr notification.

Places: line 596; `## Ledger` (a `questions` table); `## Workspace layout` (`outbox/questions/`, line 248); a new requirement RR-12; a new failure mode "a question is never answered".

## A12. Preservation analysis

Add a section `## Preservation analysis` before `## Failure modes and effects`. It says that fleetd keeps the behavior below, and it points to the 3 inventory files for the full tables:
`docs/specs/fleetd/preservation-04-behavior.md`, `docs/specs/fleetd/preservation-05-contracts-and-values.md`, `docs/specs/fleetd/preservation-06-wiring-and-platform.md`.

It has these 6 tables. Fill each one from the `## Summary` of the inventory files (every ranked `NOT MENTIONED` row), and from the rows that this file names. Use the row ids of the inventory files.

```markdown
### Behavioral invariants
| # | Behavior | Location | Business reason | Risk if lost |

### Contract surface
| Command or format | Who depends on it | Shape | Exit codes or status |

### Domain assumptions
| Value | Location | Why this value | Impact if changed |

### Wiring map
| Interface | Implementation | Registration | Verified |

### Credential source inventory
| Credential | Runtime source | Path or key name | Rotation | Proxy or cert required | Verified in new code |

### Failure mode analysis
| Risk | Likelihood | Impact | Mitigation |
```

These rows must be present:

- Security: `ANTHROPIC_API_KEY` is removed from every child process, every pane command line, every detached child, every updater and every probe, and the roster rejects a teammate file that sets it (E-10, S1). The Google keys are removed for `agy` (S3). herdr's own pane resume uses an environment that horch did not scrub (S2). A mail or outbox directory is mode 0700, and one that another user owns or can write is refused (M5). A sandboxed persona does not start on a host that cannot sandbox it (S9). `stop` refuses a worker of another host (S8).
- No lost work: W6, F9, F7, F5, W8, W9, L5, L9, M-7.
- No stuck fleet: F11, S7, L8, L6, and the land lock and check timeout values (V-10, V-11).
- Ledger: L-3, L-4, L-5, E-1, E-2.
- In the credential table: never write a secret or the value of a variable. Names and paths only. `Verified in new code` is "no" for every row, because the new code does not exist.

## A13. Migration and testing

- Phase 0 gains red tests for: several task branches ready at the same time, and none is forgotten; 2 features that conflict; a usage-limited worker that must not be reaped; a machine sleep that must not expire a lease; a daemon restart on a new binary in the middle of a run.
- The platform layer lands with Phase 1.
- CI runs the test suite on Windows. Today the Windows job only builds (`.github/workflows/ci.yml`, job `build-windows`).
- Phase 5 moves worker reports to files. That removes the Unix-socket need of the worker channel on Windows (`preservation-06-wiring-and-platform.md`, row S11). The fleetd command channel needs a Windows transport.
- Add 1 table "New work for each phase": Phase 1: platform layer, health guards, pacing. Phase 2: `waiting_quota`. Phase 4: the 2 declared gates, the landing state machine. Phase 6: the feature level, the fast lane, judge tasks, the model guide in plan validation, the question flow. Each phase that changes the schema: the backup and the newer-schema refusal.

## A14. Open questions

- "Your call": all 5 are answered. Replace the list with 1 table: the question, the answer, the decision id.
- "To confirm by test": keep the 4 items. Add: herdr preview 0.9.4 against 0.9.5; a Windows transport for the fleetd command channel; the size limit of a resolver task; the accuracy of a structural merge tool on this repository's recorded conflicts.

# Fix round 1 (2026-11-12)

The judge of ticket 08 read `docs/specs/fleetd/design.md` at commit `0505712d` and gave the verdict FAIL: 1 CRITICAL (J1) and 3 HIGH (J2, J3, J4). The review is `docs/specs/fleetd/design-review.md`. The orchestrator decided each finding. Amendments A15 to A18 are the decisions.

## A15. Hang detection on all 3 operating systems (fixes J1, CRITICAL)

New content:

- Hang detection is a concern of the platform layer. Add it to the list of concerns in A1.
- On each pass, fleetd writes a heartbeat: the time of its last full pass.
- Linux: the systemd watchdog (`WatchdogSec`) restarts a hung fleetd, as the source says.
- macOS: a launchd timer job runs `horch fleetd check` every 30 s. Windows: a scheduled task does the same.
- `horch fleetd check` restarts the service when the heartbeat is older than 30 s of awake time. Sleep time does not count.
- Each `horch` CLI command also makes this check before it sends its request.
- The summary sentence "a hung daemon is restarted" is then true on all 3 hosts. Remove each statement that hang detection is not supported on macOS or Windows.

Places: the summary (line 15); the platform layer rows near line 292; the table "What each OS provides"; the requirement group "Platform"; FM-12 and the failure mode near line 1200; Migration Phase 0 (add a red test: a hung fleetd is restarted, on each OS) and Phase 1.

## A16. The branch of a `--direct` feature (fixes J2, HIGH)

New content:

- The general rule stays: the sub-orchestrator gives the name of a feature branch, and fleetd creates the branch when the sub-orchestrator first writes its assumptions.
- A `--direct` idea with no `--to` has no assumptions phase. For it, fleetd makes the branch name from the ticket title (kebab-case, with a number suffix when the name exists), and creates the branch when it accepts the idea. The ledger row is written first.
- A `--direct` idea with `--to` uses the branch of that feature.

Places: line 738 and line 859. Both must state the same rule.

## A17. A `--direct` idea of a judgment kind (fixes J3, HIGH)

New content:

- A `--direct` plan has 1 work task. When the work task has a judgment kind, fleetd adds the paired judge task to the plan itself. The operator does not write it.
- Replace each "a plan with 1 task" for `--direct` with "a plan with 1 work task, plus its judge task when the kind is a judgment kind".

Places: line 139 (FR-12), line 738, and each other statement of the `--direct` plan shape.

## A18. A judge task needs no judge (fixes J4, HIGH)

New content:

- Add the kind `judge`. A judge task has the kind `judge`. It is not a judgment kind.
- The pairing rule applies to the judgment kinds `research`, `design`, `docs`, `report` and `creative` only. A task of kind `judge` needs no judge task. This is the explicit end of the chain.
- The verifier runs a structural check on the verdict file of a judge task: the file exists, it has 1 verdict, and each finding has a rank.
- A resolver task has a code kind. It follows the code path.

Places: lines 131 and 267 (the pairing rule), the plan example near line 426 (the judge task there gets `kind: judge`), FR-4, and the list of kinds.

# Revision round 2

The operator read the DDD draft and decided D16. The DDD run found 2 small faults in the design (discover hotspot H6, and a missing preservation row). Amendments A19 to A21 are the decisions. The base of this round is the design at commit `f6243ab2`.

## A19. The existing OS-specific code moves in 1 dedicated phase (D16)

Decision of the operator (D16): the 178 existing OS-specific sites in production code move into the platform layer in 1 dedicated phase. The options "move a site when a phase touches it" and "leave them" are rejected.

New content. The items below the first one are readings of the orchestrator. The operator can reverse them.

- Add a migration phase with the name "Phase P: the platform layer and the move of the existing OS-specific code". Its place is after Phase 0 and before Phase 1. Do not renumber Phases 0 to 7.
- Phase P creates the platform interface and the 3 OS modules. It then moves each of the 178 production sites of `preservation-06-wiring-and-platform.md`, section 6, behind the interface. It also moves the production sites of the forms `cfg(any(...))` and `cfg!(...)` that section 6 counts.
- Phase P changes no behavior. A gap of 1 OS that exists today stays a gap. The OS module states the gap as an explicit "not supported" value, so each gap of an OS is visible in 1 file.
- All sites move. A site that a later phase deletes moves also (for example the 45 socket sites of row 06 S11, which Phase 5 removes). The later phase then deletes the platform function together with its callers. Reason: the rule below has no list of exceptions.
- The platform layer is 1 place for all crates of the workspace. 4 crates have OS-specific sites: `horch-core`, `horch`, `herdr-install` and `horch-marketplace`. `herdr-install` and `horch-marketplace` do not depend on `horch-core`. So the layer is a leaf crate (`crates/horch-platform`) that depends on no other crate of the workspace, and each crate calls it. The design sentence "1 interface inside horch with 1 module per OS" stays true: the modules are `mac.rs`, `linux.rs` and `windows.rs` of that crate.
- The interface grows. Today PL-2 lists only what fleetd needs. Add the concerns of section 6: process identity and liveness; signals and stop; process groups; file locks; file permissions; paths and home directories; symlinks and links; shell and quoting; local sockets and pipes; detach of a child process; the machine probe; the sandbox host; and the small rest (archive format, file identity, user PATH). Each row of the table of section 6 must have a home in the interface.
- Phase 1 then adds only the fleetd concerns to the same interface: service install and restart, hang detection, sleep and wake, file watching, and the transport of the fleetd command channel. The sentence "The platform layer lands in this phase" leaves Phase 1.
- Move the sentence "CI runs the test suite on Windows; today the Windows job only builds" from Phase 1 to Phase 0. Reason: Phase 0 already asks for a red test "on each OS", and Phase P needs the tests on all 3 OSes as its check. A test that fails on Windows for a gap that exists today is fixed, or it is skipped on Windows by name with the row of section 6 that states the gap.
- PL-2 changes its scope from "core fleetd code" to "production code of the workspace": no `cfg(target_os)`, `cfg(unix)`, `cfg(windows)`, `cfg(target_family)` and no `cfg!` of these outside the platform layer. An `arch_scan` rule enforces it from Phase P on. The rule can name a site that cannot call the layer (a build script), each by path with its reason. Test code is not in scope: the 340 `cfg` sites in tests stay.
- Add requirement PL-6: the move of Phase P changes no behavior. Test: the test suite passes on macOS, Linux and Windows before the move and after it, and the `arch_scan` rule of PL-2 passes.
- Gate of Phase P: PL-2 and PL-6 green.
- Remove each sentence that says the migration of the existing sites is "a separate decision". The decision is D16.

Places: the Amendments table (the D1 row, and a new row for D16); Goals, non-goals and defaults (the Host OS row); Requirements (PL-2, new PL-6); Architecture and roles (the Platform layer row, the paragraph "One platform layer for three OSes"); the Migration plan (Phase 0, the new Phase P, Phase 1, and the table of what each phase adds); Open questions.

## A20. A state name is never written without its entity (fixes discover hotspot H6)

The fault: `ready` is a state of a task, of a task branch and of a feature. `landed` is a state of a task branch and of a worktree. The design also says "a task is `landed`", and a task has no state `landed`.

New content:

- The state names stay. Each state machine has its own table and column, so the ledger is not ambiguous.
- Add 1 rule: each status line, event, log line and message that gives a state also gives the kind of entity (task, task branch, feature, worktree, idea).
- Add 1 small table "States that share a name" with the meaning of each:
  - task `ready`: its dependencies are done, and it is not dispatched.
  - task branch `ready`: it is verified, and it waits for the next bundle.
  - feature `ready`: its full gate passed on its tip over the current primary branch.
  - task branch `landed`: its changes are in the feature branch by content.
  - worktree `landed`: its branch is landed, so nothing in it is unique.
- Correct each sentence that says a task is `landed`. A task is `done`. Its task branch is `landed`. Examples: invariant I8, the first no-loss invariant, and the row `integrating` of the idea states.

Places: the Ledger section (near the state lists and invariant I8); Ideas (the row `integrating`); Worktrees (the rows `landing` and `landed`); Features, landing and integration (the landing states and the no-loss invariants). Search the design for `landed` and `ready`.

## A21. Preservation row 06 S14

The fault: the Preservation analysis of the design lists rows 06 S10 to S13 and 06 S15. Row 06 S14 is absent.

New content: add row 06 S14 to the same table as rows 06 S11 to S13. Its content: the source design assumed systemd or launchd, `WatchdogSec`, `/proc`, inotify and Unix signals to a process group, and it did not name Windows. D1, A1 and A15 answer it: the platform layer, and hang detection on all 3 OSes.

Places: the Preservation analysis.
