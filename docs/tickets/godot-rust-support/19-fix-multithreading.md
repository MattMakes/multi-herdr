# 19: Correct godot-multithreading thread and loading claims

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_40 grs_41 grs_42 && python3 scripts/godot/xref_check.py && python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/copied.json skills/godot-multithreading/`
**Own:** `skills/godot-multithreading/SKILL.md`, `crates/horch-core/tests/godot_rust_support/multithreading.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, corrections).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

godot-multithreading says what the HFR reports verified about server threads, GPU sync and threaded loading.

## Requirements

- **GRS-40**: `skills/godot-multithreading/SKILL.md` (line 18) says separate physics and rendering threads are not automatically faster: physics API access is restricted to physics processing, Jolt separate-thread support is experimental in 4.7, and the rendering `Separate` thread model has known bugs. Tests: grs_40_multithreading_says_separate_server_threads_are_not_automatically_faster.
- **GRS-41**: `skills/godot-multithreading/SKILL.md` (line 22) separates CPU preparation from GPU synchronization: workers can fill plain `Image` and packed-array data, while texture creation, GPU-backed image reads and uploads from workers can synchronize with `RenderingServer` and stall. Tests: grs_41_multithreading_separates_cpu_preparation_from_gpu_sync.
- **GRS-42**: `skills/godot-multithreading/SKILL.md` (lines 192-210) says `instantiate()`, `_enter_tree`, `_ready`, server registration and pipeline surface preparation still run on the main thread after `load_threaded_get()`, and the example stages or time-slices scene integration across frames. Tests: grs_42_multithreading_stages_scene_integration_after_threaded_load.

## Steps

1. Read the matching sections of the 5 HFR reports for each row.
2. Write 1 `grs_<nn>_` test per row first.
3. Change only the named claim. Do not restructure the file. The test checks the old text is gone and the new claim is present, not line numbers.
4. Rewrite the example at lines 192-210 so it stages or time-slices scene integration. Keep it valid for `gdscript_blocks_check.py`.
5. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
6. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes for all 3 rows.
