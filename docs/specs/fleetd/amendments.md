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

# Fix round 2

The judge of ticket 23 read the round 2 change of the design and gave the verdict FAIL: 1 CRITICAL (L1) and 1 LOW (L2). The review is `docs/specs/fleetd/design-review-3.md`. A19, A20 and A21 are stated. Amendment A22 is the decision on L1 and L2.

## A22. A red test of Phase 0 does not block a gate (fixes L1, CRITICAL)

The fault: Phase 0 adds red tests. Each stays red until the phase that makes it green. The test of PL-6 says "the test suite passes before the move and after it". With the red tests of Phase 0 in the suite, Phase P cannot pass its gate.

New content:

- Phase 0 keeps a list of expected failures. Each red test of Phase 0 is on the list, with the phase that makes it green. A test run passes when each test that is not on the list passes, and each test that is on the list fails. A phase removes its tests from the list. "Green" for a failure mode means that its test passes and is not on the list.
- The test of PL-6 becomes: on each of macOS, Linux and Windows, each test has the same result before the move and after it. A test that passes before the move passes after it. A test on the list of expected failures still fails.
- The gate of Phase P stays "PL-2 and PL-6 green". Phase P needs only what Phase 0 and Phase P give.
- The sentence "A phase is released when its fault-injection tests pass" becomes "A phase is released when the tests of its gate pass". Reason: the gate of Phase P is not a fault-injection test. Change the same words in the row of D9 in the Amendments table.
- L2: the sentence "made 13 decisions" must agree with the rows of the Amendments table. D16 is a decision of the operator. Give the count that the table shows.

Places: Requirements (PL-6); the Migration plan (the paragraph before the phases, Phase 0, Phase P); the Amendments section (its first sentence, the D9 row, and a new row "Fix round 2: A22 (L1, L2)").

# Systems review round

The operator asked for 1 review of the whole design: is it coherent, and is it good systems design and systems engineering? 5 reviewers read the design at commit `043cc279`, 1 lens each. Each verdict is NOT SOUND. The reports are `docs/specs/fleetd/system-review-39-control-loop.md`, `system-review-40-state-machines.md`, `system-review-41-landing-and-git.md`, `system-review-42-migration-and-requirements.md` and `system-review-43-operations-and-agreement.md`. A finding id is `S<report>-<n>`.

The orchestrator read each CRITICAL and HIGH finding against the lines of the design. Each one holds. Amendments A23 to A31 are the decisions. They are grouped by root cause, not by report. The table at the end gives the amendment for each finding.

Rules for each amendment of this round:

- Use what the design has. Do not add a subsystem when a rule on an existing table closes the failure scenario.
- The section `## Checked and sound` of each report names mechanisms that are correct. Do not change them.
- The section `## Answers` of each report names more places where the design is silent. When an amendment covers such a place, state the rule there also.
- A decision of the operator (D1 to D4, D6 to D14, D16) does not change.

## A23. Long work is a job. A pass never waits for it. (S39-1, S39-7, S39-5)

The fault: the fast gate, the full gate, trial merges, rebases and git network calls run inside the reconcile pass. The heartbeat is written only after the pass. The hang check restarts fleetd when the heartbeat is older than 30 s. So a healthy fleetd with a long gate is restarted for ever.

New content:

- The design already runs a verification as an async job (`run_due_verifications`). The same rule now applies to each piece of work that can take more than 1 s: a task check, a fast gate, a full gate, the build of a bundle candidate, a trial merge, a rebase, a rescue, a `git fetch`, a `git push`.
- Add 1 table `jobs`: id, kind, subject (the row that it works for), input key (for example the 2 tips of a trial merge, or the candidate tip and the base tip of a gate), state (`queued`, `running`, `passed`, `failed`, `abandoned`), pid and start time, the time limit, the result. `gate_runs` and `trial_merges` rows point at their job. A job with the same kind and input key is not run 2 times: its result is used again.
- A job runs in its own process, outside the pass. fleetd supervises it like an attempt: the row first, then the process, alive by PID and start time, stopped at its time limit. After a restart of fleetd, a `running` job whose process is gone goes back to `queued`.
- A pass only reads facts, decides, writes rows and starts or observes jobs. `land_bundles` and `integrate_features` in the reconcile pass become "queue the jobs that are due, and act on the jobs that ended".
- A pass has a work budget. It handles at most a set count of rows of each kind (a starting value in `fleet.toml`), keeps a cursor in the ledger, and continues at the cursor on the next pass, in a fair order. A pass must end before the hang limit. `horch status` shows the age of the oldest row that waits for a pass.
- Trial merges between features: fleetd queues 1 job for each pair of features whose sets of changed files share a file, keyed by the 2 tips. A pair with no shared file needs no job. A count of features of n does not make n times n jobs in 1 pass.
- Remote git state is level-triggered: 1 `git fetch` job for each repository on a period (default 60 s, in `fleet.toml`). fleetd records the fetched tip and the time. Merge detection and gate freshness use the fetched tip. When a fetch fails, `horch status` shows since when the remote tip is stale.
- At most 1 job that changes git state runs for 1 repository at a time. This removes git lock contention. Jobs that only read git can run together.

Places: Architecture and roles (fleetd, Verifier); Ledger (new table `jobs`, `gate_runs`, `trial_merges`); Features, landing and integration; Supervision, liveness and reconciliation (the reconcile pass and its comments); Requirements (PL-5: say that the heartbeat does not depend on a job); Failure modes (FM-12).

## A24. An effect outside the ledger: the row first, then the effect, then the observation (S39-2, S39-3, S39-4, S39-8, S43-1, S43-7)

