# 05: The worktree_add and worktree_list jobs, and the start and wait of a job from a command

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test worktree_jobs --test fleetd_jobs && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/ports.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/run_now.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`, `crates/horch-core/tests/worktree_jobs.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("Long work is a job. A pass never waits for it."; the table "The kinds of job", rows `worktree_add`, `worktree_list` and `rescue`), "Worktrees: leased, reconciled, never lost" ("The reconciler compares three views"), "Features, landing and integration" ("1 git writer for each repository"), "Migration plan" item 5 ("until Phase 4, `horch spawn` queues the `worktree_add` job and waits for it outside the pass").

## Goal

A `worktree_add` job makes 1 worktree outside each pass and each transaction. A `worktree_list` job reads `git worktree list --porcelain` of 1 repository and the facts of each tree. A `horch` command can queue a job, start it through the launcher when no pass did, and wait for its end with no transaction open. At most 1 job that changes git state runs for 1 repository at a time.

## Requirements

- **FR-11** (the effect): the executors of this ticket hold the only `git worktree add` and `git worktree list` calls of the custody (ticket 17 makes it a fitness rule).
- **RR-10** (the third view): the `worktree_list` job, with prunable and locked entries.
- **FM-12** (kept): `git worktree add` and `list` are jobs, never work inside a pass.
- The time limit of a `rescue` job (Q25).
- The fleetd git rule (review F4 of T-607). This job runs git with the rights of fleetd and with no sandbox. So git must start no hook and no program of the project. Each git command of the job:
  - runs with `-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`, `-c commit.gpgSign=false` and `-c tag.gpgSign=false`, and with `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` in its environment;
  - first reads `git config --list --show-origin`. A key that starts a program and comes from the repository or the worktree config (not from the global or system config of the user) stops the job: it ends `failed` with the key in its reason, and changes nothing. The keys are `core.fsmonitor`, `core.sshCommand`, `core.askPass`, `core.editor`, `sequence.editor`, `credential.helper`, `gpg.program`, `diff.external`, and each `filter.<name>.clean`, `filter.<name>.smudge`, `filter.<name>.process`, `merge.<name>.driver` and `diff.<name>.textconv`.
  - For this ticket: `git worktree add` runs the `post-checkout` hook and a checkout filter, and `git status` of `worktree_list` runs `core.fsmonitor`. Write the rule in `adapters/git.rs` (`CliWorktreeGit`) as 1 function that each git call of the gateway uses, so tickets 06 and 07 get it with no new code.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q1, Q11, Q12, Q22 and Q25 apply here).
