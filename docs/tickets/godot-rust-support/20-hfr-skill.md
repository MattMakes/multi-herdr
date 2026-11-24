# 20: godot-hfr-hunt: SKILL.md, harness recorder and detection pass

<!-- horch-ticket: 1 -->
**Persona:** primary `godot-performance-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_19_hfr_hunt_skill_md grs_21 && cargo test -p horch-core --test skills_catalog && python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/copied.json skills/godot-hfr-hunt/`
**Own:** `skills/godot-hfr-hunt/SKILL.md`, `skills/godot-hfr-hunt/references/measure-harness.md`, `skills/godot-hfr-hunt/references/detection.md`, `skills/copied.json`, `skills/README.md`, `crates/horch-core/tests/skills_catalog.rs`, `crates/horch-core/tests/godot_rust_support/hfr_skill.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, skill layout, detection pass and Open Question 6. GRS-19 is shared with ticket 21, which writes the other 4 reference files.).
Waits for: 01 (Scaffold the godot_rust_support test binary); 08 (godot-check.sh fails on script errors that exit 0). Wave 5.

## Goal

A performance engineer gets a 5-step workflow to find what keeps a Godot game from 200 to 300 fps.

## Requirements

- **GRS-19**: `skills/godot-hfr-hunt/` is an own-text bundled skill. `SKILL.md` (<= 12 KB) gives the 5-step workflow in order: uncap the frame rate, run `scripts/hfr_bench.py`, read p50/p95/p99/worst/1% low against the 5.00, 4.17 and 3.33 ms budgets, classify the hot spot as script CPU, render thread, GPU, physics or spikes by its signature, and apply the matching technique. `references/` holds 1 file per angle (`script-cpu.md`, `render-gpu.md`, `physics-servers-threads.md`, `measure-harness.md`, `native-memory-hitches.md`) taken from the 5 HFR reports, and each technique keeps its What, Why/thread, Measure, Version, Evidence label and Trap. Tests: grs_19_hfr_hunt_skill_md_orders_the_five_workflow_steps, grs_19_hfr_hunt_references_have_one_file_per_angle_with_measure_evidence_and_trap.
- **GRS-21**: `skills/godot-hfr-hunt/references/detection.md` groups the "Detection" sections of the 5 HFR reports by the 5 hot-spot classes and keeps every `rg` command, profiler signature and monitor threshold, including the frame-cap search, the 5%-of-budget triage threshold (0.20 ms at 240 fps, 0.17 ms at 300 fps), the 4.17 ms and 3.33 ms frame triggers, gameplay-time `Draw`/`Surface` pipeline compilations, `PHYSICS_*_COLLISION_PAIRS` growth at a flat entity count, and GC events that line up with slow frames. Tests: grs_21_hfr_detection_reference_keeps_greps_signatures_and_thresholds.

## Steps

1. Write `grs_19_hfr_hunt_skill_md_orders_the_five_workflow_steps` and `grs_21_hfr_detection_reference_keeps_greps_signatures_and_thresholds` first.
2. Write `SKILL.md` (at or below 12 KB) with the 5 steps in the spec order. Keep source links. Do not copy the reports' Corrections sections.
3. Write `references/measure-harness.md` from report 4, with each technique's What, Why/thread, Measure, Version, Evidence and Trap, the escalation ladder, and the GDScript recorder as a ```` ```gdscript ```` block (Open Question 6). The recorder follows the spec: user arguments, cap asserts, `Time.get_ticks_usec()` deltas, summed viewport render times, `null` for unavailable timers, atomic write, exit codes.
4. Write `references/detection.md` from the 5 Detection sections, grouped by the 5 classes. Keep every `rg` command, signature and threshold the spec names.
5. Register `godot-hfr-hunt` in `skills/copied.json` (`"copied_files": []`, `"verbatim": false`), `REPO_ORIGINAL` and `skills/README.md`.
6. Check the `gdscript_blocks_check.py` arguments with `--help` before you run the Check.
7. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
8. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The recorder block passes `gdscript_blocks_check.py --strict-own`.
