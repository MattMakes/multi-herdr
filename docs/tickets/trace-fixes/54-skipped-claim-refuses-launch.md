# 54: A skipped run claim refuses the orchestrator launch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- trf_38 trf_39 trf_45 import lifecycle && cargo test -p horch -- trf_38 trf_39 recipes`
**Own:** `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/src/taskledger/import.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after review 32.
Prior work: tickets 42, 44, 50; review `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/32.md` (finding I-01 and section "Preserved history and output coverage"); report 50, section "Follow-ups outside scope".
Waits for: 50.

## Goal

Review 32 I-01: when a fleet launch finds the active run in another workspace with a live lock holder, `orchestrator::claim` returns `Claimed::Skipped` (`crates/horch-core/src/taskledger/orchestrator.rs`). `claim_launch_run` in `crates/horch/src/cmd/recipes.rs` handles `Skipped` like `Changed`: it prints the text and returns success, and `pane_launch` goes on to start a second orchestrator. Two live orchestrators can then act on 1 project ledger.

## Requirements

- **TRF-38** (continued): a fleet launch whose run claim is `Skipped` starts no agent. It prints the claim text and 1 line naming the workspace and pid that hold the run, and exits non-zero. Test names start with `trf_38_`.
- **TRF-39** (continued): a worker never registers the role `orchestrator`. Test names start with `trf_39_`.
- **TRF-45** (continued): import keeps a non-empty history. Test names start with `trf_45_`.

## Steps

1. In `claim_launch_run`, make `Claimed::Skipped` a launch refusal before `launch::run_flow` runs. `Changed` keeps its current behaviour.
2. Test `trf_38_a_skipped_claim_starts_no_agent`: an active run in workspace A with a live holder; a launch in workspace B; the launch callback never runs, the exit is non-zero, and A's route and run row do not change.
3. Report 50: worker startup (`register` in `crates/horch-core/src/execution/lifecycle.rs`, `Mailbox::register_in`) takes no orchestrator lock. Make it refuse the role `orchestrator` with 1 line. Test `trf_39_a_worker_cannot_register_the_orchestrator_role`.
4. Review 32: the import test uses an empty history. Add `trf_45_import_keeps_a_non_empty_history` in `crates/horch-core/src/taskledger/import.rs`: a legacy record with 3 history entries imports, then 1 more `update` runs, and all 4 entries remain.
5. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes, and report 54 shows the refusal lines.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/54.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
