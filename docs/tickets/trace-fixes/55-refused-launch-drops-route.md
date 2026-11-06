# 55: A refused fleet launch leaves no orchestrator route behind

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch -- trf_38 trf_39 recipes`
**Own:** `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/src/taskledger/orchestrator.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 54.
Prior work: ticket 54, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/54.md`, section "Follow-ups outside scope" (F1, F2).
Waits for: 54.

## Goal

Ticket 54 makes a fleet launch refuse when its run claim is `Skipped`. But `pane_launch` in `crates/horch/src/cmd/recipes.rs` runs `register_pane` (which writes the `orchestrator` route) before the claim, so a refused launch leaves its workspace's `orchestrator` route pointing at a pane with no agent.

## Requirements

- **TRF-38** (continued): a refused fleet launch leaves the mailbox routes of its workspace as they were before the launch. Test names start with `trf_38_`.

## Steps

1. Claim the run before `register_role`, under the same workspace orchestrator lock, so a refusal writes no route. If the order cannot change (say why in the report), remove the route on refusal instead, only when it names this launch's pane.
2. F2: let `Claimed::Skipped` carry the holder's workspace and pid as fields, and use them for the refusal line instead of a 2nd holder read.
3. Extend `trf_38_a_skipped_claim_starts_no_agent`, or add `trf_38_a_refused_launch_writes_no_route`: no `orchestrator.id` (or the previous one, unchanged) in workspace B after the refusal.
4. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/55.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
