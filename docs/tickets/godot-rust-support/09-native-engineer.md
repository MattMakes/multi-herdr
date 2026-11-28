# 09: godot-native-engineer owns gdext, and rust-gdext.md is complete

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_10 && cargo test -p horch-core --test godot_wave && python3 scripts/godot/xref_check.py && python3 scripts/godot/api_check.py --strict-own skills/copied.json skills/godot-gdextension/ && python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/copied.json skills/godot-gdextension/`
**Own:** `teammates/godot-native-engineer.md`, `skills/godot-gdextension/references/rust-gdext.md`, `crates/horch-core/tests/godot_rust_support/native_engineer.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 4, second item).
Waits for: 01 (Scaffold the godot_rust_support test binary); 06 (Rust skills: perf, fuzz, testing); 08 (godot-check.sh fails on script errors that exit 0). Wave 5.

## Goal

A `godot-native-engineer` in a Godot + Rust project launches with Rust skills and full `gdext` guidance.

## Requirements

- **GRS-10**: `godot-native-engineer` covers both C++ (`godot-cpp`) and Rust (`gdext`), sets `skills_when: {"Cargo.toml": [rust-cargo-workspace, rust-unsafe-review, rust-testing]}` alongside `"*.csproj"`, and `skills/godot-gdextension/references/rust-gdext.md` documents `.gdextension` platform library paths, `reloadable = true`, per-target Cargo builds, and headless test verification. Tests: grs_10_godot_native_engineer_adds_rust_skills_on_cargo_toml_and_gdext_ref_is_complete.

## Steps

1. Write `grs_10_godot_native_engineer_adds_rust_skills_on_cargo_toml_and_gdext_ref_is_complete` first.
2. Set the new `brief_description`. Add `"Cargo.toml": [rust-cargo-workspace, rust-unsafe-review, rust-testing]` to `skills_when` beside `"*.csproj"`. Add the 3 skills to `available_skills`. Keep the expected count within 5 to 11.
3. Rewrite the persona body to cover `godot-cpp` and `gdext`, with `cargo clippy`, `cargo test` and `godot-check.sh`.
4. Expand `rust-gdext.md` with `.gdextension` `[libraries]` per platform, `reloadable = true`, per-target `cargo build` and `cargo clippy`, and headless verification through `godot-check.sh`.
5. Check the `gdscript_blocks_check.py` arguments with `--help` before you run the Check. Correct the Check line in this ticket only if the script takes other arguments, and say so in your summary.
6. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
7. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. `rust-gdext.md` passes the strict Godot checks.
