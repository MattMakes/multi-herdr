# Map: godot-rust-support

Spec: `docs/specs/godot-rust-support.md`

| # | Title | Blocked by | Wave | GRS |
|---|---|---|---|---|
| 01 | [Scaffold the godot_rust_support test binary](01-test-scaffold.md) |  | 1 | - |
| 02 | [complexity-sweep: nested Cargo crates and 3 clippy lints](02-complexity-rust.md) | 01 | 2 | GRS-01, GRS-02 |
| 03 | [complexity-sweep: GDScript route through gdcomplexity.py](03-complexity-gdscript.md) | 01, 02 | 3 | GRS-03, GRS-04 |
| 04 | [doctor: Rust requirement, project-fact tool checks and Git LFS](04-doctor-rust.md) |  | 1 | GRS-05, GRS-08, GRS-16 |
| 05 | [Rust skills: unsafe-review, async, cargo-workspace, error-handling](05-rust-skills-a.md) |  | 1 | - |
| 06 | [Rust skills: perf, fuzz, testing](06-rust-skills-b.md) | 05 | 2 | - |
| 07 | [Rust seats: rust-developer, rust-reviewer, rust-qa-engineer](07-rust-seats.md) | 01, 04, 06 | 3 | GRS-06, GRS-07 |
| 08 | [godot-check.sh fails on script errors that exit 0](08-godot-check.md) | 01, 07 | 4 | GRS-09 |
| 09 | [godot-native-engineer owns gdext, and rust-gdext.md is complete](09-native-engineer.md) | 01, 06, 08 | 5 | GRS-10 |
| 10 | [C# skills on godot-code-reviewer and godot-qa-engineer](10-csharp-seats.md) | 01 | 2 | GRS-11 |
| 11 | [code-sweep and comment-sweep know Godot, C# and Rust](11-sweeps.md) | 01 | 2 | GRS-12, GRS-13 |
| 12 | [Suggest the default check for the project stack](12-stack-check.md) | 01, 08 | 5 | GRS-14 |
| 13 | [Measure worktree build and import cost, then seed the Godot cache](13-worktree-cache.md) | 01, 03 | 4 | GRS-15 |
| 14 | [Codex panes get a writable CARGO_HOME](14-codex-cargo-home.md) | 01 | 2 | GRS-17 |
| 15 | [Close the godot-run.sh Codex pane live check](15-godot-codex-live.md) | 01, 10 | 3 | GRS-18 |
| 16 | [Correct godot-perf-measure and the percentile convention](16-fix-perf-measure.md) | 01 | 2 | GRS-22, GRS-23, GRS-24, GRS-25, GRS-26 |
| 17 | [Correct godot-optimization CPU and draw-call claims](17-fix-optimization-cpu-draw.md) | 01 | 2 | GRS-27, GRS-28, GRS-29, GRS-30, GRS-31, GRS-32, GRS-33, GRS-34 |
| 18 | [Correct godot-optimization physics and pooling claims](18-fix-optimization-physics.md) | 01, 17 | 3 | GRS-35, GRS-36, GRS-37, GRS-38, GRS-39 |
| 19 | [Correct godot-multithreading thread and loading claims](19-fix-multithreading.md) | 01 | 2 | GRS-40, GRS-41, GRS-42 |
| 20 | [godot-hfr-hunt: SKILL.md, harness recorder and detection pass](20-hfr-skill.md) | 01, 08 | 5 | GRS-19, GRS-21 |
| 21 | [godot-hfr-hunt: the 4 technique reference files](21-hfr-references.md) | 01, 20 | 6 | GRS-19 |
| 22 | [hfr_bench.py, and wire godot-hfr-hunt to the performance engineer](22-hfr-bench.md) | 01, 21 | 7 | GRS-20, GRS-43, GRS-44 |
| 23 | [Full gate on the run branch](23-final-gate.md) | 03, 09, 11, 12, 13, 14, 15, 16, 18, 19, 22 | 8 | - |

## Waves

- Wave 1: 01, 04, 05
- Wave 2: 02, 06, 10, 11, 14, 16, 17, 19
- Wave 3: 03, 07, 15, 18
- Wave 4: 08, 13
- Wave 5: 09, 12, 20
- Wave 6: 21
- Wave 7: 22
- Wave 8: 23

## Critical path

05 -> 06 -> 07 -> 08 -> 20 -> 21 -> 22 -> 23

## Shared files

- `crates/horch-core/tests/skills_catalog.rs`: 05 -> 06 -> 07 -> 08 -> 20 -> 22. `skills/copied.json` and `skills/README.md`: 05 -> 06 -> 20.
- `crates/horch-core/src/taskledger/finish/complexity.rs` and `crates/horch-e2e/tests/taskledger.rs`: 02 -> 03. `crates/horch-core/src/taskledger/finish/mod.rs`: 03 -> 13.
- `skills/orchestrate/SKILL.md`: 08 -> 12. `docs/specs/godot.md`: 10 -> 15. `skills/godot-optimization/SKILL.md`: 17 -> 18.
- `crates/horch-core/tests/godot_rust_support.rs`: ticket 01 creates it with 1 submodule per ticket. Each later ticket owns only its submodule.

## Open questions

- GRS-15 changes code only when seeding saves more than 30 percent. Otherwise ticket 13 asks the orchestrator to amend the GRS-15 row.
- Tickets 13, 14 and 15 need a real Godot binary or Codex pane. Each sends BLOCKED when the host lacks one.