The fault: the design says that a pass is 1 SQLite transaction. A process start, a file write and a git ref move are not part of a transaction. A crash between the effect and the commit leaves the 2 out of step: 2 workers in 1 worktree, or a row that claims an effect that did not occur. Nothing stops 2 fleetd processes. A command request can be lost or applied 2 times. A worker file can be a link or have any size.

New content:

- The design already has the rule "the row first" for worktrees and branches. Make it the rule for each effect. A pass has 3 parts: (1) read the facts, with no transaction; (2) 1 transaction that decides and writes rows, each effect as a row in a state that says "intended" (for example an attempt in `launching`, a worktree in `reserved`, a job in `queued`); (3) after the commit, do the effects. The next pass observes each effect and moves its row on. No effect occurs inside the transaction.
- Each effect is safe to repeat. Its key is the id of its row. A launch names the attempt id: the attempt folder, the wrapper and its heartbeat carry it. Before a launch, fleetd looks for a live process of that attempt id and does not launch a second one. An intended row with no effect after its time limit is done again or failed, by the same pass rule.
- In the pass order, the stop of orphans and the observation of intended rows come before `dispatch`. A new attempt never starts in a worktree while a process of an older attempt of that task is alive.
- 1 fleetd for 1 ledger, by a mechanism. fleetd holds an exclusive OS file lock through the platform layer for its whole life. A second fleetd that cannot take the lock exits at once. The ledger has 1 row with a generation number. fleetd adds 1 to it at each start. Each write transaction checks that the number is its own, and fleetd exits when it is not. A restart (the service manager, `horch fleetd check`, `horch self-update`) stops the old process by its recorded PID and start time, waits until the lock is free, and only then starts the new one.
- A command request has a contract. Each request has a `request_id` that the client makes (this is the idempotency key of the DDD model), a kind, and a payload that never changes. Add 1 table `requests`: the id, the kind, the hash of the payload, the state (`pending`, `applied`, `refused`), the result and, for a refusal, the reason. fleetd applies a request and writes its `requests` row in 1 transaction. A request with an id that is already applied or refused gets the stored result again and has no second effect. A request file is moved to an archive folder only after its row is committed. The client can ask for the result of a `request_id` at any time.
- The command channel accepts only the user that owns the ledger. The socket or named pipe checks the peer user through the platform layer. The request folder has mode 0700 and each file 0600, and fleetd checks the owner of a file before it reads it. On Windows the platform module sets an equal access rule. A worker cannot send an operator command: `approve`, `abandon` and `--direct` intake are refused inside a fleet attempt, as the design already says for `--direct`.
- A file that a worker writes is not trusted. For each file type, the design gives: a size limit in bytes; for a log, a limit of lines; it must be a regular file; fleetd opens it without following a link; its real path is inside the outbox of its own attempt; the task id and attempt id in the payload equal those of the path. fleetd records a file that fails 1 of these checks as rejected, with the reason, in `ingested`, and does not read it again.
- The file protocol has 3 kinds of file, and the rule "never edited after it lands" applies to the first only: an immutable message (`task.yaml`, `START.md`, `result.json`, a question file, `verdict.json`, a request file); a snapshot that is replaced atomically (`heartbeat.json`), which fleetd reads but does not ingest; an append-only log (`progress.jsonl`), which fleetd reads from the last offset that the ledger recorded.

Places: Summary (item 1, say "1 transaction for the ledger changes of a pass"); Workspace layout and file protocol; Architecture and roles (fleetd, the command channel, the Platform layer row: add the file lock and the peer-user check to the concerns of Phase 1); Ledger (the rule "One door", new table `requests`, the generation row, `ingested`); Supervision, liveness and reconciliation (the reconcile pass); Requirements (RR-8, PL-2, the SR group); Failure modes (FM-11, FM-12, FM-15, FM-23, FM-24).

## A25. One fleet clock for each timer (S39-6)

The fault: lease time counts only awake time. Other timers do not say. After a sleep, a branch deadline and a question limit fire at once.

New content:

- fleetd keeps 1 fleet clock in the ledger: a count of seconds. Each pass adds the time since the last pass, but at most 2 times the pass period. So the clock does not run while the machine sleeps and does not run while fleetd is down. No actor is judged for a time in which nothing could act.
- Each operational timer is a deadline on the fleet clock: the lease, each rung of the stall ladder, the start timeout, each retry backoff, the wait limits, the question limit, `due_at` of each row, the time limit of a job. PL-4 now names all of them.
- These use calendar time, and the design says so for each: the hang check of fleetd itself (awake time from the platform layer, because the fleet clock stops with fleetd), the 7 day park, reminders, retention periods, the daily docs sync, the usage windows of a provider.

Places: Requirements (PL-4); Ledger (the fleet clock row, the comment of each `due_at` column); Worker profiles and adapters (the timer list); Supervision, liveness and reconciliation ("A sleep does not expire a lease" becomes the general rule).

## A26. The task state machine is complete (S40-1, S40-2, S40-3, S40-4)

The fault: the task machine has 12 states and no table. The source design had a diagram here, and the conversion lost it (the line "embedded content: task state machine"). A judged task and its judge task wait for each other. An exit with a question, or on a usage limit, burns an attempt. A dependent task starts before the code of its dependency is on the feature branch.

New content:

