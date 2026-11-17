# 06: The rescue job: a rescue ref under refs/horch/rescue/, read back and verified, never overwritten

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test worktree_rescue && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/domain/rescue.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/rescue.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`, `crates/horch-core/tests/worktree_rescue.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" ("Rescue before any cleanup", the shell block and the manifest; "Built on what horch has"), "The kinds of job" (row `rescue`), RR-9, RR-16, FM-25, FM-27; `docs/specs/fleetd/preservation-04-behavior.md` row W8; `ddd/08-code/worktree-custody/design.md` §3 (`RescueWorktree`) and §9.

## Goal

A `rescue` job saves the full state of 1 worktree (its commits, its modified files and its untracked files) on a new ref under `refs/horch/rescue/`. It never touches the branch, the index or the files of the tree. It reads the ref back and shows that the ref holds exactly the files on disk. Only then is the tree `rescued`. A rescue that fails its verification keeps the tree and opens 1 needs-you item.

## Requirements

- **RR-9**: no worktree is removed until its work is on a rescue ref that is read back and shown to hold it. This ticket gives the verified ref; ticket 07 gives the guard of the removal.
- **RR-16** (rescue refs): a rescue ref only moves forward; a second rescue of 1 tree does not overwrite the first ref; each ref write is read back.
- **FM-25** (the save): a worker killed with dirty and untracked files loses nothing.
- The rule of the orchestrator for the time limit (Q25): a rescue job that reaches its limit is never read as `rescued`. The tree stays, the job runs again by the rule of `abandoned` runs (`job_backoff_s`), and at `abandoned_run_limit` a needs-you item opens. No tree is removed without a verified rescue.
- The fleetd git rule (review F4 of T-607). This job runs git with the rights of fleetd and with no sandbox. So git must start no hook and no program of the project. Each git command of the job:
  - runs with `-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`, `-c commit.gpgSign=false` and `-c tag.gpgSign=false`, and with `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` in its environment;
  - first reads `git config --list --show-origin`. A key that starts a program and comes from the repository or the worktree config (not from the global or system config of the user) stops the job: it ends `failed` with the key in its reason, and changes nothing. The keys are `core.fsmonitor`, `core.sshCommand`, `core.askPass`, `core.editor`, `sequence.editor`, `credential.helper`, `gpg.program`, `diff.external`, and each `filter.<name>.clean`, `filter.<name>.smudge`, `filter.<name>.process`, `merge.<name>.driver` and `diff.<name>.textconv`.
  - For this ticket: `git add -A` and `git status` run a `clean` filter and `core.fsmonitor`. Use the gateway function of ticket 05. A rescue that the rule stops keeps the tree and opens the needs-you item of a failed rescue.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q5, Q6 and Q25 apply here).
- The subject of the job is the worktree id. The input key is the path and its HEAD. A `rescue` job is of class effect (`KINDS` in `fleet_runtime/domain/jobs.rs`).
- The steps, in `adapters/rescue_git.rs`, in the folder of the tree, with a private index file under `<fleet root>/run/` (`rescue-<worktree id>.index`; the fleet root of `docs/tickets/fleetd-p1/MAP.md` Q4):
  1. `GIT_INDEX_FILE=<private> git read-tree HEAD`; `git add -A`; `tree=$(git write-tree)`; `commit=$(git commit-tree "$tree" -p HEAD -m "horch rescue <worktree id> attempt <attempt>")`. `.gitignore` applies: ignored files are not saved (Q6).
  2. The ref: `refs/horch/rescue/<worktree id>/<n>`, `n` = 1 + the highest `n` that exists for that worktree (Q5). Write it create-only: `git update-ref <ref> <commit> ""` (the empty old value), so a ref that exists is never overwritten. If the write fails because the ref exists, take the next `n`.
  3. Read back: `git rev-parse <ref>` equals `commit`.
  4. Verify: with a second new private index, `read-tree HEAD`, `add -A`, `write-tree`, and compare the tree id with the tree of `commit`; `git rev-parse <commit>^` equals the HEAD of the tree; the HEAD did not move between step 1 and step 4. A fact that git cannot give fails the verification.
  5. Write the manifest `<fleet root>/rescue/<worktree id>-<n>.json`: the ref, the old HEAD, the file count, the size, the attempt (design: "fleetd then writes `/fleet/rescue/T-0007-03-1.json`").
  6. Delete the 2 private index files. The tree, its branch, its index and its files are not changed.
- The decisions, in `domain/rescue.rs` (pure, no I/O): the next `n` from the refs that exist; whether the facts of step 4 verify; the needs-you key of a failed rescue (for example `rescue_failed:<worktree id>`).
- The observation, in `application/rescue.rs`, a function that the pass (ticket 09) and a command (tickets 11, 12) call in 1 transaction: a `passed` rescue run whose result names a verified ref -> `set_rescue_ref` (ticket 03 store), then `transition(Entity::Worktree, id, "rescued", ...)` from `orphaned` (or `landing`). A `failed` run -> the row stays, and 1 needs-you item opens (Phase 1 `needs_you`, `crates/horch-core/src/fleetd/ideas/application/needs_you.rs`). An `abandoned` run (its limit or a lost process) -> the row stays, and the rule of `abandoned` runs of Phase 1 applies (`jobs::after_abandoned`, `held`): the next run waits for its backoff, and at the limit the Phase 1 needs-you item opens. An `abandoned` run is never `rescued`.
- Rescue refs are not deleted in Phase 3 (Q6). A stash entry is not saved by a rescue (preservation row W6): ticket 07 refuses to remove a tree whose branch has a stash entry.
- Register the executor in `adapters/executors.rs` (ticket 05 made the file; this ticket replaces the `no_executor` of `rescue`).

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_rescue.rs`, each with its own temporary repository. Each fails before the code exists:
   - `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref`: a tree with 1 commit, 1 modified tracked file and 1 untracked file; after the job, the rescue commit holds the 3, its parent is the HEAD, and the branch, the index (`git diff --cached` is the same) and the files of the tree did not change.
   - `rescue_keeps_the_branch_and_the_index_of_the_tree`.
   - `rr16_second_rescue_of_1_tree_does_not_overwrite_the_first_ref`.
   - `rescue_ref_is_read_back_before_the_tree_is_rescued`.
   - `failed_rescue_verification_keeps_the_tree`: a file changes between step 1 and step 4 (a fault point or a fake gateway); the row stays `orphaned`, no ref is recorded on the row, and 1 needs-you item is open.
   - `rescue_job_at_its_limit_is_not_rescued_and_runs_again_after_its_backoff`, and `rescue_job_abandoned_3_times_opens_1_needs_you_item`.
   - `rescue_writes_its_manifest_with_ref_head_count_size_and_attempt`.
   - `ignored_files_are_not_in_the_rescue_ref`.
   - `rescue_runs_no_hook_or_program_of_the_project`: a repository with a `filter.x.clean` in its config and a `.gitattributes` line `* filter=x`, where the filter writes a marker file. The rescue ends `failed` with the key in its reason, the tree stays, 1 needs-you item is open, and the marker does not exist.
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

The Check passes. A rescue job saves each commit, modified file and untracked file of a tree on a new rescue ref, reads it back and verifies it, and only then is the row `rescued`. A failed, timed-out or lost rescue leaves the tree and its row as they were.
