# 02: complexity-sweep: nested Cargo crates and 3 clippy lints

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib -- finish::complexity && cargo test -p horch-core --test godot_rust_support -- grs_01 grs_02 && cargo test -p horch-e2e --test taskledger -- grs_01`
**Own:** `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/src/taskledger/finish/complexity/`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-core/tests/godot_rust_support/complexity_rust.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 1, first 2 items).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

complexity-sweep finds every changed Rust file in a nested crate such as `rust/Cargo.toml`, and it reports cognitive complexity, function length and parameter count with clean function names.

## Requirements

- **GRS-01**: `complexity-sweep` discovers changed `.rs` files in both top-level and nested Cargo crates by finding the nearest ancestor `Cargo.toml` within the work directory, runs `cargo clippy` once per crate directory, and prefixes reported `file_path` values back to work-dir-relative paths before filtering by changed hunks. Tests: grs_01_nested_cargo_crates_are_grouped_and_paths_prefixed, grs_01_finish_analyses_nested_gdext_rust_crate.
- **GRS-02**: `complexity-sweep` runs `cargo clippy` with `-W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`, maps them to `cognitiveComplexity`, `functionLength`, and `parameterCount`, extracts the function identifier after `fn` from full-signature primary spans, and deduplicates findings by `(file_path, line, metric)`. Tests: grs_02_clippy_parses_three_lints_and_extracts_fn_identifier, grs_02_clippy_dedupes_by_file_line_and_metric.

## Steps

1. Write the failing tests first: `grs_01_nested_cargo_crates_are_grouped_and_paths_prefixed` and `grs_02_*` in the submodule or in the `mod tests` of `complexity.rs`, and `grs_01_finish_analyses_nested_gdext_rust_crate` in `crates/horch-e2e/tests/taskledger.rs`.
2. Replace the top-level `Cargo.toml` gate in `complexity::rust_sources` with nearest-ancestor discovery, modelled on `complexity::config::nearest`. Group changed `.rs` files by crate directory in a `BTreeMap`.
3. Run `cargo clippy` once per crate directory. Write `<seq>-clippy.json` for group 0 and `<seq>-clippy-<n+1>.json` for later groups.
4. Normalize each `span.file_name` to a work-dir-relative path before `keep_changed()` filters by hunk.
5. Pass `-A clippy::all -W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`. Map the lints to `cognitiveComplexity`, `functionLength` and `parameterCount`.
6. Make `name(span)` take the identifier after `fn` up to `<` or `(` for signature spans. Keep the raw slice for 1-identifier spans.
7. Deduplicate by `(file_path, line, metric)`.
8. Warm every new fake executable once with a `warm` argument before a test runs it (ADR 0111, `crates/horch-core/tests/common/warm.rs`). A submodule that needs it declares `#[path = "../common/warm.rs"] mod warm;` itself.
9. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
10. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
11. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. A fixture with `rust/Cargo.toml` and a changed over-complex function in `rust/src/lib.rs` fails the step and names `rust/src/lib.rs` and the bare function name. A function that breaks 2 lints reports 2 violations.
