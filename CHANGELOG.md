# Changelog

<!-- On a merge to main with entries under "## Unreleased", the auto-release workflow renames it to "## vX.Y.Z - YYYY-MM-DD", adds a new empty "## Unreleased" above it, and publishes the section as the release notes. -->

## Unreleased

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
- `horch fleet` and `horch orchestration` install a newer multi-herdr
  release before the fleet starts, then restart on it. `--no-update` or
  `HORCH_NO_UPDATE=1` skips it; a failed update starts the fleet on the
  version you have.
- Codex workers and the Codex orchestrator can write anywhere in the folder
  they start in, but `.git`, `.codex` and `.aws` are read-only. A Codex
  worker commits only with the new `horch ledger commit`, in a claimed ledger
  task that has a worktree, and `horch ledger finish` lands that work.
  `horch ledger commit` needs git 2.41 or later. Codex workers and the Codex
  orchestrator cannot run `horch worktree new` or `horch worktree land`: give
  that work, and every other git write, to a Claude worker. Outside the
  folder everything is read-only, `/tmp` too, and a worker can stop only
  processes it started. They now have the network and live web search.
  `--approve-for-me` is gone: a command the sandbox refuses fails.
- `horch ledger finish` checks a ledger worktree's `.git` file before each
  step and runs its git there with that verified git dir: no hook, fsmonitor
  or filter program runs in the worktree. Its push and merge in the main
  checkout keep the repository's hooks, so git-lfs still uploads in
  `pre-push`.
- `horch ledger finish` refuses the check of a worker whose harness the
  ledger does not record. `horch ledger worker new --harness <agent>` records
  it, and `horch spawn` stops and closes the pane when it cannot.

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
