# Each text says the truth about a cancelled task: it is terminal and is not requeued

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --test w3_taskledger_core_tasks && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/wakes.rs`, `crates/horch/src/cmd/taskledger.rs`, `docs/specs/task-ledger.md`, and each test or frozen file that pins one of these texts

## Goal

No text of the product says that a cancelled task can be requeued.

## Context

Task T-260 (ticket 04 of Phase 2) made `cancelled` a terminal state: a requeue of a cancelled task
is refused. Its review finding F1 found texts that still offer the requeue. The author corrected 3
of them in commit `797be833`, but that commit came after the land. T-260 landed it after a new review round.

## Steps

1. Commit `797be833` (3 texts) is on the run branch through T-260. Check it with
   `git log --oneline -5 -- crates/horch-core/src/taskledger/wakes.rs`. If it is not there, send BLOCKED.
2. Correct the 2 other texts: `crates/horch-core/src/taskledger/tasks.rs` near line 768
   ("requeue it, or drop the dependency") and the help of the requeue command in
   `crates/horch/src/cmd/taskledger.rs` near line 117. Each text names what the operator can do
   for a cancelled task: add the task again, or drop the dependency.
3. Search the workspace for each other text that offers a requeue of a cancelled task
   (`grep -rn -i "cancelled" --include=*.rs --include=*.md crates docs/specs/task-ledger.md
   teammates skills`). Correct each one in your Own. Do not change `docs/specs/fleetd/`: if a file
   there says it, name the file and the line in your done summary.
4. Correct each test and frozen file that pins a text. Check each changed line by hand.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Each test uses
  temporary folders, never the live ledger.
- Change texts only. Change no behavior.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, and the done summary lists each changed text as a user-visible change.