- Add a task state table with the same columns as the worktree table: state, owner, leaves when, time limit. It has all 12 states of the `CHECK` list. It names: `leased` to `running` (the wrapper's first heartbeat); the state during a retry backoff (`ready` with a not-before time on the fleet clock); `failed` (owner: the sub-orchestrator of the feature, which replans 1 time, with a time limit; then `escalated`); `escalated` (owner: you; `horch task requeue` sets the attempt count to 0 and moves it to `ready`; `horch task cancel` moves it to `cancelled`); `blocked_on_user` (entered by a question; left by its answer); `done` and `cancelled` are terminal.
- Replace the 3 lines "embedded content" of the design. For the task machine: the new table. For the idea machine: point at the idea state table that exists. For the components: a short list of each component and the files that go between them, from the Workspace layout section.
- Order inside 1 pass: results are ingested and acted on before a lease is expired. A valid `result.json` that is present in a pass wins over a lease expiry in the same pass.
- An illegal transition never stops the pass. `transition()` refuses it, records an event with the row and the cause, and the row goes to the needs-you list. Each other row of the pass goes on.
- 2 exits are not failures and burn no attempt. An exit with a valid question file has the outcome `asked`: the task moves to `blocked_on_user`, and the worktree stays. An exit on a usage limit or an outage has the outcome `quota`: the task follows RR-11 (`waiting_quota`, or a restart on the fallback). Each adapter maps the usage-limit signal of its CLI in exec mode also. The Stop hook and the repair resume of the wrapper accept a question file in place of `result.json`. `finish_exited` looks for these 2 outcomes before it treats an exit as `no_result`.
- When is a dependency met. For a normal dependency (`after:` on a task that is not the one it judges): the dependency is `done` and its task branch is `landed`. Invariant I2 and `promote_pending` use this rule, so the worktree of the dependent task has the code. The task state `done` means "verified". The task branch state `landed` means "its changes are in the feature branch".
- The judge pair. A judge task depends on the task it judges in another way: the dependency is met when the judged task is in `verifying` and its structural check passed. The worktree of the judge task starts from the tip of the task branch of the judged task, so the judge reads the result before it lands. The judged task stays in `verifying`. It moves to `done` when its judge task is `done` with no open CRITICAL item. Then its task branch moves to `ready`. The task branch of the judge task holds only the verdict file on top of the judged branch, and it lands after it.
- A fix round needs no change of the plan. When the judge reports an open CRITICAL item, fleetd starts a new attempt of the judged task, in a fresh session, with the verdict file as input, in the same worktree. Then it starts a new attempt of the judge task, in a fresh session, whose scope is the earlier CRITICAL items and the change. This is the "fresh session" of D4. A fix round counts in its own counter `fix_rounds` (limit 2), not in `max_attempts`. After 2 fix rounds with an open CRITICAL item, the judged task goes to `escalated`.
- Phase 4 names "land" as a step of landing, not of the verifier.

Places: Requirements (RR-3, FR-4); Architecture and roles (Done for each kind of task, the 3 "embedded content" lines); Workspace layout (the plan example: comments at `after:` and `judges:`); Ledger (`tasks`: the columns for the not-before time and `fix_rounds`; I2); Worker profiles and adapters (the Stop hook row, the usage-limit signal); Ideas (the question flow, step 1); Supervision, liveness and reconciliation (the stall ladder rungs 5 and 6, the reconcile pass order); Migration plan (Phase 4).

## A27. The landing state machine survives a crash and a race (S41-3, S41-4, S40-5, S40-7, S41-7)

The fault: a bundle and its bisect have no row. A branch move has no check of the tip that fleetd expected. `conflicted` and `in_bundle` have no exit that can occur. A cancelled task or a failed rescue blocks its idea for ever. Rescue refs have no class, no owner and no limit.

New content:

- Add 2 tables. `bundles`: id, feature, the base tip, the candidate ref (`refs/horch/bundle/<id>`) and tip, the parent bundle (for a bisect part), the state (`building`, `gating`, `passed`, `failed`, `dropped`), its job. `bundle_members`: bundle, task branch, position. A task branch is a member of at most 1 bundle that is not ended. A bisect is a set of child bundles of the failed bundle. After a restart, fleetd continues from the rows.
- Each `branches` row stores the tip that fleetd last saw (local and remote). Each move of a ref is a compare-and-set: `git update-ref <ref> <new> <expected old>` for a local ref, and `--force-with-lease=<ref>:<expected old>` for a push that rewrites. When the tip is not the expected one, fleetd moves nothing, records the new tip, makes the gate result stale and decides again on the next pass. Someone else's commit is never overwritten.
- When the tip of a feature branch moves while a bundle of that feature is in `building` or `gating`, the bundle goes to `dropped` and its members go back to `ready`. fleetd does not start a rebase of a feature branch while 1 of its bundles is in `gating`. It waits for the job, which has a time limit.
- `conflicted`: the owner is fleetd, not "the same worker" (an exec worker has exited). fleetd starts a resolver attempt for that task branch in its worktree, with the conflict in `zdiff3` form and the tests of both sides. When the resolver result passes its check, the branch goes back to `ready`. `gate_failed`: fleetd starts a fix attempt of the task with the gate output; then the branch goes back to `verifying`.
- Give a time limit (a starting value, on the fleet clock) for each landing state. At the limit the row goes to the needs-you list.
- The roll-up of an idea counts only tasks that are not `cancelled` and task branches that are not `abandoned`. When a task is cancelled, fleetd writes a rescue ref of its branch and then moves the branch to `abandoned`. The operator is not the only actor that can set `abandoned`.
- An idea is `merged` when each of its worktrees is `removed` or `rescued`. fleetd then removes the `rescued` trees. The rescue refs stay. The worktree states `landing` and `orphaned` get an exit to `rescued`. A rescue that fails its verification keeps the tree and puts 1 item on the needs-you list.
- fleetd sets `rerere.enabled` and `merge.conflictStyle=zdiff3` in each worktree and each repository that it manages. It does not assume them.
- Rescue refs have 2 classes, and fleetd owns both. `unique`: it holds work that is in no landed branch. It is kept until you decide (`horch worktree restore`, or a new command `horch rescue drop`). No collection deletes it. `superseded`: an old tip whose work is in a newer tip or in a landed branch. It expires 30 days after its feature is merged or abandoned, with the trash refs. `horch status` shows the count and the bytes of each class. When the `unique` class is over a budget (a starting value in `fleet.toml`), fleetd puts 1 item on the needs-you list. It deletes nothing.

Places: Ledger (new tables `bundles`, `bundle_members`; `branches`: the tip columns; I9, I10, I12); Worktrees (the state table rows `landing`, `orphaned`, `rescued`; the rescue paragraph; "Ideas finish clean"); Features, landing and integration (the landing state table, "The bundle", "No work is lost"); Ideas (the rows `in_progress`, `integrating`, `merged`); Failure modes (FM-25 to FM-32).

## A28. The feature state machine, and 1 sub-orchestrator for each feature (S41-2, S40-8, S43-2, S40-6, S41-5, S41-8, S43-6, S43-5, S43-4)

The fault: the feature machine has 4 states and no table. I7 asks for 1 live sub-orchestrator for each idea; FR-2 and I13 ask for 1 for each feature. An idea that is added while the full gate runs is left with no owner when the feature merges. A stack has no rule for the end of its lower feature. A failed push and a closed pull request have no state. The needs-you list only grows. `horch status` cannot say why a ready task does not run.

New content:

- Add a feature state table: state, owner, leaves when, time limit.
  - `open`: fleetd. It moves to `ready` when 2 things are true: each idea of the feature is `integrating` (or `cancelled` or `parked`), and the full gate passed on the feature tip over the fetched tip of the primary branch.
  - `ready`: you (or fleetd when the project sets the merge to automatic). It moves back to `open` when its tip or the primary tip moves, or when a new idea is accepted with `--to`. It moves to `merged` when git shows its changes in the primary branch by content.
  - `merged`: terminal. fleetd decides it 1 time and does not check it again.
  - `abandoned`: terminal. Only you set it, with a new command `horch feature abandon <feature>`. fleetd first writes a rescue ref for each branch that is not landed.
- The pull request follows the state. fleetd opens it, or shows the compare URL, only when the feature is `ready`. When the feature moves back to `open`, fleetd marks the pull request as a draft (`pr = gh`), or changes the needs-you item to "do not merge yet" (`pr = manual`).
- A merge with ideas in flight (a call of the orchestrator; it reads D12 "then we start over"). You can merge a pull request at any time. When git shows the merge and the feature still has an idea that is not `integrating`, fleetd records the feature as `merged` and moves each such idea to a new feature. The new feature branch starts from the fetched primary tip. The sub-orchestrator and its pane go on with the new feature. Each task branch of a moved idea is rebased on the new feature branch, with a rescue ref first. No idea is left without a feature and an owner.
- I7 counts for each feature: each feature that has an idea from `clarifying` to `integrating` has exactly 1 live sub-orchestrator attempt, or it waits for you. I13 stays. FR-2 stays.
- Ideas of 1 feature are planned in order. While 1 idea of a feature is in `clarifying`, `specifying` or `ticketing`, a new idea of that feature waits in `captured`. Correct the sentence "no idea waits in `captured` for a slot": no idea waits for a slot of the fleet, and ideas of different features never wait for each other.
- The live process of a feature, in any phase, owns the open worker questions of the whole feature. fleetd rings it when a question arrives. It answers open questions before it continues its phase. The question limit (15 min on the fleet clock, a starting value) covers a process that does not answer.
- A stack edge ends with its lower feature. When the lower feature is `merged`: fleetd clears `stacked_on` of the upper one, rebases it on the fetched primary tip (a rescue ref first), runs both gates again, and sets the base of its pull request to the primary branch (`gh`), or changes its needs-you item (`manual`). When the lower feature is `abandoned`: fleetd rebases the upper one on the primary tip without the commits of the lower one. A conflict opens a resolver task. The owner of each step is fleetd.
- Push and pull request results are facts on the feature row: the push state (`ok`, or `failed` with a retry count and a backoff, and after 3 failures 1 needs-you item), and the pull request state (`none`, `open`, `closed`). With `pr = gh`, fleetd reads a pull request that is closed and not merged and puts 1 item on the needs-you list: open it again, or run `horch feature abandon`. With `pr = manual`, you run `horch feature abandon`. A project can set `remote = none`: fleetd does not fetch, push or open a pull request, and a feature is `merged` when its changes are in the local primary branch.
- The needs-you list has a life cycle. Each item has a stable key (its kind and its row), and a state: `open`, `resolved`, `superseded`. A producer writes by key, so 1 thing has 1 item. `horch ideas` and the folder show only `open` items. An item that is not open is removed after 30 days. The ideas context owns the list.
- `horch status` shows each hold on dispatch with its cause: a health floor, pacing of a pool, an unhealthy profile, a git job that holds the repository, a missing fast gate. fleetd records an event when a global hold starts and when it ends.
- The health guards also cover free memory, the count of processes and the count of open files of the user, from the machine probe of the platform layer. They pause new dispatch, as the disk and load floors do. They are not a cap on parallel work (D8).

Places: Requirements (FR-2, OR-1, OR-3); Ledger (`features`: the columns for push state, pull request state and `stacked_on`; I7, I8, I13; a table for needs-you items); Ideas (the idea state table, "How many sub-orchestrators", the question flow); Features, landing and integration (Features, Between features, the pull request paragraph); Supervision, liveness and reconciliation (`keep_sub_orchestrators`, the health guards); Goals, non-goals and defaults (the health guard row).

## A29. Each process starts through 1 launcher that removes the key (S43-3)

The fault: the rule "`ANTHROPIC_API_KEY` is removed from every child process" is correct, but the new launch paths are not tied to it: the verifier, the jobs of A23, docs sync, the conformance launches. The native pane resume of herdr uses an environment that horch did not clean, and the design only warns.

New content:

- Each process that fleetd, the wrapper, a job, docs sync or the conformance suite starts goes through 1 launcher function. The launcher applies the environment rule of the profile, and it always removes `ANTHROPIC_API_KEY`. The worker-profiles context owns the rule. The platform layer owns the process start.
- An `arch_scan` rule finds no direct process start in production code outside the launcher.
- fleetd does not use the native agent resume of herdr. After a herdr restart, fleetd starts each attempt again through the launcher (FM-13). A pane that herdr restored by itself is an orphan by I5 and is stopped.
- Do not write the value of an environment variable in the design.

Places: Requirements (the SR group); Architecture and roles (fleet-run wrapper, Verifier, Platform layer); Worker profiles and adapters; Docs sync and adapter conformance; Preservation analysis (the rows on the key and on herdr resume); Failure modes (FM-13).

## A30. Each phase can pass its own gate, and each rule has a gate (S41-1, S42-1, S42-2, S42-3, S42-4, S42-5, S41-6, S42-6)

The fault: this class of fault was found 3 times (L1, S41-1, S42-1): a gate needs a thing that a later phase adds. Phase 1 needs leases (Phase 2) and rescue (Phase 3). Phase 4 needs the branch tables and the judge (Phase 6). The plan does not say what happens to a run that is in flight when a phase is released, and it does not stop an old watchdog that runs. 19 requirements and 9 failure modes have no gate. 6 preservation rows have no owner before cleanup starts.

New content:

- First make 1 table "What each gate needs": 1 row for each of the 9 phases: the gate, each mechanism the gate needs, the phase that gives each mechanism. Each mechanism must come from the same phase or an earlier one. Put this table in the Migration plan. Then correct each row that fails:
  - Phase 1: its gate is FM-4, FM-7, FM-12, and the part of FM-13 that Phase 1 can do: fleetd sees the dead processes, ends the attempts and queues the tasks again with the worktree handling of today. Phase 1 adds I4, I5 and the process part of I1. It also adds the `jobs` table, the 3-part pass, the fleet clock, the file lock and the generation number (A23 to A25).
  - Phase 2: the lease part of I1. Phase 3: the full FM-13 (the worktrees are rescued and used again) becomes green.
  - Phase 4: move the tables `features`, `branches`, `bundles`, `bundle_members` and `gate_runs` from Phase 6 to Phase 4. In Phases 4 and 5, each run is 1 feature row and its run branch is the feature branch. The landing state machine and the 2 gates work on it. The gate of Phase 4 covers FM-9 for code kinds and for the structural check. The judge pair of A26 comes in Phase 6 with the plan, and its failure mode becomes green there.
  - Phase 6 adds the idea level, several features at the same time, `trial_merges`, stacks, and the rules of A28.
- The Summary says that fleetd is the only ledger writer. Say "from Phase 5". For Phases 1 to 4, state the rule: each writer calls the same `transition()` with the same table of allowed transitions, SQLite gives 1 writer at a time, and fleetd treats each row as a fact that it reads again on each pass. No rule of Phases 1 to 4 needs the single writer.
- Phase 1 stops the old watchdog. At its first start, fleetd lists each recorded watchdog, stops each one by its PID and start time, and takes its runs. `horch ledger watch` then only points at fleetd. The gate of Phase 1 gets 1 test: an update with a live worker and a live old watchdog leaves 1 control path.
- Each phase states 4 things for a run that is in flight when the phase is released: the backfill of old rows; what happens to old processes; which old commands and files still work; and 1 test in its gate that starts on the phase before and ends on the new phase. Starting content: Phase 2 gives a held task a lease with a full time. Phase 4: an attempt that began with a claim ends by the old path, and only a new attempt uses `task.yaml` and `result.json`; the attempt row records which. Phase 5: an old worker command writes to the outbox when its attempt has one, and sends a request to fleetd when not. Phase 6: each run that exists gets 1 idea row and 1 feature row. Phase 7: a worker that runs interactive goes on; exec is the default for new attempts.
- Add 1 table "Where each rule becomes green" to the Migration plan: each requirement id and each failure mode FM-n, with the 1 phase whose gate makes its test green. The answer to question 5 of `system-review-42-migration-and-requirements.md` lists the ones with no gate today. Each gate line of a phase then names its rows. The no-loss invariants I8 to I12 get 1 test each, in the gate of the phase that first uses the invariant.
- The preservation rows 04 W6, L5, L9, W8, W9 and L8 become requirements of Phase 3 with a test in its gate. The worktree reconciler stays in report-only mode until these tests are green.
- Add 1 table of the numbers of the design: the value, its class (a fixed contract, a default with its key in `fleet.toml` or in a profile, or a starting value to tune), and where it is set. The answer to question 8 of the same report has the list.

Places: Summary; Migration plan (each phase, the 2 new tables, "New work for each phase", the paragraph after it); Requirements (verification column where a gate is named); Failure modes; Preservation analysis.

## A31. The design states the rules that only the DDD model has (S43-8)

The fault: the orchestrator decided some rules in the DDD round (D15). They are in `ddd/` and not in the design. The design must be the source.

New content: add a row "D15" to the Amendments table (decisions of the orchestrator in the DDD round; you can reverse each one), and state each rule in its section:

- A request carries an idempotency key (A24 covers it). A refusal carries its reason.
- `reset_at` of a usage limit is optional. When it is absent, fleetd waits until the pool probe reports ok.
- `result.json` has a status from the closed list `completed`, `blocked`, `failed`.
- A worker question goes to the needs-you list after 15 min with no answer from a running sub-orchestrator. This is a starting value. The wait for you has no limit.
- A requeue by you sets the attempt count to 0. A requeue by a review keeps its own counter.
- Supervision and conformance are an operating concern outside the flow of 1 idea.
- Module owners inside the 1 binary: the fleet-run wrapper belongs to worker-profiles; plan validation is a module of ideas with its own rules; the needs-you list belongs to ideas; git has 2 callers, worktree-custody (worktrees and rescue refs) and integration (branches and merges); task-execution and integration change together; docs sync is a `horch` subcommand that fleetd schedules. An `arch_scan` rule enforces the module boundaries.
- A drift that docs sync finds becomes an idea (the design already says so). The worker-profiles module marks a profile unhealthy, and healthy again when the conformance checks pass.

Places: the Amendments table; Architecture and roles; Worker profiles and adapters; Ideas (the question flow); Docs sync and adapter conformance; Requirements (RR-11, RR-12).

## The amendment for each finding

| Finding | Rank | Amendment |
| --- | --- | --- |
| S39-1 | CRITICAL | A23 |
| S39-2 | CRITICAL | A24 |
| S39-3 | HIGH | A24 |
| S39-4 | HIGH | A24 |
| S39-5 | MEDIUM | A23 |
| S39-6 | MEDIUM | A25 |
| S39-7 | LOW, decision D8 | A23 (the work budget). D8 does not change. |
| S39-8 | HIGH | A24 |
| S40-1 | CRITICAL | A26 |
| S40-2 | HIGH | A26 |
| S40-3 | HIGH | A26 |
| S40-4 | HIGH | A26 |
| S40-5 | HIGH | A27 |
| S40-6 | HIGH | A28 |
| S40-7 | HIGH | A27 |
| S40-8 | HIGH | A28 |
| S41-1 | CRITICAL | A30 |
| S41-2 | CRITICAL | A28 |
| S41-3 | HIGH | A27 |
| S41-4 | HIGH | A27 |
| S41-5 | HIGH | A28 |
| S41-6 | HIGH | A30 |
| S41-7 | HIGH | A27 |
| S41-8 | MEDIUM | A28 |
| S42-1 | CRITICAL | A30 |
| S42-2 | HIGH | A30 |
| S42-3 | HIGH | A30 |
| S42-4 | HIGH | A30 |
| S42-5 | HIGH | A30 |
| S42-6 | MEDIUM | A30 (the table of numbers) |
| S42-7 | LOW, decision D16 | No change. Reported to the operator: Phase P comes before the phases that stop the daily stalls. |
| S43-1 | HIGH | A24 |
| S43-2 | CRITICAL | A28 |
| S43-3 | MEDIUM | A29 |
| S43-4 | LOW, decision D8 | A28 (more health guards). D8 does not change. |
| S43-5 | MEDIUM | A28 |
| S43-6 | MEDIUM | A28 |
| S43-7 | MEDIUM | A24 |
| S43-8 | MEDIUM | A31 |

# Systems review round, fix round 2

5 fresh reviewers read the design at commit `c8bf35ac`, after amendments A23 to A31. The reports are the 5 files `docs/specs/fleetd/system-review-*-2.md`. Each finding of the first reports is closed, except S40-8, S42-2 and S42-4. The fixes opened new gaps: 4 CRITICAL (T39-1, T39-2, T41-1, T43-1) and some HIGH. A new finding id is `T<report>-<n>`. Amendments A32 to A36 are the decisions. This is the last fix round: what a third review still finds open goes to the operator.

## A32. Nothing slow runs between 2 heartbeats (T39-1, T39-2)

The fault: part 3 of a pass runs `git worktree add` and `git worktree remove` before the heartbeat. Part 1 of a pass scans each outbox and each worktree with no limit. Each can take more than the hang limit, and then a healthy fleetd is restarted for ever.

New content:

- An effect that part 3 of a pass does itself is short by its nature: start a process through the launcher, signal a process, write or move a small file, `git update-ref` on a local ref. Each other effect is a job. Add the job kinds `worktree_add` and `worktree_remove`. A worktree in `reserved` waits for its `worktree_add` job, and the attempt launches only when the job passed. The general rule, stated 1 time: a git command that reads or writes a working tree, or that uses the network, is always a job.
- The fact scans of part 1 have a budget, like the rows of part 2. fleetd scans the outbox of each live attempt and the request folder; it does not walk the whole tree. Each scan (outboxes, worktree folders, processes) has a cursor in the ledger (`pass_cursors`) and a time budget for 1 pass (a starting value in `fleet.toml`). When the budget ends, the scan stops, the pass goes on with the facts it has, and the next pass continues at the cursor. File watching puts a changed outbox at the front of the order. `git worktree list` is a job.
- The whole pass has a time budget that is lower than the hang limit. fleetd looks at the time between 2 items and ends the pass in order: it commits, does the short effects, and writes the heartbeat. `horch status` shows the age of the oldest fact that waits for a scan.

Places: Ledger (`jobs.kind`, `pass_cursors`); Worktrees (the state table rows `reserved`, `created`, `landed`); Supervision, liveness and reconciliation (the reconcile pass: parts 1 and 3 and their comments, "A pass has a work budget"); Requirements (PL-5); Failure modes (FM-12).

## A33. What a job key means, and each job kind has a valid row (T41-3, T43-3, T40-4, T43-1)

The fault: the rule "a job with the same kind and input key is not run 2 times" also stops the retry of a failed push and the next periodic fetch. A job that was stopped at its time limit never runs again. The job `docs_sync` has no kind in the list, and a job row needs a repository.

New content:

- There are 2 classes of job, and the design gives the class and the input key of each kind in 1 table.
  - A pure job: its result depends only on its inputs. The kinds: a task check (the tip of the task branch and the check), a fast gate and a full gate (the candidate tip and the base tip), a trial merge (the 2 tips). A result of `passed` is used again. A result of `failed` is used again only when the job ran to its end and the check or gate gave the verdict. A job that ended as `abandoned` (stopped at its time limit, or its process was lost) has no verdict and runs again.
  - An effect job: it changes or reads the world outside its inputs. The kinds: `fetch`, `push`, `rebase`, `rescue`, the build of a bundle candidate, `worktree_add`, `worktree_remove`, `docs_sync`, `conformance`. Its result is never used again. Each run is a new row with a run number. At most 1 row of 1 kind and 1 subject is `queued` or `running`. A `failed` push or fetch runs again after its backoff or at its next period.
- The uniqueness rule of the table is: kind, subject, input key and run number.
- The kind list of `jobs` has each kind that the design names, from the phase that adds the table: also `docs_sync` and `conformance`, which Phase 7 starts to use. The repository column can be empty for a job that belongs to no repository. The subject of a `docs_sync` job is the CLI and its version.

Places: Ledger (`jobs`: the kind list, the columns, the uniqueness rule, the comment); Supervision, liveness and reconciliation ("Long work is a job"); Features, landing and integration (push and fetch); Docs sync and adapter conformance; Migration plan (New work for each phase, Phase 7); Failure modes (the row on a failed push).

## A34. Each new state has an exit (T41-1, T40-2, T41-2, T40-1, T40-3, S40-8)

The fault: 5 paths that A26 to A28 opened have no exit.

New content:

- A bundle in `passed` whose ref move fails its compare-and-set goes to `dropped`, and its members go back to `ready`. Add the bundle state `landed`: the move of the feature ref was observed. fleetd does not queue a rebase of a feature branch while 1 of its bundles is in `gating` or in `passed`.
- fleetd owns the candidate refs `refs/horch/bundle/<id>`. It deletes the ref of a bundle that is `landed`, `failed` or `dropped` when no child bundle of it is still in `building`, `gating` or `passed`. It reads the ref store again to verify the deletion and tries again on the next pass when it failed. `horch status` shows the count of bundle refs.
- A resolver attempt and a fix attempt of a task that is `done` are attempts of the task branch. Each has a lease and is watched by the stall ladder like an attempt of a `running` task. I1 says: each live attempt has exactly 1 lease. A failure of such an attempt does not move the task. It adds 1 to a counter on the `branches` row, and fleetd starts the next one after the backoff. At `max_attempts` of the profile, the branch goes to the needs-you list. `horch task requeue <task>` then sets the counter to 0 and starts 1 more. `horch branch abandon <branch>` is the other exit.
- Each task state that is not terminal has an exit to `cancelled`: a replan that replaces the task, `horch task cancel`, or the cancel or abandon of its idea or feature. fleetd first stops the process group of a live attempt, then moves the task, then handles its branch by the rule of A27 (a rescue ref, then `abandoned`). The cancel of a judged task also cancels its judge task. A plan change that would leave a judgment task with no judge task is refused by plan validation.
- The feature state `open` has an exit to `merged`: git shows the changes of the feature tip in the primary branch by content. Each idea in flight then moves to a new feature by the rule "A merge with ideas in flight". Add this exit to the `open` row of the feature state table and to the allowed transitions.

Places: Ledger (`bundles.state`, `branches`: the counter; I1); Features, landing and integration (the feature state table, the landing state table, "The bundle", the paragraph on resolver and fix attempts); Supervision, liveness and reconciliation (the task state table, the stall ladder rung 6).

## A35. An operator command needs a thing that a worker does not have (T42-1, T43-2)

The fault: the command channel checks the OS user. A worker runs as the same user. So the check cannot tell you from a worker, and the refusal inside the `horch` command does not protect the protocol.

New content:

- fleetd decides at the server. Each request kind has a class: `operator` or `worker`. The design lists the class of each kind. The kinds of a worker (`progress`, `result`, `ask`) go through the outbox of its attempt and need no channel.
- The operator token. fleetd makes a random token file in the state root at install, with mode 0600. The `horch` command reads it and sends it with each `operator` request, on the channel and in a request file. fleetd refuses an `operator` request that has no valid token, and records the refusal.
- The sandbox is the wall. The sandbox profile of each worker denies 3 things: to read the token file, to connect to the command channel, and to write into the request folder. Profile validation refuses a profile whose sandbox rules allow 1 of them. The conformance suite has 1 check for it for each adapter.
- A second check for the live channel. The platform layer gives the process of the peer. fleetd refuses an `operator` request from a process that belongs to the process group or job object of a live attempt, or that descends from its wrapper.
- State the limit in the design in plain words: on an OS where a CLI has no sandbox (the platform module returns "not supported"), a worker has the rights of the user, and the token does not stop a hostile worker. `horch status` and plan validation mark such a profile as "no sandbox". The second check still applies.
- The `requests` row records the class of the request and how it was authorized.
- The gate of Phase 5 gets 1 test: from inside a worker sandbox, a raw request on the channel and a request file with the right owner are both refused, for each `operator` kind.

Places: Requirements (SR-5, a new SR requirement for the token and the sandbox rule); Architecture and roles (the command channel, "Only you can send a command"); Ledger (`requests`); Worker profiles and adapters (the sandbox row of the profile, profile validation); Docs sync and adapter conformance (the checks for each adapter); Migration plan (Phase 5 and its row in the 2 tables); Failure modes (the row on a planted command).

## A36. The migration text agrees with itself (S42-2, S42-4)

The fault: the architecture still says that agents never touch the ledger, and Phases 1 to 4 need direct writes. FM-9 is marked green in Phase 4, and its judge path comes in Phase 6.

New content:

- Each sentence that says that only fleetd writes the ledger, or that an agent or a `horch` command never writes it, says "from Phase 5". The table "What a release does to a run in flight" says for Phases 1 to 4: a worker command and a `horch` command write the ledger directly, through `transition()`.
- In the table "Where each rule becomes green", FM-9 and RR-3 become green in Phase 6. The gate of Phase 4 names a part test: "FM-9 for a code kind and for the structural check". The gate line of Phase 4 and the row of FM-9 say the same.

Places: Architecture and roles (fleetd, the command channel, the roles table); Ledger (the writer rule); Migration plan (Phase 4, Phase 6, the 3 tables); Failure modes (FM-9).

## The amendment for each finding of the second reports

| Finding | Rank | Amendment |
| --- | --- | --- |
| T39-1 | CRITICAL | A32 |
| T39-2 | CRITICAL | A32 |
| T40-1 | HIGH | A34 |
| T40-2 | HIGH | A34 |
| T40-3 | HIGH | A34 |
| T40-4 | HIGH | A33 |
| S40-8 (still open) | HIGH | A34 |
| T41-1 | CRITICAL | A34 |
| T41-2 | HIGH | A34 |
| T41-3 | HIGH | A33 |
| T42-1 | HIGH | A35 |
| S42-2 (still open) | HIGH | A36 |
| S42-4 (still open) | HIGH | A36 |
| T43-1 | CRITICAL | A33 |
| T43-2 | HIGH | A35 |
| T43-3 | HIGH | A33 |

# Systems review round, closing fix

5 fresh reviewers read the design at commit `5327458b`, after amendments A32 to A36. The reports are the 5 files `docs/specs/fleetd/system-review-*-3.md`. Each verdict is PASS. No CRITICAL finding is open, and there is no new one. 3 HIGH findings are open: S42-2, and T42-1 with T43-2, which are the same fault. Amendment A37 closes them.

## A37. A worker never runs without its sandbox, and 2 sentences agree (T42-1, T43-2, S42-2)

The fault of T42-1 and T43-2: A35 accepted a profile with no sandbox on an OS where the sandbox of a CLI is not supported. Such a worker can read the operator token and write a request file that fleetd accepts. This also contradicts preservation row 06 S9, which the design keeps: a sandboxed persona does not start on a host that cannot sandbox it. The orchestrator made that call in A35, and it was wrong. The behavior of today wins.

New content:

- Each worker profile and each sub-orchestrator profile has a sandbox. A worker never runs without it. Row 06 S9 stays as it is.
- When the platform module returns "not supported" for the sandbox of a CLI on an OS, profile validation refuses the profile on that OS, with the reason. Plan validation refuses a plan that names the profile, and the fallback rule of the roster applies when the fallback has a sandbox on that OS. `horch status` shows the profile as "not available on this OS: no sandbox".
- Remove the paragraph "The limit, in plain words" and each sentence that accepts or marks a profile as "no sandbox". Remove the same words from SR-8 and from the conformance check when they are there.
- Say the consequence 1 time, in the table "What each OS provides": on Windows the sandbox of Claude Code is not supported today, so no Claude Code worker runs on Windows until its adapter has a sandbox there. fleetd, the ledger and each CLI that has a sandbox on Windows run. Add 1 row to Open questions: "Do you want workers with no sandbox on an OS that cannot sandbox a CLI? Default: no (06 S9). A yes gives such a worker the rights of the user, and it could send an operator command."
- With this rule, the 3 denials of the sandbox (the token file, the command channel, the request folder) hold for each worker. The second check on the live channel stays as a second layer.

The fault of S42-2: the row FM-24 of the failure modes still says "Single writer; everything else opens read-only" with no phase.

New content:

- In the row FM-24, say: from Phase 5 there is a single writer, and each other process opens the ledger read-only. In Phases 1 to 4 each writer calls `transition()`, and SQLite gives 1 writer at a time. Search the design 1 more time for a sentence that states the single writer with no phase, and correct it in the same way.

1 small agreement (a note of the third review of the state machines, T40-1): rule 2 of the Ledger section says that a lease expiry moves "the task" to `stalled`. For a resolver attempt or a fix attempt of a task that is `done`, say: the lease expiry ends that attempt and adds 1 to `repair_attempts` of the branch, and the task does not move.

Places: Requirements (SR-8); Architecture and roles (the command channel paragraphs, "What each OS provides"); Ledger (rule 2); Worker profiles and adapters (the paragraph "The sandbox of a profile denies the operator path"); Docs sync and adapter conformance (the check "Operator boundary"); Failure modes (FM-24); Open questions; the Amendments table (1 row "Systems review round, closing fix: A37").
