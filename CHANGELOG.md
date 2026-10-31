# Changelog

<!-- On a merge to main with entries under "## Unreleased", the auto-release workflow renames it to "## vX.Y.Z - YYYY-MM-DD", adds a new empty "## Unreleased" above it, and publishes the section as the release notes. -->

## Unreleased

- horch now removes stale state whose owner is gone: old spool files, dead
  channel sockets, the registries and mail of closed workspaces, finished
  skill bundles and Codex homes, and killed temp files. Records of closed
  panes end as `PaneVanished`. `horch fleet` and `horch spawn` sweep at most
  once an hour; `horch doctor` counts the stale state, and
  `horch doctor --clean` removes it.
- Worker reports and task-ledger wakes remain queued until `horch inbox`
  acknowledges them. Missing orchestrator registrations no longer drop
  mail, and the courier keeps retrying while the orchestrator is available.
  An expired Claude login pauses delivery and sends one operator notification.
- `horch inbox` shows worker state and unanswered questions. `horch peek
  <role> --lines <n>` reads a worker's recent pane output without typing.
- The watchdog recognizes waits for reviews, unanswered questions and
  background commands, with deadlines and visible reasons. A dead agent
  process is still detected immediately.
- `horch ledger worktree new --from <ref>`, `scratch`, and `drop` support
  alternate branches and disposable work. Drop refuses dirty or unique work;
  `--keep-branch` retains commits when removing a clean worktree.
- `horch sessions` and `horch context` default to the caller's workspace.
  Both accept `--all-workspaces`, `--live` and `--role`; closed panes are
  marked, and multiline session summaries remain readable.
- Codex session discovery matches a unique launch token, so workers started
  together do not share the wrong session or transcript.
- A worker's handoff note can schedule compaction automatically. Ledger
  notes and progress now run the context check, and compaction completion
  counts as progress.
- Fleet startup prints its version and checks message acknowledgement.
  Offline stale-binary checks, live-orchestrator warnings and host-load
  notes make conflicting work visible. Doctor also tests delivery, Docker,
  MCP command availability and sandbox support, and warns about proxy hosts.
- Quota parsers cover captured replies from current Claude Code, Codex and
  Antigravity CLIs. Unknown readings show their reason in `horch quota`.
- `horch note --fact` shares bounded, deduplicated project facts with later
  worker briefs and claims. Briefs name the host and its command-line tools.
- Worker and orchestrator instructions now cover reversible questions,
  background waits, exact permission denials, one completion report and
  delegated verification. Orchestrators retain decisions and get a command
  card on resume; Claude workers pre-allow common read-only commands.
- A Codex worker's exec policy allows only its own `horch ledger`
  subcommands outside its sandbox (`claim`, `worktree`, `progress`, `status`,
  `show`, `steps`, `finish`, `done`); `add`, `add-map` and `reopen` are
  refused. A Codex pane's private codex home, which holds these rules, is
  now in horch's data folder, so a sandboxed worker cannot edit it. `finish`
  runs the check command of a Codex worker inside `codex sandbox`, in the
  same `fleet_write` profile as its pane: the check can write only the
  worktree and horch's state folder, and it keeps the task's check timeout.
  Only the git, push and `gh` steps of `finish` run outside the sandbox.
  On Windows, horch removes the old broad
  `horch ledger` worker rule from the shared codex rules file. That file
  serves every codex session, so after a Codex orchestrator starts, its
  broad `horch ledger` rule applies to Windows workers too. Every worker
  briefing tells the worker to run each `horch ledger` command alone and
  carries the network-safety rule. `horch teammates --check` fails for a
  Codex teammate without these lines, and for a Codex pane with the network
  whose briefing says it has none. The Codex orchestrator briefing no longer
  says its sandbox has no network. An overlay of `_base/fleet-worker.md`
  must merge the new ledger lines and the `== Network ==` section.
- The task ledger upgrades to schema 6. Older horch binaries refuse the
  upgraded database, so every fleet that shares `~/.horch/ledger.sqlite3`
  needs the updated binary. The upgrade keeps every existing row.
- Parked worktrees now reach an operator decision. `horch worktree triage`
  (`--json` for scripts) lists each worktree under `.worktrees/` that no live
  worker holds and no step settles, with its commits ahead and behind,
  diffstat, uncommitted files, conflicts and disk. `horch worktree triage
  apply <file|->` runs the decisions of a reply table (`LAND`, `PR`,
  `FINISH`, `ARCHIVE`, `KEEP`, `DELETE`) and refuses the whole reply when
  1 line is bad. No decision forces git or loses a commit: `ARCHIVE` and
  `DELETE` keep the tip under `refs/horch/archive/` or `refs/horch/trash/`.
  `horch worktree restore <name>` makes the folder again from that ref, and
  `horch ledger gc` deletes trash refs older than 30 days. `horch fleet`
  starts the `worktree-steward` teammate beside the orchestrator when triage
  finds 1 or more parked worktrees.
- `horch ledger run close` and `run close --abandon` refuse while a parked
  worktree of the run is on disk with no decision. `--abandon` still cancels
  the open tasks, but the run stays active until `horch worktree triage
  apply` decides each worktree.
