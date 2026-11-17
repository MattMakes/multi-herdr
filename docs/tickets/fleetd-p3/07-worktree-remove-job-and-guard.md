# 07: The worktree_remove job and its guard: only a landed tree or a tree on a verified rescue ref is removed

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test worktree_remove --test worktree_rescue && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/domain/removal.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/remove.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/remove_git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`, `crates/horch-core/tests/worktree_remove.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" (the rows `landed`, `rescued`, `removed`; "Rescue before any cleanup": "Only after that may a tree be removed"), "The kinds of job" (row `worktree_remove`), RR-9, RR-13, FM-27, I6; `docs/specs/fleetd/preservation-04-behavior.md` rows W6 and W8.

## Goal

A `worktree_remove` job is the only code that removes a worktree folder. Just before it removes, it checks again that nothing in the tree is unique: the tree is `landed` and clean at its landed commit, or its rescue ref was read back and holds exactly the files on disk. It also keeps today's refusals of a drop: a dirty tree, a branch that a stash entry names, unique commits, and a HEAD that moved during the checks. If 1 fact is unknown, it removes nothing.

## Requirements

- **RR-9** (the guard): no worktree is removed until its work is on a verified ref.
- **RR-13** (the refusals at the removal): a dirty tree, a branch with a `git stash` entry, unpublished unique commits and a HEAD that moved during the checks are refused, and the tree stays.
- **FM-27** (prevented inside the fleet): fleetd removes only `landed` trees or trees with a verified ref.
- The fleetd git rule (review F4 of T-607). This job runs git with the rights of fleetd and with no sandbox. So git must start no hook and no program of the project. Each git command of the job:
  - runs with `-c core.hooksPath=/dev/null`, `-c core.fsmonitor=false`, `-c commit.gpgSign=false` and `-c tag.gpgSign=false`, and with `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` in its environment;
  - first reads `git config --list --show-origin`. A key that starts a program and comes from the repository or the worktree config (not from the global or system config of the user) stops the job: it ends `failed` with the key in its reason, and changes nothing. The keys are `core.fsmonitor`, `core.sshCommand`, `core.askPass`, `core.editor`, `sequence.editor`, `credential.helper`, `gpg.program`, `diff.external`, and each `filter.<name>.clean`, `filter.<name>.smudge`, `filter.<name>.process`, `merge.<name>.driver` and `diff.<name>.textconv`.
  - For this ticket: the facts of the guard run `git status` (`core.fsmonitor`), and `git worktree remove` can run a hook. Use the gateway function of ticket 05.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q7 and Q8 apply here).
- Today's refusals: `drop_worktree` and `verified_head` in `crates/horch-core/src/taskledger/worktree_ops.rs` (row W6: dirty tree, `git.stashes_on`, unique commits without `--keep-branch`, a HEAD that differs from the recorded branch); `delete_worktree` and `cleanup` in `crates/horch-core/src/taskledger/finish/land.rs`; `remove_folder` and `check_identity` in `crates/horch-core/src/taskledger/triage/apply.rs`. Read them. Do not change them here: tickets 12 and 13 make them queue this job.
- The guard, in `domain/removal.rs` (pure, from facts as values): the tree may be removed only when each of these holds:
  1. Saved: (a) the row is `landed`, its HEAD equals `landed_commit`, and the tree has no modified and no untracked file; or (b) the row is `rescued`, its `rescue_ref` reads back to its commit, a tree written from the folder now (private index, as ticket 06) has the same tree id as that commit, and the HEAD now is the parent of that commit (Q7). Triage, drop and cleanup reach (b) through a rescue job first (tickets 12 and 13).
  2. No `git stash` entry names the branch of the tree.
  3. The HEAD read at the start of the checks equals the HEAD read at their end.
  4. Each fact was read. A fact that git cannot give makes the guard refuse, with its reason.
- The executor, in `adapters/remove_git.rs`: read the facts, ask the guard, then `git worktree remove <path>` with no `--force`. A refusal ends the job `failed` with the reason of the guard and changes nothing. Delete the branch only when it is merged (design row `landed`: "deletes the branch only if it is merged"), with `git branch -d` (no `-D`). Register it in `adapters/executors.rs` (tickets 05 and 06 made the file).
- The observation, in `application/remove.rs`, a function that the pass (ticket 09) and a command (ticket 12) call in 1 transaction: a `passed` run -> `transition(Entity::Worktree, id, "removed", ...)` from `landed` or `rescued`, and today's `cleaned_at`. A `failed` run -> the row stays and 1 needs-you item opens with the reason. An `abandoned` run -> the rule of `abandoned` runs of Phase 1.
- The queue, in `application/remove.rs`: `queue_remove(tx, row, cause)` queues 1 `worktree_remove` run (subject the worktree id, input key the path) only for a row in `landed` or `rescued`. Each other state is refused with its reason.

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_remove.rs`, each with its own temporary repository. Each fails before the code exists:
   - `rr9_rescue_ref_is_read_back_before_removal`: a rescued tree is removed, and the rescue ref holds each file.
   - `pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head`: 4 cases. After each refusal the folder, each modified and untracked file, each commit and the stash entry are still there.
   - `remove_refuses_a_rescued_tree_that_changed_after_its_rescue`: a file is written after the rescue; the tree stays.
   - `remove_refuses_a_landed_tree_with_an_untracked_file`.
   - `remove_refuses_when_a_fact_is_unknown` (a fake gateway gives an error for 1 fact).
   - `remove_keeps_a_branch_that_is_not_merged`.
   - `queue_remove_refuses_an_active_orphaned_or_reserved_row`.
   - `failed_remove_keeps_the_row_and_opens_1_needs_you_item`.
   - `remove_runs_no_hook_or_program_of_the_project`: a repository with `core.fsmonitor` set in its config to a script that writes a marker file. The remove ends `failed` with the key in its reason, the tree stays, and the marker does not exist.
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

The Check passes. A `worktree_remove` job removes a tree only when its work is landed or on a read-back rescue ref that matches the folder now, and each refusal leaves every file, commit and stash entry in place.
