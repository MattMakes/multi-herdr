# 21: godot-hfr-hunt: the 4 technique reference files

<!-- horch-ticket: 1 -->
**Persona:** primary `godot-performance-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_19 grs_21 && python3 scripts/godot/xref_check.py && python3 scripts/godot/api_check.py --strict-own skills/copied.json skills/godot-hfr-hunt/`
**Own:** `skills/godot-hfr-hunt/references/script-cpu.md`, `skills/godot-hfr-hunt/references/render-gpu.md`, `skills/godot-hfr-hunt/references/physics-servers-threads.md`, `skills/godot-hfr-hunt/references/native-memory-hitches.md`, `crates/horch-core/tests/godot_rust_support/hfr_references.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, skill layout).
Waits for: 01 (Scaffold the godot_rust_support test binary); 20 (godot-hfr-hunt: SKILL.md, harness recorder and detection pass). Wave 6.

## Goal

Each hot-spot class in the workflow points to a reference file with measured techniques.

## Requirements

- **GRS-19**: `skills/godot-hfr-hunt/` is an own-text bundled skill. `SKILL.md` (<= 12 KB) gives the 5-step workflow in order: uncap the frame rate, run `scripts/hfr_bench.py`, read p50/p95/p99/worst/1% low against the 5.00, 4.17 and 3.33 ms budgets, classify the hot spot as script CPU, render thread, GPU, physics or spikes by its signature, and apply the matching technique. `references/` holds 1 file per angle (`script-cpu.md`, `render-gpu.md`, `physics-servers-threads.md`, `measure-harness.md`, `native-memory-hitches.md`) taken from the 5 HFR reports, and each technique keeps its What, Why/thread, Measure, Version, Evidence label and Trap. Tests: grs_19_hfr_hunt_skill_md_orders_the_five_workflow_steps, grs_19_hfr_hunt_references_have_one_file_per_angle_with_measure_evidence_and_trap.

## Steps

1. Write `grs_19_hfr_hunt_references_have_one_file_per_angle_with_measure_evidence_and_trap` first. It checks all 5 angle files, with `measure-harness.md` from ticket 20.
2. Write `script-cpu.md` from report 1, `render-gpu.md` from report 2, `physics-servers-threads.md` from report 3 and `native-memory-hitches.md` from report 5.
3. Give every technique its What, Why/thread, Measure, Version, Evidence label and Trap. Keep source links.
4. Keep the skill directory at or below 160 KB.
5. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
6. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. Every link from `SKILL.md` resolves.
