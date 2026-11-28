# 06: Rust skills: perf, fuzz, testing

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test skills_catalog`
**Own:** `skills/rust-perf/`, `skills/rust-fuzz/`, `skills/rust-testing/`, `skills/copied.json`, `skills/README.md`, `crates/horch-core/tests/skills_catalog.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 3, second item (skills 4, 5 and 6). GRS-07 is proved by ticket 07.).
Waits for: 05 (Rust skills: unsafe-review, async, cargo-workspace, error-handling). Wave 2.

## Goal

The last 3 own-text Rust skills exist in the bundled catalog.

## Steps

1. Write `SKILL.md` for `rust-perf`, `rust-fuzz` and `rust-testing` from the topics in the spec. Write own text. Copy nothing.
2. Keep each `SKILL.md` at or below 12 KB and each directory at or below 160 KB.
3. Register each skill in `skills/copied.json`, `REPO_ORIGINAL` and `skills/README.md`, as ticket 05 did.
4. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. The 7 `rust-*` skills show in `horch skills` output.
