# 04: doctor: Rust requirement, project-fact tool checks and Git LFS

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch -- doctor && cargo test -p horch-core --test godot_wave`
**Own:** `crates/horch-core/src/roster/offer.rs`, `crates/horch/src/cmd/doctor.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 2).
Waits for: nothing. Wave 1.

## Goal

`horch doctor` finds a missing Rust toolchain, a missing GDScript analyser and a missing `git-lfs` before a fleet run starts.

## Requirements

- **GRS-05**: `horch doctor` checks finish-step static-analysis tools from project facts even when no seat is offered: `cargo`, `rustc`, and `cargo clippy` when `Cargo.toml` is present, and `uv` or `python3` with `gdtoolkit` when `project.godot` is present. Tests: grs_05_doctor_checks_finish_step_tools_from_project_facts.
- **GRS-08**: `Requirement::Rust` exists and is spelled `rust`; `horch doctor` checks `cargo`, `rustc`, and `cargo clippy` when an offered seat requires `rust` and reports actionable install fixes when any of the three is missing or broken. Tests: grs_08_rust_requirement_is_spelled_rust_and_checked_by_doctor.
- **GRS-16**: In a Godot project whose `.gitattributes` contains `filter=lfs`, `horch doctor` checks that `git-lfs` is installed and working even when no seat explicitly lists `requires: [git-lfs]`. Tests: grs_16_doctor_checks_git_lfs_on_godot_project_with_lfs_rules.

## Steps

1. Write the tests first in the `#[cfg(all(test, unix))] mod tests` of `doctor.rs`: `grs_05_doctor_checks_finish_step_tools_from_project_facts`, `grs_08_rust_requirement_is_spelled_rust_and_checked_by_doctor` and `grs_16_doctor_checks_git_lfs_on_godot_project_with_lfs_rules`. Use hermetic temp directories and fake binaries on `PATH`.
2. Add `Requirement::Rust`, serialized as `rust`, in `offer.rs`.
3. In `doctor.rs`, check `cargo --version`, `rustc --version` and `cargo clippy --version` for `Requirement::Rust`. Give the `https://rustup.rs` hint and `rustup component add clippy`.
4. Run the same Rust check from `ProjectFacts` when `Cargo.toml` is present and no offered seat requires `rust`. Report it once when both apply.
5. When `project.godot` is present, check `uv` on `PATH` or `python3 -c "import gdtoolkit.parser"`. Warn with the complexity-sweep GDScript route and the install command.
6. When `Requirement::Godot` applies and `.gitattributes` contains `filter=lfs`, run `git_lfs_problem`.
7. Warm every new fake executable once with a `warm` argument before a test runs it (ADR 0111, `crates/horch-core/tests/common/warm.rs`). A submodule that needs it declares `#[path = "../common/warm.rs"] mod warm;` itself.
8. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
9. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
10. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. Each missing tool gives 1 warning with a copy-pasteable fix. A Godot project without `filter=lfs` gets no `git-lfs` check.
