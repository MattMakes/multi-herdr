# 37: The fetch job: each period is a new run, remote_tips records each fetched tip, and a failed fetch shows since when the tip is stale

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::adapters::exec_fetch fleetd::integration::application::remote && cargo test -p horch-core --test fetch_job && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/adapters/exec_fetch.rs`, `crates/horch-core/src/fleetd/integration/application/remote.rs`, `crates/horch-core/tests/fetch_job.rs`

Spec: `docs/specs/fleetd/design.md`, "Between features" (Remote git state is level-triggered), "The kinds of job" (`fetch`: effect), "Push and pull request results are facts on the feature row" (`pr_state` from the fetch with `pr = gh`), F11.

## Goal

1 `fetch` job runs for each repository every `fetch_period_s`, also after a failed run. A passed run writes the fetched tip of the primary branch and of each feature branch in `remote_tips` and clears `stale_since`. A failed run sets `stale_since` once. With `pr = gh`, the fetch reads the state of the pull request of each feature into `pr_state`. With `remote = none`, no fetch runs.

## Requirements

- 2 fetch periods in a row make 2 fetch runs (a test of the gate).
- F11: the git network call has its bound (3 min), `GIT_TERMINAL_PROMPT=0`, no hooks, and never retries inside a run.
- A closed pull request that is not merged puts 1 needs-you item: "open it again, or run `horch feature abandon`".

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The git call goes through `crates/horch-core/src/vcs/network.rs` (`output`, `bound`). The pull request read is `gh pr view --json state` through the launcher; a missing `gh` gives the cause in the job result.
- `application/remote.rs` holds the period rule and the observation, also for the push job of ticket 38.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first (a local bare repository as the remote): `fetch_records_the_tips_in_remote_tips`, `2_periods_make_2_fetch_runs`, `failed_fetch_sets_stale_since_and_the_next_period_runs_again`, `remote_none_runs_no_fetch`, `closed_unmerged_pull_request_opens_1_item` (a fake `gh` on PATH).
2. Write the executor and the period rule.
3. Run the Check. Commit the exact files.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. Remote tips are fresh within 1 fetch period, and a failed fetch is visible.