- The jobs of Phase 1: the kinds and their input keys in `crates/horch-core/src/fleetd/fleet_runtime/domain/jobs.rs` (`KINDS`; `worktree_add`, `worktree_remove`, `worktree_list` and `rescue` have `executor_phase: 3`); `queue`, `observe`, `JobRunner::start`, `run_job` and `time_limit_s` in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`; the rows in `fleet_runtime/adapters/jobs_store.rs`. Today `run_job` runs `task_check` and gives each other kind `jobs::no_executor`.
- Add to `run_job`: `Kind::WorktreeAdd`, `Kind::WorktreeList`, `Kind::Rescue` and `Kind::WorktreeRemove` call `worktree_custody::adapters::executors::run(conn, &row)`. In this ticket `executors::run` runs `worktree_add` and `worktree_list`, and returns `jobs::no_executor(kind)` for `rescue` and `worktree_remove` (tickets 06 and 07 fill them in this file). Add `Kind::Rescue => Some(limits.rescue)` to `time_limit_s` (the key of ticket 04).
- The `worktree_add` executor, in `adapters/git.rs`. The subject is the worktree id. The input key is the path, the branch and the base tip. It reads the `worktrees` row (ticket 03 store) and runs, in the project root of the row:
  1. If the folder exists: prune it only when it holds no work. "No work" is: the folder is empty; or it is a worktree whose branch is the branch of the row, whose HEAD is the base tip, and whose `git status --porcelain --untracked-files=all` is empty. Run `git worktree prune` for a stale entry of the path. Any other folder is kept: the job ends `failed` with the reason "the folder holds files or commits", and ticket 09 adopts the folder as an orphan. A fact that git cannot give is not "no work".
  2. `git worktree add -b <branch> <path> <base tip>`. A branch that exists already fails the job and keeps the branch.
  3. The result JSON: the path, the branch, the HEAD.
  The row stays `reserved`. The pass, or the command of Q1, observes the passed job and moves the row (tickets 08 and 09).
- The `worktree_list` executor, in `adapters/git.rs`. The subject is the repository (the project root). The input key is the number of the period on the fleet clock (`clock / worktree_list_period_s`), so 1 run is made for each period. It runs `git worktree list --porcelain` (with `prunable` and `locked` entries), and for each entry and each row of the repository that is not `removed`: the HEAD, the dirty flag (`git status --porcelain --untracked-files=all` is not empty) and the size of the folder in bytes. The result JSON holds them. It changes nothing.
- The driven port of these git calls, `WorktreeGitGateway` (`ddd/08-code/worktree-custody/design.md` §4), is a trait in `application/ports.rs`, so tests of tickets 08 and 09 use a fake. `CliWorktreeGit` in `adapters/git.rs` implements it with `crate::taskledger::git::Git`.
- `adapters/run_now.rs`: `run_now(conn, job_id, …) -> Outcome` for a `horch` command (Q1). It never holds a transaction while it waits.
  1. If the run is `queued` and its repository has no other git-writing job `running` (Q12): start it with the start code of the pass (`JobRunner::start` through the launcher, and the move `queued` -> `running` through `transition()`). The move is a compare-and-set: when the pass started it first, the command does not start a second process.
  2. Each 100 ms: read the row. Call `jobs::observe` for that 1 run in a short transaction, so a run whose process is gone, or that is past its deadline, ends `abandoned` the same way as in a pass.
  3. Return the outcome of the ended run.
- 1 git writer for each repository (Q12): add a function in `application/jobs.rs`, for example `git_writer_busy(conn, repo) -> bool`, true when a run of `worktree_add`, `worktree_remove` or `rescue` of `repo` is `running`. `run_now` uses it. Ticket 10 uses it in the pass.
- Each test makes its own temporary git repository and ledger (`HORCH_LEDGER_DB` in a temporary folder).

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_jobs.rs`. Each fails before the code exists:
   - `worktree_add_job_makes_the_tree_on_its_branch_at_the_base_tip`.
   - `worktree_add_job_prunes_an_empty_partial_folder_and_runs_again`.
   - `worktree_add_job_keeps_a_folder_with_work_and_fails`: 3 cases, a folder with a modified file, with an untracked file, and with a commit that is not on the base. After the job, each file and the commit are still there.
   - `worktree_add_job_with_an_existing_branch_fails_and_keeps_the_branch`.
   - `worktree_list_job_reports_each_entry_with_prunable_and_locked`.
   - `worktree_list_job_reports_head_dirty_and_size_of_each_row_tree`.
   - `run_now_starts_a_queued_job_once_when_the_pass_starts_it_too`.
   - `run_now_ends_a_job_whose_process_is_gone_as_abandoned`.
   - `second_git_writing_job_of_1_repo_waits_for_the_first`.
   - `rescue_job_has_the_time_limit_of_job_limit_s_rescue`.
   - `worktree_jobs_run_no_hook_or_program_of_the_project`: a repository with a `post-checkout` hook that writes a marker file, and a test with `core.fsmonitor` set in the repository config to a script that writes a second marker. The `worktree_add` job ends `failed` with `core.fsmonitor` in its reason; with the key removed it passes. The `worktree_list` job runs. No marker exists after each job.
2. Write the code. Run the Check. Commit.

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

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes. A `worktree_add` job makes a tree outside each transaction and never removes a folder with work, a `worktree_list` job reports the 3 facts of each tree, and a command can run a job to its end with no transaction open.
