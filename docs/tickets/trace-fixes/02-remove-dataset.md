# 02: Remove competitive dataset rounds

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo test -p horch-core --lib && cargo test -p horch --lib -- trf_47`
**Own:** `crates/horch-core/src/competition/`, `crates/horch-core/src/evaluation/`, `crates/horch-core/src/dataset/`, `crates/horch-core/src/fleet_runs/`, `crates/horch-core/src/measure/`, `crates/horch-core/src/teacher/`, `crates/horch-core/src/lib.rs`, `crates/horch/src/dataset/`, `crates/horch/src/bin/multi-herdr-dataset.rs`, `crates/horch/src/lib.rs`, `crates/horch/src/main.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch/src/cmd/selfupdate.rs`, `crates/horch/Cargo.toml`, `crates/herdr-install/src/manifest.rs`, `crates/horch-e2e/src/bin/fake-herdr.rs`, `crates/horch-e2e/tests/judge.rs`, `crates/horch-e2e/tests/usage_dataset.rs`, `crates/horch-e2e/tests/promotion.rs`, `crates/horch-e2e/tests/fleet_sync.rs`, `crates/horch-e2e/tests/fleet_compete.rs`, `crates/horch-e2e/tests/dataset.rs`, `crates/horch/tests/dataset_cli.rs`, `crates/horch/tests/r_cli_cmds_selfupdate.rs`, `crates/horch/tests/self_update_cli.rs`, `crates/horch-core/tests/judging.rs`, `crates/horch-core/tests/dataset_export.rs`, `crates/horch-core/tests/fleet_compete.rs`, `crates/horch-core/tests/evaluation.rs`, `crates/horch-core/tests/r_usage_measure_projection.rs`, `crates/horch-core/tests/judge_input.rs`, `crates/horch-core/tests/promotion.rs`, `crates/horch-core/tests/coordinator.rs`, `crates/horch-core/tests/measure.rs`, `crates/horch-core/tests/competition_planner.rs`, `crates/horch-core/tests/fleet_runs.rs`, `crates/horch-core/tests/r_fleet_runs_pins.rs`, `crates/horch-core/tests/fleet_export.rs`, `crates/horch-core/tests/golden/export-1.0.0.jsonl`, `teammates/_base/fleet-compete.md`, `teammates/_base/competition-candidate.md`, `teammates/judge.md`, `docs/specs/dataset-competition.md`, `docs/specs/fleet-dataset.md`, `docs/dataset-config.md`, `.github/workflows/release.yml`, `justfile`

Spec: `docs/specs/trace-fixes.md` (contracts in section 3, schema in 4, timings in 5, conventions in 8).
Waits for: nothing; wave 1. Wave 1.

## Goal

Delete the unused competitive dataset feature and every wire that ships or installs it (D17).

## Requirements

- **TRF-47**: `multi-herdr-dataset` and the competition, evaluation, dataset, fleet_runs, measure and teacher modules are gone; release archives hold `horch` and `herdr-install`. Test name starts with `trf_47_`.

## Steps

1. Map every reference first: `git grep -n -e multi-herdr-dataset -e competition:: -e evaluation:: -e fleet_runs:: -e measure:: -e teacher:: -e dataset::`. A reference in a file you do not own is a QUESTION.
2. Remove the 6 core modules, `crates/horch/src/dataset/`, the binary, the `--compete-*` fleet flags in main.rs, and the fleet-run fact writes in spawn.rs and messaging.rs.
3. Remove their tests, the export golden, the 3 teammate files (confirm `judge.md` has no other user), the 2 specs and `docs/dataset-config.md`. Their requirement IDs leave with them.
4. Remove the binary from the release archive, `horch install`, self-update and the herdr-install manifest. Keep README and CHANGELOG for ticket 30; list what they must say in your report.
5. Write `trf_47_*`: the release asset list and `horch install` name only horch and herdr-install.

## Done when

No file references the removed modules or binary; the check passes; the report lists the README and CHANGELOG lines for ticket 30.

## Report

Write `ai_docs/reports/trace-fixes/02.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
