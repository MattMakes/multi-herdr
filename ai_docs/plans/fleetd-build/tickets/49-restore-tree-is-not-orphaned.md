# The tree of horch worktree restore is created, not orphaned, when the command goes through a request

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --test p3_triage && cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-e2e/tests/p3_triage.rs`

## Goal

After `horch worktree restore` through the test fleetd, the row of the new operator tree is in
state `created` (then `active` as the design says), and the reconciler never moves it to
`orphaned`. The test `restore_makes_its_tree_through_a_worktree_add_job` of
`crates/horch-e2e/tests/p3_triage.rs` is green with its assert unchanged.

## Context

- Found by T-880 (opus-hardening-154). After the routing of the worktree commands (T-474) and
  the move of the test to the test fleetd, the new row of the operator tree is `orphaned`, not
  `created`. The test states a requirement: the reconciler does not orphan an operator tree.
- Read the done summaries of T-474, T-880 and T-300 first (`horch ledger show T-<n>`). T-300
  found a related fault: the reconciler adopted a tree from a stale `worktree_list` result.
- The design is `docs/specs/fleetd/design.md`, the worktree state table (about line 1030).

## Steps

1. Run the test and find why the row becomes `orphaned`: the order of the request handler, the
   `worktree_add` job and the reconcile scan. Write 1 unit test in the custody context that
   shows the cause. Check: red first.
2. Fix the cause in the custody code. Do not make the reconciler skip a real orphan. Check: the
   unit test and the e2e test are green.
3. Check the same order fault for a tree that fleetd makes for a task attempt. Add 1 test if no
   test proves it.

## Constraints

- Simplified Technical English. No em dash.
- 1 door: each state change goes through `transition()`. No schema step.
- Do not change an assert of the e2e test.
- Use a timeout on each test command. Each test uses temporary folders. Never use the live ledger.
- NEVER use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes, and the done summary names the cause and each changed file.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
