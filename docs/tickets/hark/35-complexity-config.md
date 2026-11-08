# 35: The complexity sweep uses the nearest analyser config, and hark gets DSP thresholds

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-hardening` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core taskledger::finish::complexity`
**Own:** `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/src/taskledger/finish/complexity/`, `skills/hark/engine/.static-analysisrc.json`

## Goal

`horch ledger finish` judges each changed file against the static-analysis config nearest to that file. `skills/hark/engine/.static-analysisrc.json` holds thresholds suited to numeric DSP code. Every other path keeps the default thresholds.

## Context

- Operator decision (2026-11-08): hark refactors its substantive complexity breaches, and a config under `skills/hark/engine` sets DSP-suited thresholds. The rest of the repo keeps the defaults.
- The complexity step in `crates/horch-core/src/taskledger/finish/complexity.rs` runs the analyser CLI (`static-analysis`, see `analyser()` near line 353) once over the changed files with `--threshold-action none`.
- The CLI finds its config with cosmiconfig, module name `static-analysis`, searching upward from the current directory. Config reference: `/Users/mascott/.claude/plugins/marketplaces/skill-marketplace/plugins/code/skills/complexity-sweep/config/config-reference.md`. It has no per-path overrides.
- Evidence: T-15 (hark timeline) got 498 breaches on 185 of 303 functions. 43 are substantive (branch complexity, or 5 or more metrics). 95 are 1-2 minor breaches, for example scopeDepthRatio on a 3-line function. Full list: `/Users/mascott/.horch/evidence/3ca13978-7b86-4673-99f4-2ce74aecc285/187291f2-8200-442c-9ef3-fbb634c5daca/2-complexity.json`.

## Steps

1. For each changed file, find the nearest `.static-analysisrc.json` (or another cosmiconfig name for `static-analysis`) from the file's folder up to the repo root. Group files by that config. Run the analyser once per group with `--config <path>`. Files with no config run as today.
2. Add tests: a file under a folder with a config is judged by it; a file outside keeps the defaults; 2 groups in 1 run both report.
3. Read the evidence file. Write `skills/hark/engine/.static-analysisrc.json`. Keep the default thresholds for control flow (cyclomatic, essential complexity, cognitive). Relax only the metrics that dense numeric code breaches without a real problem (for example halstead volume, difficulty and effort, scope-depth ratio on short functions, peak live variables), and only as far as the evidence needs. Write the reason for each relaxed metric in a `"$comment"` field or in your DONE report.
4. Re-run the analyser with the new config on the T-15 files. Report how many functions still breach. They must be about the 43 substantive ones.
5. Run the check. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Do not relax cyclomatic or essential complexity.
- Do not change the default thresholds for any path outside `skills/hark/engine`.

## Done when

- The check passes. The DONE report gives the relaxed metrics with reasons and the breach count of step 4.