- `horch ledger status` prints `Parked worktrees: N (X GB, measured <age>
  ago)`. The size is the one `horch worktree triage` stored, so `status`
  walks no folder. Rows triage did not measure add `; K not measured`, and
  with no measurement the line says `(disk not measured; run horch worktree
  triage)`.
- In the worker's own pane, `horch ledger finish` runs its steps detached.
  It prints `finish runs (PID n); horch tells you the result. End your turn
  now`, exits 0, and mails the result to the worker when the steps end. A
  second finish while that run is alive starts nothing. `--wait`, `--json`
  and a call from any other pane run the steps in the foreground as before.
- The finish check has a timeout: 20 minutes by default, set per task with
  `horch ledger add --check-timeout <seconds>`. A check past it fails and
  its whole process group is stopped. `land` runs the check outside the land
  lock, and a land that waits names the holder every 30 s. A failed push is
  retried by the next `finish`, and a commit after the land runs the check,
  review and land again.
- `horch ledger add --shared` tasks run 1 at a time. A shared claim refuses
  unless the main checkout is on the run branch and prints the `git switch`
  line; land refuses a task with no commit since its claim.
- Code-sweep rounds belong to the task, so a replacement worker reuses a
  finished round. `horch ledger finish --rereview` starts a new round for
  the same tree, and the orchestrator can rule that 1 finding does not block
  with `horch ledger waive`. A reviewer that cannot run hands the round to
  the other reviewer persona. Complexity-sweep no longer sends `.rs` or `.gd`
  files to the TypeScript analyser.
- Task-ledger git runs with repository hooks off (`core.hooksPath=/dev/null`)
  and `GIT_CONFIG_NOSYSTEM=1`, as worktree landing already did. Ledger push,
  rebase, merge and `worktree add` no longer run repo hooks. git, the check,
  `gh` and the complexity tool get no inherited `GIT_DIR` or
  `GIT_WORK_TREE`, and git never prompts.
- `horch ledger run close [--abandon]` ends a run, and `run new
  --max-attempts <n>` sets how often a task is tried (default 3). After the
  last attempt the task is held with 1 `QUESTION` wake; `horch ledger
  requeue` releases it. `horch ledger unblock <task> --from <blocker>` drops
  1 dependency, and `status` names tasks that wait for a cancelled task.
  `horch ledger cancel --stop` also stops the worker. `finish`, land and
  `done` refuse a cancelled task. `horch spawn --run <run>` names the run a
  worker joins; ledger commands use the run of the caller's workspace, and
  `horch assign` refuses only in the workspace that owns an active run.
- A failed or refused spawn ends its worker row as `launch_failed`. The
  watchdog ends pre-claim rows whose process is gone, wakes once for a live
  worker that does not claim in 10 minutes, and `ledger status` marks stale
  rows `STALE`.
- `horch ledger status` (text and `--json`) shows the watchdog's version and
  binary, and ends the line with `OLD BINARY: ...` when the watchdog runs
  another build. `horch install` and the next ledger command stop a watchdog
  of another build by its recorded PID and start time, then start one on the
  current binary.
- A herdr `working` state with no timer and no transcript counts as a
  check-in for up to 2 times the stall rounds; then the worker is `suspect`
  with 1 wake, and dead at the next check. Each check note names the signals
  the watchdog could not read. Detached horch processes no longer inherit
  the starting pane's identity, so a wake goes to the run's own workspace.
- `horch doctor` warns when `codex-reviewer` or `architect-reviewer` cannot
  run on the host (not in the roster, or its binary is missing or broken),
  and for an offered harness with no timer format and no transcript lookup,
  whose workers the watchdog cannot see progress.
- `horch fleet` and `horch doctor` warn while herdr resumes agents on
  restore, and name `herdr update --handoff`. A pane that herdr resumed for
  a registered worker gets that worker's identity back.
- Inside a fleet, `horch inbox` acknowledges the orchestrator's mail only in
  the registered orchestrator pane. Another pane gets the mail as with
  `--peek`, and 1 stderr line that says so. A worker's `horch inbox` prints
  and acknowledges the mail to its own role. horch's own notices to a worker,
  such as reviews-in, stay queued until the worker acts on them.
- horch refuses a registry or mail folder that another user owns or that
  others can write, and names the path. New registry, mail, spool and compact
  folders are mode 0700 and their files 0600. An old temp registry is copied
  once into the state folder, and one that others can write is not copied.
- `horch telemetry ensure` no longer waits a fixed 5 s for the collector it
  starts, so a slow start on a loaded host no longer opens a second collector
  workspace. A later `ensure` reports a start in progress and starts no
  other. `ensure`, `horch telemetry` and `horch quota` report a failed start
  on stderr, and the next `ensure` starts the collector again.
- When `horch telemetry ensure` (and so `horch install`) replaces a collector
  that runs an old binary, it now says why: the binary was rebuilt or
  reinstalled at the same path, or the collector ran another path. It no
  longer prints the same path twice.
