# Changelog

<!-- On a merge to main with entries under "## Unreleased", the auto-release workflow renames it to "## vX.Y.Z - YYYY-MM-DD", adds a new empty "## Unreleased" above it, and publishes the section as the release notes. -->

## Unreleased

- Worker reports and task-ledger wakes remain queued until `horch inbox`
  acknowledges them. Missing orchestrator registrations no longer drop
  mail, and the courier keeps retrying while the orchestrator is available.
  An expired Claude login pauses delivery and sends one operator notification.
- `horch inbox` shows worker state and unanswered questions. `horch peek
  <role> --lines <n>` reads a worker's recent pane output without typing.
- The watchdog recognizes waits for reviews, unanswered questions and
  background commands, with deadlines and visible reasons. A dead agent
  process is still detected immediately. The task ledger upgrades to schema 2;
  older horch binaries refuse the upgraded database, so fleets sharing it
  need the updated binary.
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
- A Codex worker runs only its own `horch ledger` subcommands outside its
  sandbox (`claim`, `worktree`, `progress`, `status`, `show`, `steps`,
  `finish`, `done`); `add`, `add-map` and `reopen` are refused. On Windows,
  horch removes the old broad `horch ledger` rule from the shared codex rules
  file. Every worker briefing tells the worker to run each `horch ledger`
  command alone and carries the network-safety rule. `horch teammates --check`
  fails for a Codex teammate without these lines. An overlay of
  `_base/fleet-worker.md` must merge the new ledger lines and the
  `== Network ==` section.

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
