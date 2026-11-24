# 10: C# skills on godot-code-reviewer and godot-qa-engineer

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_11 && cargo test -p horch-core --test godot_wave`
**Own:** `teammates/godot-code-reviewer.md`, `teammates/godot-qa-engineer.md`, `docs/specs/godot.md`, `crates/horch-core/tests/godot_wave.rs`, `crates/horch-core/tests/godot_rust_support/csharp_seats.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 4, third item, and Open Question 4).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

Review and QA seats in a C# Godot project get the C# skills as expected skills.

## Requirements

- **GRS-11**: `godot-code-reviewer` and `godot-qa-engineer` set `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` while staying within the 5-to-11 expected-skills budget in both GDScript and C# projects, and `docs/specs/godot.md` (`GDW-03`) reflects that only `godot-tech-lead` omits `skills_when` for `*.csproj`. Tests: grs_11_csharp_project_adds_csharp_skills_to_godot_reviewer_and_qa_seats.

## Steps

1. Write `grs_11_csharp_project_adds_csharp_skills_to_godot_reviewer_and_qa_seats` first.
2. Add `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` to both seats.
3. On `godot-qa-engineer`, move `godot-bot-balance-loop` and `godot-visual-verification` from `skills:` to `available_skills:`.
4. Leave only `godot-tech-lead` in `NO_CSHARP_SEATS` in `godot_wave.rs`, and change its array length.
5. Update `GDW-03` in `docs/specs/godot.md`. Do not edit line 5 of that file. Ticket 15 owns it.
6. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
7. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. Both seats expect at most 11 skills in a C# project and at least 5 in a GDScript project.
