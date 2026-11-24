# 05: Rust skills: unsafe-review, async, cargo-workspace, error-handling

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test skills_catalog`
**Own:** `skills/rust-unsafe-review/`, `skills/rust-async/`, `skills/rust-cargo-workspace/`, `skills/rust-error-handling/`, `skills/copied.json`, `skills/README.md`, `crates/horch-core/tests/skills_catalog.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 3, second item (skills 1, 2, 3 and 7). GRS-07 is proved by ticket 07.).
Waits for: nothing. Wave 1.

## Goal

4 own-text Rust skills exist in the bundled catalog.

## Steps

1. Write `SKILL.md` for `rust-unsafe-review`, `rust-async`, `rust-cargo-workspace` and `rust-error-handling` from the topics in the spec. Write own text. Copy nothing.
2. Keep each `SKILL.md` at or below 12 KB and each directory at or below 160 KB. Put detail in `references/` when needed.
3. Register each skill in `skills/copied.json` with `"copied_files": []` and `"verbatim": false`, in `REPO_ORIGINAL` in `crates/horch-core/tests/skills_catalog.rs`, and in `skills/README.md`.
4. Run `horch ledger progress` after each skill.
5. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The 4 skills show in `horch skills` output.