- A failed `horch fleet` or `horch orchestration` start leaves no workspace
  and no live orchestrator record. With `HORCH_STATE_DIR` set and
  `HORCH_LEDGER_DB` unset, `fleet`, `spawn` and `doctor` warn that the task
  ledger is still shared; with `HORCH_LEDGER_DB` set, the orchestrator pane
  gets it too.
- Agent CLI updates run once at a time per CLI and skip a CLI checked less
  than 24 hours ago (`horch update` always runs). An updater past the timeout
  is left running, not killed, and later runs skip it until it ends.
- `horch context --over` shows how long ago each compaction request was
  made, marks a request with no `COMPACT-READY` after 20 minutes as
  `request-stale`, and lists a failed compaction with its step. `--request`
  asks again on a stale or failed row, at most 2 times in one cycle.
- `horch done` and `horch worktree list` skip a worktree whose folder is
  gone, with a `skipped worktree <path> (...)` note on stderr, and report the
  rest. They report only worktrees under the repo's `.worktrees/` folder that
  the task ledger does not record, so your own checkouts and other workers'
  ledger worktrees are no longer `UNREGISTERED`.
- `just verify` and CI run `cargo doc` with warnings as errors, and clippy
  fails on an `unsafe` block without a `// SAFETY:` note
  (`undocumented_unsafe_blocks`).

## v0.4.0 - 2026-10-29

- Fleet role registrations, briefs and launch records now live in horch's
  state folder, so clearing temporary files no longer breaks a running
  fleet. Fleets started before the upgrade still resolve their old records,
  and worker panes inherit the fleet's state-folder setting.
- `horch done` and `horch ledger done` record completion before pushing the
  report and limit all pushes together to 20 seconds. Reports stay queued
  for the courier when the orchestrator is slow or still starting, so a
  worker can finish within its command timeout.
- A ledger claim now gives a usable absolute ticket path when the ticket
  is absent from the worker's worktree, including tickets in ignored
  folders. Queue wait reasons now name the task holding a shared file.
- `horch inbox` now prints the role list first, then pending messages, and
  ends with `N messages acknowledged`. It used to print messages first.
  `horch inbox --roles` prints only the roles and leaves mail pending.
  `horch inbox --peek` prints mail and leaves it pending. Mail is for the
  orchestrator only: a worker gets only the role list. A script that read
  the role list from `horch inbox` should use `--roles`. A script that must
  not acknowledge mail should use `--peek`. The reminder other commands print
  now reads "N messages are not acknowledged", without "worker". If herdr
  does not answer for 5 seconds, `inbox` stops that herdr call, marks the
  pane states unknown, and still prints mail.

## v0.3.1 - 2026-10-28

- `horch fleet` and `horch orchestration` install a newer multi-herdr
  release before the fleet starts, then restart on it. `--no-update` or
  `HORCH_NO_UPDATE=1` skips it; a failed update starts the fleet on the
  version you have.
- Codex workers and the Codex orchestrator can write anywhere in the folder
  they start in, `.git` included, so they can commit and make worktrees.
  Outside that folder everything is read-only, `/tmp` too, and a worker can
  stop only processes it started. They now have the network and live web
  search. `--approve-for-me` is gone: a command the sandbox refuses fails.

## v0.3.0 - 2026-10-28

- The task ledger: `horch ledger` is now a queue the orchestrator and its
  workers share (`~/.horch/ledger.sqlite3`). The orchestrator queues tickets
  (`run new`, `add-map`, `plan-workers`) and spawns workers with no task;
  each worker claims its own, and cannot finish until `horch ledger finish`
  has run the review and complexity sweeps, landed the work on the run
  branch, updated the run's pull request and safely removed its worktree.
- Workers check in every 30 s, and a watchdog replaces a worker that stops
  making progress. A worker is stopped only by the process IDs the ledger
  recorded (`horch ledger stop`); nothing stops processes by name any more.
- `horch ledger import` brings in the old session ledgers, and
  `horch ledger unfinished` lists past work that never landed.
- The old ledger plumbing moved to `horch ledger legacy`. In a ledger run,
  `horch assign` refuses: queue work with `horch ledger add` instead.

## v0.2.1 - 2026-10-27

- The agent CLI update step waits out a 7-day release cooldown: a CLI updates
  only once its newest release is at least 7 days old, and the package
  managers its updater calls (npm, pnpm, yarn, uv, pip) get the same cutoff.
  Antigravity and Prime, whose release dates horch cannot read, no longer
  update automatically. `HORCH_UPDATE_COOLDOWN_DAYS` sets the cooldown.

## v0.2.0 - 2026-10-25

- Codex workers and the Codex orchestrator start with `--approve-for-me`:
  a request to act outside the sandbox goes to Codex's automatic reviewer
  instead of failing. Plan panes stay read-only with the same reviewer.
- Worker reports stay queued until `horch inbox` acknowledges them. The
  courier resends them to an idle orchestrator.
- Prebuilt releases for macOS, Linux and Windows, with `horch self-update`
  and a boot-time notice when a newer release exists.
- `horch tell` keeps long messages in a folder per workspace, so one fleet
  can no longer read another fleet's messages.
