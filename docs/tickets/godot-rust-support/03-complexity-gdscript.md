# 03: complexity-sweep: GDScript route through gdcomplexity.py

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `python3 -m unittest discover -s scripts/godot/tests -p 'test_gdcomplexity.py' && cargo test -p horch-core --lib -- finish::complexity && cargo test -p horch-core --test godot_rust_support -- grs_03 grs_04 && cargo test -p horch-e2e --test taskledger -- grs_03`
**Own:** `scripts/godot/gdcomplexity.py`, `scripts/godot/tests/test_gdcomplexity.py`, `crates/horch-core/src/taskledger/finish/complexity.rs`, `crates/horch-core/src/taskledger/finish/complexity/`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-e2e/tests/taskledger.rs`, `docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md`, `crates/horch-core/tests/godot_rust_support/complexity_gdscript.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 1, last 2 items, and Open Question 3).
Waits for: 01 (Scaffold the godot_rust_support test binary); 02 (complexity-sweep: nested Cargo crates and 3 clippy lints). Wave 3.

## Goal

complexity-sweep checks changed GDScript functions with `gdtoolkit.parser`, and its not-applicable note names every routed language and tool.

## Requirements

- **GRS-03**: `complexity-sweep` routes changed `.gd` files to `scripts/godot/gdcomplexity.py` (`gdtoolkit.parser`), reporting per-function `cyclomaticComplexity` (counting nested lambdas in their enclosing function) and `parameterCount` with exact 1-based start and end lines in the analyser JSON shape consumed by `in_changed_functions()`, reading optional thresholds from the nearest `static-analysis` config, and failing with an install hint when neither `uv` nor `python3` with `gdtoolkit` is available. Tests: grs_03_gdcomplexity_reports_exact_lines_and_metrics_for_changed_functions, grs_03_missing_gdscript_analyser_fails_with_install_hint; Python: test_gdcomplexity_counts_branches_and_lambdas, test_gdcomplexity_reads_config_thresholds.
- **GRS-04**: When no changed file matches a routed language, `complexity-sweep` reports `"no changed file is in a language that the analyser (TypeScript, JavaScript, Python), clippy (Rust) or gdcomplexity (GDScript) reads"`, and ADR 0086 records the nested Rust, 3-lint clippy, and GDScript `gdcomplexity` rules. Tests: grs_04_not_applicable_message_names_all_routed_languages_and_tools, grs_04_adr_0086_documents_rust_and_gdscript_routes.

## Steps

1. Write the Python tests first: `test_gdcomplexity_counts_branches_and_lambdas` and `test_gdcomplexity_reads_config_thresholds`. Run the script with `uv run --with gdtoolkit==4.5.0` when `uv` is on `PATH`, else with `python3` when `gdtoolkit` imports. When neither works, fail with the install hint. Do not skip.
2. Write `scripts/godot/gdcomplexity.py`. Follow the spec for `func_def` lines, the branch-node list, lambdas in their enclosing function, `parameterCount`, the default thresholds 10 and 7, `--config`, the analyser JSON shape and the trailing sentinel function.
3. Embed the script with `include_str!`. Resolve `finish::Tools.gdcomplexity_cli` from `HORCH_GDCOMPLEXITY_CLI`, else write the embedded copy to `<evidence_root>/bin/gdcomplexity.py`. Do not read `<repo>/scripts/godot/` at run time.
4. Route changed `.gd` files to it through `uv` or `python3`. With changed `.gd` files and no runner, return `Outcome::fail("no gdscript analyser", ...)` with `uv tool install gdtoolkit==4.5.0` or `pipx install "gdtoolkit==4.*"`.
5. Change the not-applicable note to the exact text of GRS-04.
6. Update ADR 0086 for nested crate grouping, the 3 clippy lints and the `gdcomplexity.py` route.
7. Write `grs_03_*` and `grs_04_*` in the submodule, and `grs_03_*` finish e2e in `crates/horch-e2e/tests/taskledger.rs` with `HORCH_GDCOMPLEXITY_CLI`.
8. Warm every new fake executable once with a `warm` argument before a test runs it (ADR 0111, `crates/horch-core/tests/common/warm.rs`). A submodule that needs it declares `#[path = "../common/warm.rs"] mod warm;` itself.
9. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
10. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
11. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. A changed tangled `.gd` function fails the step at its exact line. An untouched tangled function in the same file passes. A host without `uv` and `gdtoolkit` fails with the install hint.
