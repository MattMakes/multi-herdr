# 17: Only worktree custody runs git worktree add or remove: the arch_scan rule, and the profiles deny git worktree

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib harness && cargo test -p horch-core --test golden_prompts && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/tests/arch_scan.rs`, `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/tests/golden_prompts.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md`, FR-11, FM-26, "Worktrees: leased, reconciled, never lost" ("Only fleetd creates or removes worktrees. ... Their profiles deny `git worktree` through each CLI's permission rules, and a new `arch_scan` rule keeps worktree calls out of every horch code path except fleetd.").

## Goal

A fitness test fails when production code outside `crates/horch-core/src/fleetd/worktree_custody/adapters/` runs `git worktree add`, `remove` or `prune`. The worker profiles of Claude Code and Codex deny `git worktree` through the permission rules of each CLI. A worker that makes a tree anyway is found by the reconciler (ticket 09) as an unowned tree.

## Requirements

- **FR-11**: `arch_scan` finds no worktree calls outside fleetd.
- **FM-26**: profiles deny `git worktree`; `arch_scan` keeps it out of horch.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q15 and Q16 apply here).
- After tickets 08, 12, 13 and 15, the production calls of `git worktree add` and `remove` are in `fleetd/worktree_custody/adapters/` only, with 1 exception (Q15): `crates/horch-core/src/vcs/landing.rs`, the claims of `horch worktree new` and `land`, which belong to no ledger run. Find each call with `rg '"worktree",' crates --glob '*.rs'` and check the list. If a production call outside these 2 places is left, send a QUESTION with its path; do not move it in this ticket.
- The rule, in `crates/horch-core/tests/arch_scan.rs`: a new test `fitness_only_worktree_custody_runs_git_worktree_add_or_remove`. It scans the production code (skip tests and comments, as `forbidden_lines` and `code_lines` do) of each crate for a string literal `"worktree"` followed by `"add"`, `"remove"` or `"prune"` in the same call. Allowed: files under `fleetd/worktree_custody/adapters/`; the list `WORKTREE_CALL_EXEMPT` with 1 entry, `("horch-core/vcs/landing.rs", "<the reason of Q15>")`. `git worktree list` is a read and is not in the rule (Q16). Add a matcher test with its own cases, as `status_write_matcher_finds_a_direct_update` does. `docs/architecture.md` gets the row of the new rule in its table of fitness rules.
- The profiles (Q16):
  - Claude Code: `crates/horch-core/src/harness/claude.rs` writes `permissions.deny` entries of a worker pane (the `PERMS` entries; see `deny_writes` and `add_entries`). Add `Bash(git worktree add:*)`, `Bash(git worktree remove:*)` and `Bash(git worktree prune:*)` for a worker. The orchestrator pane does not need them; follow how the file tells a worker from the orchestrator.
  - Codex: `crates/horch-core/src/harness/codex.rs` renders `prefix_rule` blocks (`decision` `forbidden` for a worker). Add `["git", "worktree", "add"]`, `["git", "worktree", "remove"]` and `["git", "worktree", "prune"]` as `forbidden` for a worker. On Windows, codex shares 1 rules file and gets no forbidden rule (`the_shared_rules_file_gets_no_forbidden_rule`); keep that. There the reconciler alone catches a tree that a worker makes. Name it in the commit message.
  - Other CLIs: no change. The reconciler catches their trees.
  - `crates/horch-core/tests/golden_prompts.rs` pins the rules; update the pins of the new rules only.

## Steps

1. Write the tests first. Each fails before the code exists:
   - `fitness_only_worktree_custody_runs_git_worktree_add_or_remove` and its matcher test.
   - In `claude.rs`: `worker_settings_deny_git_worktree_add_remove_and_prune`.
   - In `codex.rs`: `worker_rules_forbid_git_worktree_add_remove_and_prune` (with the decision of the rule test helper of that file, `["allow", "prompt", "forbidden"]`).
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

The Check passes. A `git worktree add` or `remove` in production code outside worktree custody fails `arch_scan`, and the worker settings of Claude Code and Codex deny `git worktree add`, `remove` and `prune`.
