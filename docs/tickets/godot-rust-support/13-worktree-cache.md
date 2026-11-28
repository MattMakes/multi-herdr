# 13: Measure worktree build and import cost, then seed the Godot cache

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_15 && cargo test -p horch-core --test vcs`
**Own:** `ai_docs/reports/godot-rust-worktree-timing.md`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/vcs/landing.rs`, `crates/horch-core/tests/godot_rust_support/worktree_cache.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 6, first item (G9)).
Waits for: 01 (Scaffold the godot_rust_support test binary); 03 (complexity-sweep: GDScript route through gdcomplexity.py). Wave 4.

## Goal

Godot workers stop re-importing unchanged assets in a new worktree, when measurement shows that the gain is worth it.

## Requirements

- **GRS-15**: Worktree cold vs warm build/import times are measured in `ai_docs/reports/godot-rust-worktree-timing.md`, and worktree creation seeds `.godot/imported/` (and class/UID caches) from the base checkout when present so Godot workers avoid cold re-imports without sharing `CARGO_TARGET_DIR`. Tests: grs_15_worktree_new_seeds_godot_imported_cache_when_present.

## Steps

1. Measure first. Build a sample Godot + Rust project. Time a cold and a warm `cargo check`, `cargo test` and `godot --headless --import` in a new worktree, 3 runs each. Write the numbers and the commands to `ai_docs/reports/godot-rust-worktree-timing.md`.
2. Change code only when seeding `.godot/imported/` saves more than 30 percent of the cold import wall time. Then copy `.godot/imported/` and `uid_cache.bin` or `global_script_class_cache.cfg` from the base checkout in `create_worktree` and `add_worktree`. Never share `CARGO_TARGET_DIR`.
3. With code: write `grs_15_worktree_new_seeds_godot_imported_cache_when_present`.
4. Without code: write a `grs_15_` test that checks the report holds the cold and warm times and the no-seeding decision. Send `horch tell orchestrator` a NOTE that the GRS-15 row needs an amendment.
5. If no Godot binary is on the host, send BLOCKED with the command you need.
6. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
7. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
8. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The report gives the numbers and the decision against the 30 percent rule.
