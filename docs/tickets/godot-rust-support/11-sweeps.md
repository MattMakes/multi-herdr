# 11: code-sweep and comment-sweep know Godot, C# and Rust

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib -- finish::codesweep finish::comments && cargo test -p horch-core --test godot_rust_support -- grs_12 grs_13 && python3 -m unittest discover -s scripts/comments/tests && python3 scripts/comments/comment_lint.py check`
**Own:** `crates/horch-core/src/taskledger/finish/codesweep.rs`, `crates/horch-core/src/taskledger/finish/comments.rs`, `skills/comment-pruning/references/pruning-pass.md`, `scripts/comments/comment_lint.py`, `scripts/comments/tests/test_comment_lint.py`, `scripts/comments/baseline.json`, `docs/adr/0195-code-and-comment-sweeps-adapt-to-project-languages.md`, `crates/horch-core/tests/godot_rust_support/sweeps.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 5, first 2 items, Open Question 2 and Further Notes).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

code-sweep sends Godot, Rust and Swift diffs to their domain reviewer and picks the right lenses. comment-sweep never edits Godot data files and keeps safety and doc comments.

## Requirements

- **GRS-12**: `code-sweep` triage triggers `g` (contracts) on GDScript/C#/gdext contract keywords (`class_name `, `signal `, `@export`, `public class `, `public struct `, `public interface `, `#[godot_api]`), `d` (dependencies) on `.csproj`, `Directory.Packages.props`, `project.godot`, `.gdextension`, and `plugin.cfg`, and `u` (UI) on `.tscn` and `.tres`; round-1 reviewer selection assigns the first review slot to the matching domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`) when available while keeping cross-vendor coverage on the remaining slots. Tests: grs_12_codesweep_triages_godot_and_csharp_signals_and_selects_domain_reviewer.
- **GRS-13**: `comment-sweep` excludes `.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, and `project.godot` in `comments::NOT_CODE`, `skills/comment-pruning/references/pruning-pass.md` explicitly preserves Rust `// SAFETY:` comments and Rust/GDScript doc comments (`///`, `//!`, `##`), and `scripts/comments/comment_lint.py` scans `.cs` files. Tests: grs_13_comment_sweep_skips_godot_data_files_and_keeps_safety_and_doc_comments.

## Steps

1. Write `grs_12_codesweep_triages_godot_and_csharp_signals_and_selects_domain_reviewer` and `grs_13_comment_sweep_skips_godot_data_files_and_keeps_safety_and_doc_comments` first.
2. Extend `contracts`, `touches_dependencies` and `touches_ui` in `codesweep.rs` with the exact strings of GRS-12.
3. Pick the domain reviewer in the order Godot, Rust, Swift. When it is not in `cx.tools.reviewers_down`, give it the first round-1 slot (the `b` lens) with the generic reviewer as fallback. Keep the other slots on `codex-reviewer` or `architect-reviewer`. Leave rounds 2 and 3 unchanged.
4. Add `.tscn`, `.tres`, `.import`, `.uid`, `.gdextension` and `project.godot` to `NOT_CODE` in `comments.rs`.
5. Add the Rust `// SAFETY:`, `///`, `//!` and GDScript `##` keep rules to `## Keep list` in `pruning-pass.md`.
6. Add `.cs` (`//` and `/* */`) to `comment_lint.py` with a test in `test_comment_lint.py`. Change `baseline.json` only if the scanner change needs it.
7. Write ADR 0195 for both decisions. Use the next free number if 0195 is taken, and say so in your summary.
8. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
9. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
10. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. A diff of only `.gd` files gives `godot-code-reviewer` the `b` slot and a cross-vendor reviewer on every other slot.
