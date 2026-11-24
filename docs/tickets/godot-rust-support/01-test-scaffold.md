# 01: Scaffold the godot_rust_support test binary

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `cargo test -p horch-core --test godot_rust_support`
**Own:** `crates/horch-core/tests/godot_rust_support.rs`, `crates/horch-core/tests/godot_rust_support/complexity_rust.rs`, `crates/horch-core/tests/godot_rust_support/complexity_gdscript.rs`, `crates/horch-core/tests/godot_rust_support/rust_team.rs`, `crates/horch-core/tests/godot_rust_support/godot_check.rs`, `crates/horch-core/tests/godot_rust_support/native_engineer.rs`, `crates/horch-core/tests/godot_rust_support/csharp_seats.rs`, `crates/horch-core/tests/godot_rust_support/sweeps.rs`, `crates/horch-core/tests/godot_rust_support/stack_check.rs`, `crates/horch-core/tests/godot_rust_support/worktree_cache.rs`, `crates/horch-core/tests/godot_rust_support/codex_cargo.rs`, `crates/horch-core/tests/godot_rust_support/live_checks.rs`, `crates/horch-core/tests/godot_rust_support/perf_measure.rs`, `crates/horch-core/tests/godot_rust_support/optimization_cpu_draw.rs`, `crates/horch-core/tests/godot_rust_support/optimization_physics.rs`, `crates/horch-core/tests/godot_rust_support/multithreading.rs`, `crates/horch-core/tests/godot_rust_support/hfr_skill.rs`, `crates/horch-core/tests/godot_rust_support/hfr_references.rs`, `crates/horch-core/tests/godot_rust_support/hfr_bench.rs`

Spec: `docs/specs/godot-rust-support.md` (Test Seams, item 1, and Open Question 1).
Waits for: nothing. Wave 1.

## Goal

Every later ticket adds its `grs_<nn>_` tests to 1 shared test binary without editing the same file as another ticket.

## Steps

1. Create `crates/horch-core/tests/godot_rust_support.rs`. It holds only 1 `mod <name>;` line for each submodule below, in this order: `complexity_rust`, `complexity_gdscript`, `rust_team`, `godot_check`, `native_engineer`, `csharp_seats`, `sweeps`, `stack_check`, `worktree_cache`, `codex_cargo`, `live_checks`, `perf_measure`, `optimization_cpu_draw`, `optimization_physics`, `multithreading`, `hfr_skill`, `hfr_references`, `hfr_bench`.
2. Create each submodule file `crates/horch-core/tests/godot_rust_support/<name>.rs` empty. Rust resolves `mod <name>;` in `tests/godot_rust_support.rs` to `tests/godot_rust_support/<name>.rs`.
3. Add no test and no `use` line. Later tickets own 1 submodule each and fill it.
4. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
5. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check compiles the binary and reports 0 tests. `cargo clippy -p horch-core --all-targets -- -D warnings` is clean.
