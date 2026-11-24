# Spec: Godot and Rust fleet support (GRS)

| | |
|---|---|
| Status | Proposed (not started). Planned requirements GRS-01 to GRS-44; Requirement Table (section 5) adopts the active `scripts/check-req-coverage.sh` column header (`Requirement`) alongside the tests that implement each requirement. |
| Date | 2026-11-24 |
| Author | spec-writer-1 |
| Inputs | Brief (`ai_docs/plans/godot-rust-support/brief.md`), operator decisions (`ai_docs/plans/godot-rust-support/decisions.md`), static-analysis report (`ai_docs/reports/godot-rust-static-analysis.md`, section 9 items 1, 2, 3, 5, 6), capability-audit report (`ai_docs/reports/godot-rust-capability-audit.md`, gaps G1 to G14), spec amendment brief (`ai_docs/plans/godot-rust-support/spec-amend-brief-2.md`), high frame rate (HFR) research reports (`ai_docs/reports/godot-hfr/1-script-cpu.md`, `2-render-gpu.md`, `3-physics-servers-threads.md`, `4-measure-harness.md`, `5-native-memory-hitches.md`). |
| Companion | [`docs/specs/godot.md`](godot.md) (GDW-01 to GDW-13) and [ADR 0086](../adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md). |

---

## Problem Statement

When a fleet runs on a Rust crate, a Godot project (GDScript or C#), or a hybrid Godot + Rust (`godot-rust` / `gdext`) project, several critical gaps in static analysis, domain seats, skills, finish sweeps, default checks, and host-tool validation weaken quality or hide defects:

1. **Static analysis (`complexity-sweep`) skips nested Rust crates and GDScript**:
   - The `complexity-sweep` finish step runs `cargo clippy` only when `Cargo.toml` sits at the root of the work directory. In a standard `godot-rust` (`gdext`) layout where the crate lives in a subfolder (such as `rust/Cargo.toml` beside `project.godot`), changed `.rs` files are silently skipped with a "not applicable" verdict.
   - When clippy does run on Rust, it checks only `clippy::cognitive_complexity` (a `restriction` lint that clippy's own documentation warns against using as a sole measurement tool) and omits function-length (`clippy::too_many_lines`) and argument-count (`clippy::too_many_arguments`) checks. Because those two lints span the whole function signature rather than just the identifier, the existing span-name slice extracts the full signature line instead of the function name, and deduplication by `(file, line)` drops a second metric violation on the same function.
   - Changed `.gd` files are completely skipped during `complexity-sweep`. Off-the-shelf GDScript tools either report no per-function complexity (`gdlint`, `godot --check-only`) or report wrong function start line numbers (`gdradon`), breaking changed-hunk filtering.
   - When no changed file matches a routed language, the "not applicable" message fails to name every routed language and tool.
2. **No Rust domain team, Rust skills, or Rust host-tool check**:
   - Unlike Swift (4 seats, 12 skills), Unreal (11 seats, 33 skills), and Godot (20 seats, 82 skills), Rust has zero specialist seats (`rust-developer`, `rust-reviewer`, `rust-qa-engineer`), zero bundled `rust-*` skills, no `offer_when: ["Cargo.toml"]` roster matching, and no `Requirement::Rust` (`requires: [rust]`) check in `horch doctor`.
   - `horch doctor` also has no project-fact checks for finish-step tools (`cargo` + `clippy` in a Cargo project; `uv` or `python3` with `gdtoolkit` in a Godot project) when no offered seat happens to carry a matching requirement.
3. **Godot correctness and `gdext` ownership**:
   - Headless Godot commands (`--import`, `--check-only`, and GUT test runs) frequently exit `0` even when scripts fail to parse (`SCRIPT ERROR`, `Parse Error`) or when zero tests run (`Nothing was run`). While `godot-testing` documents a log-grepping bash snippet in prose, `godot-build-verify` ships no executable verification script that enforces log-based failure detection as a land check.
   - `godot-native-engineer` describes itself and its workflow as C++ (`godot-cpp` and SCons) only, with no `skills_when` hook for `Cargo.toml` and only a brief 78-line `rust-gdext.md` reference that omits `.gdextension` Cargo library paths, per-platform target builds, hot reload, and headless engine tests.
   - In a C# Godot project (`*.csproj`), `skills_when` attaches `godot-csharp-godot` and `godot-csharp-signals` to the 15 builder seats, but omits `godot-code-reviewer` and `godot-qa-engineer` (under the original GDW-03 rule), leaving review and QA without the C# skills as expected skills.
4. **Language-blind finish sweeps (`code-sweep`, `comment-sweep`) and missing default stack checks**:
   - `code-sweep` always selects generic reviewers (`codex-reviewer` or `architect-reviewer`) and never routes domain diffs to `godot-code-reviewer`, `rust-reviewer`, or `swift-reviewer`. Its diff triage signals miss GDScript and C# contract keywords (`class_name`, `signal`, `@export`, `public`), Godot and C# dependency manifests (`*.csproj`, `project.godot`, `*.gdextension`, `plugin.cfg`, `addons/`), and Godot UI files (`*.tscn`).
   - `comment-sweep` treats Godot scene and asset metadata files (`.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, `project.godot`) as code files to be swept for comments, and `skills/comment-pruning/references/pruning-pass.md` does not explicitly protect Rust `// SAFETY:` comments (required by `clippy::undocumented_unsafe_blocks` and `unsafe impl ExtensionLibrary`) or identify GDScript `##` documentation comments.
   - `horch ledger run new` and `skills/orchestrate/SKILL.md` provide no default check command proposal or stack check table from project facts when `--check` is omitted.
5. **Operational and environment friction (G9, G11, G12, G13)**:
   - Worktrees start cold without measurement of Cargo build and Godot import costs or documented/seeded cache handling (G9).
   - Godot seats do not trigger a `git-lfs` doctor check when a Godot project uses Git LFS in `.gitattributes` (G11).
   - A `codex-sol` fallback pane running `cargo build` on a `gdext` crate may fail if Cargo attempts to write outside the workspace sandbox to `~/.cargo` (G12).
   - The live check of `godot-run.sh` inside a real Codex pane noted as pending in `docs/specs/godot.md` has not been recorded as closed (G13).
6. **No high frame rate (HFR) hunt for Godot**:
   - The operator wants a teammate to target 200 to 300 fps (5.00 ms, 4.17 ms and 3.33 ms frame budgets) to find code that needs optimization. No bundled skill gives that workflow, a benchmark harness with a pass/fail rule, or a detection pass.
   - The 5 HFR reports (`ai_docs/reports/godot-hfr/`) find 21 incorrect or incomplete claims in `skills/godot-perf-measure`, `skills/godot-optimization` and `skills/godot-multithreading`.

---

## Solution

Extend the fleet's static analysis, roster, skills, finish steps, doctor checks, and live checks so that Rust, Godot (GDScript and C#), and hybrid Godot + Rust (`gdext`) projects are first-class citizens:

1. **Static Analysis (`complexity-sweep`)**:
   - Discover nested Cargo crates by walking up from each changed `.rs` file to the nearest ancestor `Cargo.toml` within the work directory, grouping changed `.rs` files by crate root, running `cargo clippy` per crate root, and prefixing reported paths back to work-dir-relative paths.
   - Expand the clippy complexity pass to enforce `-W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`, mapping them to `cognitiveComplexity`, `functionLength`, and `parameterCount`, extracting the function identifier after `fn` from signature spans, and deduplicating by `(file_path, line, metric)`.
   - Add a GDScript complexity route for changed `.gd` files using a Python script (source `scripts/godot/gdcomplexity.py`, embedded in `horch-core` and written to `<evidence_root>/bin/gdcomplexity.py` at run time) powered by `gdtoolkit.parser` (`gdtoolkit==4.5.0`) that computes per-function `cyclomaticComplexity` (branch nodes + 1, including lambdas inside their enclosing function) and `parameterCount` with exact AST start and end lines, reading optional threshold overrides from the nearest `static-analysis` config and emitting the standard analyser JSON schema consumed by `in_changed_functions()`.
   - Update the `complexity-sweep` not-applicable message to name every routed language and tool: TypeScript, JavaScript, and Python (the analyser), Rust (clippy), and GDScript (gdcomplexity).
   - Record the finish-step design in an updated ADR 0086 (and a companion ADR for code-sweep and comment-sweep language awareness).
2. **Rust Team, Skills, and Host Doctor Checks (G1, G2, G8, Report A item 5)**:
   - Add three Rust seats in `teammates/`: `rust-developer` (Opus builder, fallback `codex-sol`), `rust-reviewer` (Opus read-only reviewer denying `Edit`, `Write`, `NotebookEdit`, fallback `codex-sol`), and `rust-qa-engineer` (Sonnet QA seat, fallback `codex-sol`), all with `offer_when: ["Cargo.toml"]` and `requires: [rust]`.
   - Add seven own-text bundled Rust skills in `skills/`: `rust-unsafe-review`, `rust-async`, `rust-cargo-workspace`, `rust-perf`, `rust-fuzz`, `rust-testing`, and `rust-error-handling`, registered in `skills/copied.json`, `REPO_ORIGINAL`, and `skills/README.md`.
   - Add `Requirement::Rust` (spelled `rust` in teammate frontmatter) checking `cargo`, `rustc`, and `cargo clippy`, and add finish-step tool checks in `horch doctor` keyed directly on project facts (`Cargo.toml` -> `cargo` and `cargo clippy`; `project.godot` -> `uv` or `python3` with `gdtoolkit`) independent of whether a seat is offered.
3. **Godot Correctness, `gdext` Ownership, and C# Reviewer/QA Skills (G4, G5, G14)**:
   - Ship an executable `skills/godot-build-verify/scripts/godot-check.sh` wrapper that runs `godot-run.sh` (import, project-wide script parse check, and optional GUT/gdUnit4 test suite) and fails with non-zero exit status if the engine exits non-zero or if the log contains `SCRIPT ERROR`, `Parse Error`, `ERROR: Failed to load script`, or `Nothing was run` (stripping ANSI escapes).
   - Expand `teammates/godot-native-engineer.md` to own both C++ (`godot-cpp`) and Rust (`godot-rust` / `gdext`), add `skills_when: {"Cargo.toml": [rust-cargo-workspace, rust-unsafe-review, rust-testing]}`, and expand `skills/godot-gdextension/references/rust-gdext.md` to cover `.gdextension` library paths per platform, `cargo build --target`, hot-reload configuration, and headless engine test verification.
   - Update `teammates/godot-code-reviewer.md`, `teammates/godot-qa-engineer.md`, and `docs/specs/godot.md` (GDW-03) so that `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` applies to `godot-code-reviewer` and `godot-qa-engineer` as well as the 15 builder seats.
4. **Language-Aware Sweeps and Default Stack Checks (G3, G6, G7)**:
   - Update `code-sweep` (`crates/horch-core/src/taskledger/finish/codesweep.rs`) to assign the first round-1 review slot to the domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`) when the task's changed files match that stack, keep the remaining slots on the cross-vendor reviewer (Open Question 2, Option 1), and expand triage signals: `contracts` (`class_name `, `signal `, `@export`, `public class`, `public interface`, `public struct`, `#[godot_api]`), `touches_dependencies` (`.csproj`, `Directory.Packages.props`, `project.godot`, `.gdextension`, `plugin.cfg`), and `touches_ui` (`tscn`, `tres`).
   - Update `comment-sweep` (`crates/horch-core/src/taskledger/finish/comments.rs`) to exclude Godot non-code data files (`.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, `project.godot`) in `NOT_CODE`, and update `skills/comment-pruning/references/pruning-pass.md` to keep Rust `// SAFETY:` comments on `unsafe` blocks/impls and GDScript `##` / Rust `///` and `//!` public doc comments.
   - Add stack-based default check suggestion/inference (`crates/horch/src/cmd/taskledger.rs` and `skills/orchestrate/SKILL.md`): when `horch ledger run new <slug>` is called without `--check`, print or apply the recommended check command from project facts (Rust: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`; Godot: `bash skills/godot-build-verify/scripts/godot-check.sh` or project equivalent; hybrid Godot+Rust: both), while preserving `--check` override and `**Check:** none`.
5. **Low-Severity Hardening and Live Checks (G9, G11, G12, G13)**:
   - Measure cold versus warm worktree Cargo build and Godot import times on a sample Godot + Rust project (`ai_docs/reports/godot-rust-worktree-timing.md`), and only if measured savings justify it, seed `.godot/imported/` on `horch ledger worktree new` and document `CARGO_TARGET_DIR` / `sccache` guidance (G9).
   - Check Git LFS in `horch doctor` for Godot projects whenever `.gitattributes` contains `filter=lfs` (G11).
   - Ensure Codex launches set a writable `CARGO_HOME` under the workspace or state root when `~/.cargo` is not writable in `workspace-write` mode, verified by a live check (G12).
   - Run and record `godot-run.sh` inside a real Codex pane in `docs/live-checks/godot-codex.md` and update `docs/specs/godot.md` status (G13).
6. **High Frame Rate Hunt Skill (`GRS-19` to `GRS-44`)**:
   - Add the own-text skill `skills/godot-hfr-hunt/`: a `SKILL.md` workflow, 1 reference file per research angle, a detection pass, and the benchmark harness `scripts/hfr_bench.py` from report 4.
   - Correct each claim that the reports find wrong in the existing Godot performance skills, 1 requirement row per correction.
   - Add `godot-hfr-hunt` to `teammates/godot-performance-engineer.md` within the 11-skill cap of `GDW-08`.

---

## User Stories

1. As a `godot-native-engineer` working on a `godot-rust` (`gdext`) project with `rust/Cargo.toml` and `project.godot`, I want `complexity-sweep` to find `rust/Cargo.toml` when I edit `rust/src/player.rs`, so that my Rust changes are analysed by clippy instead of silently skipped.
2. As a `rust-developer` working in a multi-crate repository with independent sub-crates in subfolders, I want `complexity-sweep` to group changed `.rs` files by their nearest ancestor `Cargo.toml` within the worktree and run `cargo clippy` once per crate group, so that every changed crate is checked against its own manifest and `clippy.toml`.
3. As a `rust-developer`, I want `complexity-sweep` to check `clippy::cognitive_complexity`, `clippy::too_many_lines`, and `clippy::too_many_arguments` in a single clippy invocation capped at `warn`, so that overly long functions and functions with too many parameters are caught alongside high cognitive complexity even if the crate uses `#![deny(warnings)]`.
4. As a `rust-developer`, I want `complexity-sweep` to report the clean function identifier (such as `tangled`) rather than the entire `pub fn tangled(...) -> u32 {` signature line when `too_many_lines` or `too_many_arguments` fires, so that violation messages are readable and consistent across metrics.
5. As a `rust-developer`, I want `complexity-sweep` to deduplicate clippy findings by `(file_path, line, metric)` rather than `(file_path, line)`, so that a function that violates both `cognitive_complexity` and `too_many_lines` reports both violations while duplicate emissions across lib and test targets are still collapsed.
6. As a `godot-gameplay-programmer` editing `.gd` scripts in a Godot 4.7 project, I want `complexity-sweep` to parse changed `.gd` files with `gdtoolkit.parser` and report functions whose cyclomatic complexity or parameter count exceeds the threshold, so that tangled GDScript functions are caught before landing.
7. As a `godot-gameplay-programmer`, I want the GDScript complexity analyser to report exact 1-based start and end line numbers for every `func` (counting nested lambdas toward their enclosing function), so that `keep_changed()` only flags functions whose lines were actually modified in my task's diff.
8. As a project maintainer, I want the GDScript complexity route to read optional `cyclomaticComplexity` and `parameterCount` thresholds from the nearest `static-analysis` configuration file (and default to `cyclomaticComplexity` threshold 10 and `parameterCount` threshold 7 when unconfigured), so that one config file can tune thresholds across languages.
9. As a worker running `horch ledger finish` on a task that only edits Markdown or JSON, I want the `complexity-sweep` not-applicable note to name TypeScript, JavaScript, Python, Rust, and GDScript, so that the log accurately states which languages the step routes.
10. As an operator running `horch doctor` in a project with `Cargo.toml` (at top level or 1 level down), I want doctor to verify that `cargo`, `rustc`, and `cargo clippy` are installed and runnable even when no seat is spawned yet, so that finish-step and build failures are caught before a fleet run starts.
11. As an operator running `horch doctor` in a Godot project (`project.godot`), I want doctor to verify that the GDScript complexity runner dependency (`uv` or `python3` with `gdtoolkit` 4.5.0) is available, and give a copy-pasteable install fix (`uv tool install gdtoolkit==4.5.0` or `pipx install "gdtoolkit==4.*"`) when missing, so that `complexity-sweep` on `.gd` files never fails unexpectedly mid-run.
12. As an orchestrator planning work on a Rust repository (`Cargo.toml`), I want `horch teammates` to offer `rust-developer`, `rust-reviewer`, and `rust-qa-engineer` via `offer_when: ["Cargo.toml"]`, so that I can assign Rust implementation, read-only review, and test-first QA to domain specialists.
13. As a `rust-developer`, `rust-reviewer`, or `rust-qa-engineer`, I want every Rust seat to declare `requires: [rust]` (`Requirement::Rust`), so that `horch doctor` reports a missing `cargo`, `rustc`, or `clippy` toolchain naming the seats that need it.
14. As a `rust-developer`, I want expected and available bundled skills covering `rust-unsafe-review`, `rust-async`, `rust-cargo-workspace`, `rust-perf`, `rust-fuzz`, `rust-testing`, and `rust-error-handling`, so that I follow fleet standards for `unsafe` `// SAFETY:` contracts, Miri, Tokio cancellation and `Send` bounds, Cargo workspace dependencies and features, Criterion/allocation profiling, `cargo fuzz`/proptest, nextest/doctests, and `thiserror`/`anyhow` error boundaries.
15. As a `rust-reviewer`, I want my teammate definition to deny `Edit`, `Write`, and `NotebookEdit` tools and load `code-review`, `security-review`, `rust-unsafe-review`, `rust-async`, `rust-error-handling`, and `rust-perf`, so that my reviews are strictly read-only and focused on ownership, `unsafe` soundness, async cancellation, and API contracts.
16. As a `rust-qa-engineer`, I want my teammate definition to load `check`, `debug`, `tdd`, `rust-testing`, `rust-fuzz`, and `rust-unsafe-review`, so that I write failing unit/integration/property tests first and verify fixes with `cargo test`.
17. As a `godot-qa-engineer` or orchestrator setting a run check for a Godot project, I want an executable `skills/godot-build-verify/scripts/godot-check.sh` script that runs headless `--import`, a project-wide `--check-only` script parse pass, and any configured GUT (`.gutconfig.json`) or gdUnit4 suite while scanning stripped logs for `SCRIPT ERROR`, `Parse Error`, `Failed to load script`, and `Nothing was run`, so that a land check never passes when Godot exits `0` on a broken script.
18. As a `godot-native-engineer` working on a Godot project that contains a `Cargo.toml` (at top level or 1 level down, such as `rust/Cargo.toml`), I want my briefing to cover both C++ (`godot-cpp`) and Rust (`godot-rust` / `gdext`) and `skills_when` to automatically add `rust-cargo-workspace`, `rust-unsafe-review`, and `rust-testing` to my expected skills, so that I have full Rust and GDExtension guidance at launch.
19. As a `godot-native-engineer` reading `skills/godot-gdextension/references/rust-gdext.md`, I want concrete patterns for `.gdextension` `[libraries]` per platform and build profile, `cargo build` target invocations, `reloadable = true` hot reload in Godot 4.2+, and headless test verification through `godot-check.sh`, so that I can build and verify a `gdext` library end-to-end.
20. As a `godot-code-reviewer` or `godot-qa-engineer` working on a C# Godot project (`*.csproj`), I want `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` on my seat (updating GDW-03), so that I automatically receive C# interop and signal lifetime rules as expected skills without manual `horch skills read` calls.
21. As a worker running `code-sweep` on a Godot or Rust diff, I want `code-sweep` to include `godot-code-reviewer` (when changed files touch `.gd`, `.tscn`, `.tres`, `.gdextension`, or `project.godot`) or `rust-reviewer` (when changed files touch `.rs` or `Cargo.toml`) as a reviewer persona when available, while keeping cross-vendor review coverage, so that domain-specific lifecycle, thread, `unsafe`, and ownership bugs are caught during `horch ledger finish`.
22. As a worker running `code-sweep` on a GDScript, Godot C#, or `gdext` diff, I want `codesweep::triage` to trigger the `g` (contracts) lens on `class_name `, `signal `, `@export`, `public class`, `public struct`, `public interface`, and `#[godot_api]`, the `d` (dependencies) lens on `.csproj`, `Directory.Packages.props`, `project.godot`, `.gdextension`, and `plugin.cfg`, and the `u` (UI) lens on `.tscn` and `.tres`, so that the right review lenses are selected for Godot and C# changes.
23. As a `rust-developer` or `godot-native-engineer` running `comment-sweep` on code with `unsafe` blocks or public APIs, I want `skills/comment-pruning/references/pruning-pass.md` to explicitly preserve Rust `// SAFETY:` comments (and `///` / `//!` doc comments) and GDScript `##` doc comments, so that `comment-remover` never deletes required safety justifications or editor documentation.
24. As a Godot developer running `comment-sweep`, I want `comments::NOT_CODE` to exclude `.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, and `project.godot`, so that `comment-remover` is never spawned to edit generated or structured Godot resource files.
25. As an orchestrator starting a ledger run with `horch ledger run new <slug>`, I want `horch` and `skills/orchestrate/SKILL.md` to suggest (or print in the error message when a task has no check and no `--check` was given) the canonical default check command for the detected stack (`Cargo.toml` -> `cargo test --workspace`; `project.godot` -> `godot-check.sh`), so that runs never accidentally use a raw `godot --headless` command that ignores script errors.
26. As an operator or engineer evaluating worktree startup time (G9), I want a repeatable measurement report comparing cold worktree `cargo check`/`cargo test` and `godot --headless --import` times against shared/seeded caches, and if seeding `.godot/imported/` on `horch ledger worktree new` saves significant wall time without corrupting imports, I want it enabled or documented cleanly.
27. As an operator running `horch doctor` in a Godot project whose `.gitattributes` uses `filter=lfs` (G11), I want doctor to verify `git-lfs` is installed and initialized even though `godot-*` seats do not unconditionally require `git-lfs` on non-LFS projects.
28. As a worker on a `codex-sol` fallback pane building a Rust or `gdext` crate (G12), I want `CARGO_HOME` to remain writable (or fall back to a workspace/state-root cargo directory when `~/.cargo` is read-only under the Codex sandbox), so that `cargo build` and `cargo clippy` succeed inside a Codex pane.
29. As an operator maintaining `docs/specs/godot.md` (G13), I want the pending real Codex pane check for `godot-run.sh` (`scripts/live/godot-codex.sh`) executed or verified and recorded in `docs/live-checks/godot-codex.md` and `docs/specs/godot.md`, closing the last open item from the Godot wave.
30. As a `godot-performance-engineer` with a 200 to 300 fps target, I want a skill that uncaps the frame rate, runs a fixed benchmark scenario, reads frame-time percentiles and sends me to the technique for the thread that owns the cost, so that I optimize the measured hot spot and not a guess.
31. As an orchestrator setting a performance check, I want `hfr_bench.py compare` to print 1 exact verdict and exit non-zero on a frame-time regression or a budget failure, so that a slower candidate cannot land without notice.

---

## Test Seams

Following `docs/specs/godot.md` and `docs/testing-and-gates.md`, every requirement ID (`GRS-01` to `GRS-44`) is pinned by at least one Rust test whose function name starts with `grs_<nn>_` so that `scripts/check-req-coverage.sh` enforces 100% requirement coverage in `just gate`. We use four existing seams and one new integration test file (`crates/horch-core/tests/godot_rust_support.rs`):

1. **Primary Seam — Roster, Skills, and Finish Unit/Integration Suite (`crates/horch-core/tests/godot_rust_support.rs`)**:
   - **Test prefix**: `grs_01_` through `grs_44_`.
   - **What it tests**:
     - Complexity routing, nested Cargo crate discovery, multi-lint clippy JSON parsing (`cognitive_complexity`, `too_many_lines`, `too_many_arguments`), signature-span function name extraction, `(file, line, metric)` deduplication, GDScript `gdcomplexity.py` invocation and JSON output integration with `in_changed_functions()`, and the updated not-applicable message (`GRS-01` to `GRS-04`).
     - Rust team roster presence (`rust-developer`, `rust-reviewer`, `rust-qa-engineer`), `offer_when: ["Cargo.toml"]`, `Requirement::Rust` (`requires: [rust]`), tool denials on `rust-reviewer`, and the 7 `rust-*` skills in the bundled skill catalog (`GRS-06`, `GRS-07`, `GRS-08`).
     - `godot-check.sh` exit code behavior against fake Godot engines that exit `0` while printing `SCRIPT ERROR`, `Parse Error`, `Failed to load script`, or `Nothing was run` with ANSI color codes (`GRS-09`).
     - `godot-native-engineer` briefing and `skills_when: {"Cargo.toml": ...}` resolution, plus `rust-gdext.md` content checks (`GRS-10`).
     - `godot-code-reviewer` and `godot-qa-engineer` receiving `godot-csharp-godot` and `godot-csharp-signals` in a `*.csproj` project (`GRS-11`, updating `gdw_03_*` in `crates/horch-core/tests/godot_wave.rs`).
     - `codesweep::triage` lens selection on GDScript, C#, and `.tscn` diffs and domain reviewer selection (`GRS-12`).
     - `comments::is_code` rejecting `.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, and `project.godot`, and `pruning-pass.md` keeping `// SAFETY:` and `##` doc comments (`GRS-13`).
     - Stack default check suggestion from `ProjectFacts` (`GRS-14`).
     - Worktree `.godot/imported` seeding / timing report verification, Git LFS doctor check on `.gitattributes`, Codex `CARGO_HOME` environment preparation, and live-check doc closure (`GRS-15` to `GRS-18`).
     - `godot-hfr-hunt` layout, workflow steps, reference sections and detection content (`GRS-19`, `GRS-21`), `hfr_bench.py` through its Python suite (`GRS-20`), the old and new claim text in each corrected skill file (`GRS-22` to `GRS-42`), the `godot-performance-engineer` skill lists (`GRS-43`), and catalog registration (`GRS-44`).
2. **Existing Seam — Doctor Unit Tests (`crates/horch/src/cmd/doctor.rs`)**:
   - **Test prefix**: `grs_05_`, `grs_08_`, `grs_16_`.
   - **What it tests**: `Requirement::Rust` (`cargo`, `rustc`, `cargo clippy` checks) and project-fact finish-step tool warnings (`Cargo.toml` without `cargo`/`clippy`; `project.godot` without `uv`/`gdtoolkit`; `project.godot` + `.gitattributes` `filter=lfs` without `git-lfs`) using hermetic temp directories and warmed fake binaries.
3. **Existing Seam — Task-Ledger E2E Finish Suite (`crates/horch-e2e/tests/taskledger.rs`)**:
   - **Test prefix**: `grs_01_`, `grs_03_`.
   - **What it tests**: Full `horch ledger finish` runs on a worktree with a nested `rust/Cargo.toml` crate and a `.gd` script change.
4. **Existing Seam — Godot Python Gate (`scripts/godot/tests/`)**:
   - **Test prefix**: `test_gdcomplexity_*`, `test_godot_check_*` and `test_hfr_bench_*` in `scripts/godot/tests/`, invoked both by `just gate` (`godot_skills` step) and from `grs_03_` / `grs_09_` / `grs_20_` in `crates/horch-core/tests/godot_rust_support.rs`.

---

## Implementation Decisions

### 1. Static Analysis (`complexity-sweep`)

- **Nested Cargo crate discovery (Report A item 1, G10 -> `GRS-01`)**:
  - Replace the top-level `dir.join("Cargo.toml").is_file()` gate in `complexity::rust_sources` with ancestor discovery modeled on `complexity::config::nearest`: for each changed `.rs` file in `git diff --name-only --diff-filter=AMR base...HEAD`, walk from the file's parent directory up to the work directory (`cx.dir`) to find the nearest `Cargo.toml`.
  - Group changed `.rs` files by their nearest `Cargo.toml` directory (`BTreeMap<PathBuf, Vec<String>>`). Files with no `Cargo.toml` at or under `cx.dir` are ignored.
  - For each crate group `(crate_dir, group_files)`, run `cargo clippy` with `current_dir(&crate_dir)` and write output to `<seq>-clippy.json` (for group 0) or `<seq>-clippy-<n+1>.json` (for subsequent groups).
  - Normalize each `span.file_name` in `clippy_violations`: strip `crate_dir` if absolute, then join with `crate_dir.strip_prefix(&cx.dir)` so `Violation.file_path` is always relative to the work directory `cx.dir` (e.g., `rust/src/lib.rs`), matching the keys in `changed_lines(&diff)` and `rust_starts(&cx.dir, &group_files)`.
- **Clippy lint set, span name extraction, and deduplication (Report A item 2 -> `GRS-02`)**:
  - Pass three lints to `cargo clippy`: `-A clippy::all -W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`.
  - Map clippy lint codes to violation metric names:
    - `clippy::cognitive_complexity` -> `"cognitiveComplexity"`
    - `clippy::too_many_lines` -> `"functionLength"`
    - `clippy::too_many_arguments` -> `"parameterCount"`
  - Extract `(value, threshold)` via the existing `score(&d.message)` parser, which reads the trailing `(value/threshold)` pattern common to all three lints.
  - Update `name(span: &Span)` so that when the sliced span text contains whitespace or `fn` (as in signature spans emitted by `too_many_lines` and `too_many_arguments`), it finds the `fn` keyword token and takes the identifier after `fn` up to the first `<` or `(`, falling back to the raw span slice for single-identifier spans.
  - Deduplicate findings in `clippy_violations` by `(v.file_path, v.line, v.metric)` instead of `(v.file_path, v.line)`, so two different lints on the same function are both kept while duplicate target compilations (`--all-targets` compiling `lib` and `test`) are deduplicated.
- **GDScript complexity route via `gdtoolkit.parser` (Report A item 3 -> `GRS-03`)**:
  - Ship an own-text Python script `scripts/godot/gdcomplexity.py`, embed it in `horch-core` with `include_str!`, and resolve it on `finish::Tools.gdcomplexity_cli` from `HORCH_GDCOMPLEXITY_CLI` when set, else from the embedded copy written to `<evidence_root>/bin/gdcomplexity.py` (Open Question 3, Option 1). Do not look up `<repo>/scripts/godot/gdcomplexity.py` at run time.
  - Run it on changed `.gd` files via `uv run --with gdtoolkit==4.5.0 python3 -I <gdcomplexity.py> --format json [--config <cfg>] <files>` when `uv` is available on `PATH`, or `python3 -I <gdcomplexity.py> ...` when `gdtoolkit` is importable directly in `python3` (e.g., via `pipx` or system site-packages; if `-I` hides user site-packages when not using `uv`, fall back to `python3 -B` without `-I` when `uv` is absent).
  - `gdcomplexity.py` parses each `.gd` file with `gdtoolkit.parser.parser.parse(source, gather_metadata=True)`. For every `func_def` node (top-level and inner-class methods):
    - `line` is `func_def.meta.line` and `end_line` is `func_def.meta.end_line`.
    - `cyclomaticComplexity` is `1` plus the count of branching AST nodes inside the function body (including any nested `lambda` inside that `func_def`, excluding nested inner `class_def` methods which are counted as their own functions): `if_branch`, `elif_branch`, `for_stmt`, `for_stmt_typed`, `while_stmt`, `match_branch`, `and_test`, `or_test`, `asless_and_test`, `asless_or_test`, `ternary_expr`, `asless_ternary_expr`, `pattern_guard`.
    - `parameterCount` is the number of `func_arg_regular`, `func_arg_inf`, and `func_arg_typed` children in the function's `func_args`.
  - Thresholds default to `cyclomaticComplexity: 10` and `parameterCount: 7`. When `--config <path>` points to a JSON/YAML `static-analysis` config (grouped by `complexity::config::groups`), `gdcomplexity.py` reads any configured `cyclomaticComplexity` or `parameterCount` warn/error threshold from it.
  - `gdcomplexity.py` outputs the exact JSON schema that `complexity::in_changed_functions()` already parses (`files: [{filePath, functions: [{name, line}]}]`, `violations: [{functionName, filePath, line, metric, value, threshold}]`), plus a synthetic trailing sentinel function at `end_line + 1` when top-level statements follow a function so `keep_changed()` bounds each function accurately.
  - If changed `.gd` files exist and neither `uv` (with `gdtoolkit`) nor `python3` with `gdtoolkit` can run `gdcomplexity.py`, `complexity-sweep` returns `Outcome::fail("no gdscript analyser", ...)` with the install command (`uv tool install gdtoolkit==4.5.0` or `pipx install "gdtoolkit==4.*"`), never silently skipping `.gd` files (respecting ADR 0086).
- **Updated not-applicable message and ADR 0086 (Report A items 2, 3, 6 -> `GRS-04`)**:
  - When no changed file is in `EXTENSIONS`, `.rs` (with a `Cargo.toml`), or `.gd`, `complexity::step` returns:
    `"no changed file is in a language that the analyser (TypeScript, JavaScript, Python), clippy (Rust) or gdcomplexity (GDScript) reads"`.
  - Update `docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md` to record nested `Cargo.toml` grouping, the three clippy lints (`cognitive_complexity`, `too_many_lines`, `too_many_arguments`), and the `gdcomplexity.py` (`gdtoolkit.parser`) route for GDScript.

### 2. Host Tools and Doctor Checks (`GRS-05`, `GRS-08`, `GRS-16`)

- **`Requirement::Rust` (`GRS-08`)**:
  - Add `Rust` to `horch_core::roster::Requirement` (`crates/horch-core/src/roster/offer.rs`), serialized as `"rust"`.
  - In `crates/horch/src/cmd/doctor.rs`, include `Requirement::Rust` in `requirement_problems()`. `problems_for(Requirement::Rust, ...)` checks:
    1. `cargo --version` on `PATH` (or reports missing `cargo` with `https://rustup.rs` install hint).
    2. `rustc --version` on `PATH`.
    3. `cargo clippy --version` (or reports missing clippy component with `rustup component add clippy`).
- **Finish-step tool doctor warnings keyed on project facts (Report A item 5 -> `GRS-05`)**:
  - Because `complexity-sweep` runs for every worker regardless of which teammate is offered, `horch doctor` runs a project-fact check whenever a project directory is present:
    - If `ProjectFacts` for the project matches `"Cargo.toml"`, run the Rust toolchain check (`cargo`, `rustc`, `cargo clippy`) even if no offered seat lists `requires: [rust]` (or deduplicate with `Requirement::Rust` when `rust-*` seats are offered).
    - If `ProjectFacts` matches `"project.godot"`, check that `gdcomplexity` can run (`uv` on `PATH` or `python3 -c "import gdtoolkit.parser"` succeeds); if neither is present, emit a doctor warning naming the `complexity-sweep` GDScript route and how to install `uv` or `gdtoolkit==4.5.0`.
- **Git LFS doctor check for Godot projects with LFS rules (G11 -> `GRS-16`)**:
  - In `crates/horch/src/cmd/doctor.rs`, when `requirement == Requirement::Godot` and the project's `.gitattributes` contains `filter=lfs`, run `git_lfs_problem(path, pathext, git)` so a Godot project that tracks binary assets with Git LFS gets the `git-lfs` binary check without forcing non-LFS Godot projects to install `git-lfs`.

### 3. Rust Team and Skills (G1, G2 -> `GRS-06`, `GRS-07`)

- **Three Rust seats (`GRS-06`)**:
  - Create three teammates in `teammates/`:
    1. `teammates/rust-developer.md`: `base: fleet-worker`, `agent: claude`, `phase: implementation`, `model: opus`, `system_prompt: sr_eng_system_prompt`, `offer_when: ["Cargo.toml"]`, `requires: [rust]`, `fallbacks: [codex-sol]`, `effort: medium`, `compact_window: 200000`, `compact_at: 300000`, `permission_mode: auto`, `inherit_plugins: false`, `mcp_servers: {}`, `disallowed_tools: [Agent]`, `disabled_skills: [herdr-orchestrator, herdr-worker]`. `skills: [tdd, rust-cargo-workspace, rust-error-handling, rust-async, rust-testing, rust-unsafe-review]`, `available_skills: [rust-perf, rust-fuzz, security-review, api-contracts]`.
    2. `teammates/rust-reviewer.md`: `base: fleet-worker`, `agent: claude`, `phase: validation`, `model: opus`, `system_prompt: sr_eng_system_prompt`, `offer_when: ["Cargo.toml"]`, `requires: [rust]`, `fallbacks: [codex-sol]`, `effort: high`, `compact_window: 200000`, `compact_at: 300000`, `permission_mode: auto`, `inherit_plugins: false`, `mcp_servers: {}`, `disallowed_tools: [Agent, Edit, Write, NotebookEdit]`, `disabled_skills: [herdr-orchestrator, herdr-worker]`. `skills: [code-review, security-review, rust-unsafe-review, rust-async, rust-error-handling, rust-perf]`, `available_skills: [rust-cargo-workspace, rust-testing, rust-fuzz]`.
    3. `teammates/rust-qa-engineer.md`: `base: fleet-worker`, `agent: claude`, `phase: validation`, `model: sonnet`, `offer_when: ["Cargo.toml"]`, `requires: [rust]`, `fallbacks: [codex-sol]`, `effort: high`, `compact_window: 150000`, `compact_at: 300000`, `permission_mode: auto`, `inherit_plugins: false`, `mcp_servers: {}`, `disallowed_tools: [Agent]`, `disabled_skills: [herdr-orchestrator, herdr-worker]`. `skills: [check, debug, tdd, rust-testing, rust-fuzz, rust-unsafe-review]`, `available_skills: [rust-cargo-workspace, rust-async, rust-perf]`.
  - Add `"rust-developer"`, `"rust-qa-engineer"`, and `"rust-reviewer"` in alphabetical order to `SKIP_NEW_TEAMMATES` in `crates/horch-core/tests/baseline_oracles.rs` and `crates/horch-core/tests/skills_catalog.rs`, and document the Rust team in `teammates/README.md` and `docs/skills-and-teams.md`.
- **Seven bundled own-text Rust skills (`GRS-07`)**:
  - Create seven own-text skills under `skills/` (each with `SKILL.md` <= 12 KB, directory <= 160 KB, `"copied_files": []`, `"verbatim": false` in `skills/copied.json`, listed in `REPO_ORIGINAL` in `crates/horch-core/tests/skills_catalog.rs` and in `skills/README.md`):
    1. `rust-unsafe-review`: `// SAFETY:` comment contracts on every `unsafe` block and `unsafe impl`, pointer provenance and aliasing rules, FFI/GDExtension boundary invariants, and running `cargo +nightly miri test` when available.
    2. `rust-async`: ownership across `.await`, cancellation safety in `tokio::select!`, `Send`/`Sync` bounds, avoiding blocking calls on async executors, and bounded channels/backpressure.
    3. `rust-cargo-workspace`: workspace dependency inheritance (`[workspace.dependencies]`), additive feature flags, MSRV verification, lockfile discipline, and `cargo clippy --workspace --all-targets -- -D warnings`.
    4. `rust-perf`: allocation profiling, borrowing vs cloning on hot paths, iterator vs indexed loop trade-offs, `cargo bench` / Criterion baselines, and avoiding premature `unsafe`.
    5. `rust-fuzz`: property testing (deterministic PRNG / `proptest`) and `cargo fuzz` / `libfuzzer` harness design, shrinking counterexamples into deterministic regression unit tests.
    6. `rust-testing`: unit, integration, and doc tests, hermetic temp-dir fixtures, `cargo test --workspace --no-fail-fast` and `cargo nextest`, and testing error variants rather than `.unwrap()`.
    7. `rust-error-handling`: library typed errors vs application contextual errors (`thiserror` / hand-written `Display`+`Error` vs `anyhow::Context`), preserving error chains (`{e:#}`), and no panics (`unwrap`/`expect`) on untrusted input.

### 4. Godot Correctness, `gdext` Ownership, and C# Skills (G4, G5, G14 -> `GRS-09`, `GRS-10`, `GRS-11`)

- **Executable `godot-check.sh` land check script (G4 -> `GRS-09`)**:
  - Add `skills/godot-build-verify/scripts/godot-check.sh` (executable, `0755`, added to `EXEMPT_FILES` / `EXEMPT_FROM_TEXT_ONLY` in `crates/horch-core/tests/skills_catalog.rs` if required alongside `godot-run.sh`, with zero comments if scanned by `comment_lint.py`).
  - `godot-check.sh` runs from a Godot project root (where `./project.godot` exists) using `godot-run.sh`:
    1. Runs `godot-run.sh --import` and checks its log.
    2. If the project is C# (`is_csharp`), runs `dotnet build --nologo`.
    3. Runs a project-wide GDScript parse check (`godot-run.sh --check-only --script <each .gd outside .godot/ and addons/>`, or the project's parse check runner).
    4. If `.gutconfig.json` or `addons/gut/gut_cmdln.gd` exists (or `--gut` is passed), runs `godot-run.sh -s res://addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json`.
    5. Strips ANSI escape sequences from every captured log and fails with exit code `3` if any log matches `SCRIPT ERROR`, `Parse Error`, `Failed to load script`, or `Nothing was run`, or exits with the underlying non-zero engine/dotnet exit code.
  - Reference `godot-check.sh` in `skills/godot-build-verify/SKILL.md`, `skills/godot-testing/references/headless-gut.md`, and `skills/orchestrate/SKILL.md`.
- **`godot-native-engineer` and `rust-gdext.md` expansion (G5 -> `GRS-10`)**:
  - Update `teammates/godot-native-engineer.md`:
    - `brief_description`: `"Godot 4 native code: GDExtension in C++ (godot-cpp) and Rust (gdext), bindings, SCons/Cargo builds, threads."`
    - Add `"Cargo.toml": [rust-cargo-workspace, rust-unsafe-review, rust-testing]` to `skills_when:` alongside `"*.csproj": [godot-csharp-godot, godot-csharp-signals]`.
    - Add `rust-cargo-workspace`, `rust-unsafe-review`, and `rust-testing` to `available_skills:` so they are visible in non-Cargo Godot projects and promoted to `skills:` when `Cargo.toml` is present (keep total expected skills within the GDW-08 5-to-11 limit: 6 base skills + 3 Rust skills = 9, or + 2 C# skills = 11).
    - Update the persona body to cover both `godot-cpp` (C++ / SCons) and `godot-rust` (`gdext` / Cargo), checking `cargo clippy` and `cargo test` alongside `godot-check.sh`.
  - Expand `skills/godot-gdextension/references/rust-gdext.md` (own text, passing strict Godot checks) with:
    - Multi-platform `.gdextension` `[libraries]` paths (`macos.debug`, `linux.debug.x86_64`, `windows.debug.x86_64`) pointing to `res://../rust/target/debug/` or `res://rust/target/debug/`.
    - `reloadable = true` under `[configuration]` for Godot 4.2+ hot reload.
    - Per-platform `cargo build` and `cargo clippy` commands.
    - Headless verification of registered `GodotClass` nodes via `godot-check.sh` and GDScript smoke tests.
- **C# `skills_when` for `godot-code-reviewer` and `godot-qa-engineer` (G14 -> `GRS-11`)**:
  - Add `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` to `teammates/godot-code-reviewer.md` (base 8 skills + 2 = 10 <= 11) and `teammates/godot-qa-engineer.md` (adjust base skills from 11 to 9 by moving `godot-bot-balance-loop` and `godot-visual-verification` to `available_skills:` so that in a `*.csproj` project, 9 + 2 = 11 expected skills satisfies GDW-08's 5-to-11 invariant).
  - Update `docs/specs/godot.md` requirement `GDW-03` and `NO_CSHARP_SEATS` in `crates/horch-core/tests/godot_wave.rs` so only `godot-tech-lead` (which already names `godot-language-choice` and coordinates architecture rather than writing/testing C# code) is in `NO_CSHARP_SEATS`.

### 5. Sweeps and Default Checks (G3, G6, G7 -> `GRS-12`, `GRS-13`, `GRS-14`)

- **Language-aware `code-sweep` reviewer selection and triage signals (G3 -> `GRS-12`)**:
  - In `crates/horch-core/src/taskledger/finish/codesweep.rs`:
    - Expand `contracts(s: &Signals<'_>)` to also match `"class_name "`, `"signal "`, `"@export"`, `"public class "`, `"public struct "`, `"public interface "`, `"#[godot_api]"`.
    - Expand `touches_dependencies(s: &Signals<'_>)` to also match `".csproj"`, `"Directory.Packages.props"`, `"project.godot"`, `".gdextension"`, `"plugin.cfg"`.
    - Expand `touches_ui(s: &Signals<'_>)` `UI` extensions to include `"tscn"` and `"tres"`.
    - Add domain-reviewer selection for round 1 when changed files match a domain stack:
      - If changed files contain `.gd`, `.tscn`, `.tres`, `.gdextension`, or `project.godot`, the domain reviewer is `"godot-code-reviewer"`.
      - Else if changed files contain `.rs` or `Cargo.toml`, the domain reviewer is `"rust-reviewer"`.
      - Else if changed files contain `.swift` or `Package.swift`, the domain reviewer is `"swift-reviewer"`.
      - When a domain reviewer is selected and not listed in `cx.tools.reviewers_down`, assign the first selected review task (the `"b"` bug-hunter correctness lens) to the domain reviewer persona (with the standard cross-vendor generic reviewer as fallback), while keeping the remaining review tasks on the cross-vendor generic reviewer (`codex-reviewer` / `architect-reviewer`) so every review round retains both domain expertise and cross-vendor diversity.
  - Record this finish-step design in a new ADR in `docs/adr/`.
- **Language-aware `comment-sweep` exclusions and keep rules (G6 -> `GRS-13`)**:
  - In `crates/horch-core/src/taskledger/finish/comments.rs`, add `".tscn"`, `".tres"`, `".import"`, `".uid"`, `".gdextension"`, and `"project.godot"` to `NOT_CODE` so Godot scene, resource, and engine metadata files are never sent to `comment-remover`.
  - In `skills/comment-pruning/references/pruning-pass.md`, update the `## Keep list` to explicitly specify:
    - Under item 5 (or a dedicated safety item): Rust `// SAFETY:` comments explaining why an `unsafe` block or `unsafe impl` upholds its safety invariants (required by `clippy::undocumented_unsafe_blocks`), Rust `///` and `//!` doc comments on public items, and GDScript `##` doc comments on classes, signals, properties, and functions.
  - In `scripts/comments/comment_lint.py`, add `.cs` (`//` and `/* */`) to the language comment scanner so multi-herdr's own comment lint covers C# files if any are added.
- **Default check per stack (G7 -> `GRS-14`)**:
  - Add a pure function `suggested_check(facts: &ProjectFacts) -> Option<&'static str>` in `horch_core::roster` (or `taskledger::finish`):
    - Both `"project.godot"` and `"Cargo.toml"` in `facts`: `Some("cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings")` (plus `godot-check.sh` noted for Godot scenes/scripts).
    - `"project.godot"` in `facts`: `Some("bash skills/godot-build-verify/scripts/godot-check.sh")`.
    - `"Cargo.toml"` in `facts`: `Some("cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings")`.
    - `"Package.swift"` in `facts`: `Some("swift test")`.
  - When `horch ledger run new <slug>` is called without `--check` (and when `gate_cmd` or `land::check_command` reports a missing check), print a `NOTE:` or error hint naming the suggested check command for the project's detected stack. Document the stack check table in `skills/orchestrate/SKILL.md` §0 step 3.

### 6. Low-Severity Hardening and Live Checks (G9, G11, G12, G13 -> `GRS-15` to `GRS-18`)

- **Worktree cache measurement and `.godot/imported` seeding (G9 -> `GRS-15`)**:
  - First measure cold vs warm worktree build/import times on a representative Rust crate and Godot project and write the evidence to `ai_docs/reports/godot-rust-worktree-timing.md`.
  - In `crates/horch-core/src/taskledger/finish/mod.rs` (`create_worktree`) and `crates/horch-core/src/vcs/landing.rs` (`add_worktree`), if `<root>/.godot/imported` and `<root>/.godot/uid_cache.bin` (or `global_script_class_cache.cfg`) exist in the base repository, copy them into `<worktree>/.godot/` on worktree creation so headless Godot workers do not re-import unchanged assets from scratch. Do not share `CARGO_TARGET_DIR` across worktrees (per `docs/testing-and-gates.md:210`, a shared `CARGO_TARGET_DIR` across checkouts can bake stale `build.rs` paths).
- **Codex sandbox `CARGO_HOME` fallback (G12 -> `GRS-17`)**:
  - In `crates/horch-core/src/harness/codex.rs` (or `teammates/_base/harness-defaults.md` / `godot-run.sh`), verify via a test and live check whether a Codex `workspace-write` pane can run `cargo check` / `cargo clippy` when dependencies are already fetched vs when `~/.cargo` needs a lock write. Set or document `CARGO_HOME` fallback or `--offline` / workspace-local cargo cache handling if `~/.cargo` writes are blocked.
- **Close pending `godot-run.sh` real Codex pane live check (G13 -> `GRS-18`)**:
  - Run `scripts/live/godot-codex.sh` inside a real Codex pane (or verify the recorded `B-fixed` run in `docs/live-checks/godot-codex.md` against the wrapper script), record the `godot-run.sh` pane run in `docs/live-checks/godot-codex.md`, and update line 5 of `docs/specs/godot.md` to mark the live check complete.

### 7. High Frame Rate Hunt Skill (`GRS-19` to `GRS-44`)

- **Skill layout and workflow (`GRS-19`)**:
  - Create the own-text skill `skills/godot-hfr-hunt/` (`SKILL.md` <= 12 KB, directory <= 160 KB). The skill is written from the 5 reports in `ai_docs/reports/godot-hfr/` and keeps their source links. It does not copy the reports' "Corrections" sections. `GRS-22` to `GRS-42` put those in the skills they correct.
  - `SKILL.md` holds this workflow, in this order:
    1. **Uncap.** Run a release export in a foreground window with `--disable-vsync --max-fps 0`. Assert `Engine.max_fps == 0`, `OS.low_processor_usage_mode == false` (its default 6,900 microsecond sleep caps a run near 145 fps), `application/run/frame_delay_msec == 0` and no `--fixed-fps`. Reject a run whose frame times cluster at the display period or at 6.9 ms. Do not time an editor-hosted or `--headless` run for rendering (`--headless` uses the dummy renderer).
    2. **Run the harness.** Use `scripts/hfr_bench.py` (`GRS-20`) on 1 fixed scenario: settle, warm up for 5 s, record for 30 s.
    3. **Read the percentiles.** Read p50, p95, p99, worst frame and 1% low against the 5.00 ms (200 fps), 4.17 ms (240 fps) and 3.33 ms (300 fps) budgets. A p99 much larger than p50 is a spike problem. A high p50 and p99 is a steady-cost problem.
    4. **Classify the hot spot** into 1 of 5 classes by its signature, then open that reference file:
       - script CPU: the `TIME_PROCESS` or `TIME_PHYSICS_PROCESS` 1-second peak rises with the slow interval, and the Profiler ranks script functions by `Self` and `Calls` -> `references/script-cpu.md`;
       - render thread: `RenderingServer.get_frame_setup_time_cpu()` plus summed viewport render CPU is high, with high draw calls, objects or primitives -> `references/render-gpu.md`;
       - GPU: summed viewport GPU time is near the budget, and halving `scaling_3d_scale` gives a large drop -> `references/render-gpu.md`;
       - physics: `TIME_PHYSICS_PROCESS`, `PHYSICS_*_COLLISION_PAIRS`, `PHYSICS_*_ACTIVE_OBJECTS` or `TIME_NAVIGATION_PROCESS` own the time -> `references/physics-servers-threads.md`;
       - spikes: a slow frame lines up with a pipeline `SURFACE` or `DRAW` counter delta, a load, `instantiate()` or `add_child()` event, or a managed GC event -> `references/native-memory-hitches.md`.
       When wall time is high but every built-in category is low, follow the escalation ladder in `references/measure-harness.md` (native sampling profiler, then Tracy, then a GPU frame capture).
    5. **Apply the matching technique** and prove it with `hfr_bench.py compare`. Accept a change only when the verdict is `PASS` and the game state is the same.
  - `references/` holds 1 file per research angle: `script-cpu.md` (report 1), `render-gpu.md` (report 2), `physics-servers-threads.md` (report 3), `measure-harness.md` (report 4) and `native-memory-hitches.md` (report 5), plus `detection.md` (`GRS-21`).
  - Each technique in a reference file keeps the 6 fields of its report: **What**, **Why/thread**, **Measure**, **Version**, **Evidence** (the report's label: verified, not verified, or design recommendation) and **Trap**. A technique without its measurement, evidence label and trap is incomplete.
- **Benchmark harness `hfr_bench.py` (`GRS-20`)**:
  - `skills/godot-hfr-hunt/scripts/hfr_bench.py` is a Python 3 standard-library script, mode `0755`, with zero comments.
  - The game side is a GDScript recorder in `references/measure-harness.md` (Open Question 6). It reads the user arguments `--hfr-bench=<scenario> --warmup-seconds=5 --sample-seconds=30 --hfr-out=<absolute path>`, asserts the caps from workflow step 1, and records `frame_ms` as the callback-to-callback `Time.get_ticks_usec()` delta. It sums render CPU and GPU time over every drawn viewport (`viewport_set_measure_render_time`), and records `null`, not `0`, for an unavailable timer. It writes the take file atomically, quits with `0`, and quits non-zero on an invalid configuration or missing samples.
  - Commands:
    - `python3 skills/godot-hfr-hunt/scripts/hfr_bench.py summarize <take.json>` recomputes the summary from the raw frames and prints it as JSON.
    - `python3 skills/godot-hfr-hunt/scripts/hfr_bench.py compare --baseline <export> --candidate <export> --scenario <name> --out <dir> [--pairs 5] [--target-fps 240]` runs 1 untimed warm-up take, then 5 interleaved baseline/candidate pairs. Each take runs `<export> --windowed --resolution 1920x1080 --disable-vsync --max-fps 0 -- --hfr-bench=<name> --warmup-seconds=5 --sample-seconds=30 --hfr-out=<dir>/<name>-<baseline|candidate>-<nn>.json`. It writes `<dir>/compare.json`, prints the pair table, and prints 1 verdict line.
  - Take file (1 JSON document per take, from report 4): `schema_version: 1`; `provenance` (`git_sha`, `godot_version`, `build`, `scenario`, `renderer`, `rendering_driver`, `resolution`, `arguments`, `thermal_start`, `thermal_end`); `validity` (`foreground`, `vsync_period_detected`, `gpu_timer_available`, `warmup_seconds`, `sample_seconds`); `summary` (`frames`, `frame_ms_p50`, `frame_ms_p95`, `frame_ms_p99`, `worst_frame_ms`, `one_percent_low_fps`, `over_3_333_ms`, `over_4_167_ms`); and raw `frames` rows (`i`, `frame_ms`, `render_cpu_ms`, `render_gpu_ms`, `draw_calls`, `primitives`). All frame times are in milliseconds.
  - Percentiles use nearest rank: sort `frame_ms` ascending and take index `ceil(p * n) - 1`. The 1% low frame time is `frame_ms_p99`, and `one_percent_low_fps = 1000 / frame_ms_p99`. The script never averages or sorts FPS values.
  - `compare.json` holds 1 row per pair: baseline and candidate p50, p95 and p99, the p99 delta, `valid`, and the invalid reason.
  - A pair is invalid when either take: lost foreground status, ended with a thermal state above nominal, detected a V-Sync period, differs from its partner in renderer, driver, resolution or arguments, has fewer than 20 s of valid samples, or has no take file.
  - Regression rule (report 4): the verdict is `FRAME_TIME_REGRESSION` only when at least 3 pairs are valid, at least 4 of 5 valid p99 deltas are positive (or all deltas are positive when only 3 or 4 pairs are valid), and the median paired p99 delta is greater than `max(0.10 ms, baseline_p99 * 0.05)`.
  - Budget rule (Open Question 5): `--target-fps` takes an integer from 200 to 300, default 240. A value outside that range is a usage error. The budget is `1000 / target` ms. The verdict is `<target>_FPS_BUDGET_FAILURE` (for example `240_FPS_BUDGET_FAILURE`) when the median candidate take p99 is greater than the budget. A budget failure does not need a baseline regression. The script prints the 240 fps (4.167 ms) and 300 fps (3.333 ms) budget results as information lines. They do not change the verdict or the exit code.
  - Verdict lines are exact: `PASS`, `FRAME_TIME_REGRESSION`, `<target>_FPS_BUDGET_FAILURE`, `INFRASTRUCTURE_INVALID`. Exit `0` on `PASS`. Exit `1` on `FRAME_TIME_REGRESSION` or on the budget failure for `--target-fps`. Exit `2` on `INFRASTRUCTURE_INVALID` (fewer than 3 valid pairs), which is never a pass.
- **Detection pass (`GRS-21`)**:
  - `references/detection.md` collects the "Detection" sections of the 5 reports, grouped by the 5 classes of workflow step 4, and keeps every `rg` command, profiler signature and monitor threshold. It includes:
    - the cap search: `application/run/max_fps`, `application/run/low_processor_mode`, `application/run/frame_delay_msec`, `Engine.max_fps`, `low_processor_usage_mode`, `window_set_vsync_mode`, `--fixed-fps`;
    - the triage threshold: a subsystem that takes 5% of the budget (0.20 ms at 240 fps, 0.17 ms at 300 fps);
    - the evidence triggers: any repeated frame over 4.17 ms or 3.33 ms, any gameplay-time `Draw` or `Surface` pipeline compilation, any managed allocation or GC event that lines up with a slow frame, a boundary loop with thousands of native calls per frame, `PHYSICS_*_COLLISION_PAIRS` that grow while the entity count stays flat, and physics above 60 TPS without a measured need;
    - the shadow, geometry, occlusion, 2D, shader and pipeline signatures from report 2, the physics, navigation and worker greps from report 3, and the loading, native-call, C# GC and scene-churn table from report 5.
- **Corrections to existing skills (`GRS-22` to `GRS-42`)**:
  - Each row names the file, the line range at the base commit of this spec, the incorrect claim and the corrected claim. The test for a row checks the text, not the line numbers.
  - Corrections change only the named claim. They do not restructure the file.
- **Wiring (`GRS-43`)**:
  - Add `godot-hfr-hunt` to `skills:` in `teammates/godot-performance-engineer.md`, and move `godot-assets-pipeline` from `skills:` to `available_skills:`. The seat then expects 9 skills, and 11 in a C# project (`skills_when` adds `godot-csharp-godot` and `godot-csharp-signals`). Without the move, a C# project gives 12, above the `GDW-08` cap. `godot-assets-pipeline` stays expected on `godot-release-engineer` and `godot-technical-artist`.
- **Skill gate (`GRS-44`)**:
  - Register `godot-hfr-hunt` in `skills/copied.json` (`"copied_files": []`, `"verbatim": false`), in `REPO_ORIGINAL` in `crates/horch-core/tests/skills_catalog.rs`, and in `skills/README.md`.
  - Add `godot-hfr-hunt/scripts/hfr_bench.py` to `EXEMPT_FILES` in `crates/horch-core/tests/skills_catalog.rs`, and assert its mode is `0755`.
  - The skill passes the existing `godot_skills` gate step in `scripts/phase-gate.sh`: `scripts/godot/tests`, `xref_check.py`, `api_check.py --strict-own`, `gdscript_blocks_check.py --strict-own` (which checks the recorder block) and `csharp_blocks_check.py --strict-own`. `scripts/comments/comment_lint.py check` passes on `hfr_bench.py`.
  - `scripts/godot/tests/test_hfr_bench.py` tests `hfr_bench.py` without the Godot binary. A fake export script writes take files from fixed frame lists, as `test_godot_run.py` does for `godot-run.sh`.

---

## Requirement Table

`scripts/check-req-coverage.sh` machine-reads every table whose second header cell is `Requirement` and checks that each `GRS-NN` ID has at least one Rust test starting with `grs_<nn>_`.

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| GRS-01 | `complexity-sweep` discovers changed `.rs` files in both top-level and nested Cargo crates by finding the nearest ancestor `Cargo.toml` within the work directory, runs `cargo clippy` once per crate directory, and prefixes reported `file_path` values back to work-dir-relative paths before filtering by changed hunks. | - | grs_01_nested_cargo_crates_are_grouped_and_paths_prefixed, grs_01_finish_analyses_nested_gdext_rust_crate |
| GRS-02 | `complexity-sweep` runs `cargo clippy` with `-W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`, maps them to `cognitiveComplexity`, `functionLength`, and `parameterCount`, extracts the function identifier after `fn` from full-signature primary spans, and deduplicates findings by `(file_path, line, metric)`. | - | grs_02_clippy_parses_three_lints_and_extracts_fn_identifier, grs_02_clippy_dedupes_by_file_line_and_metric |
| GRS-03 | `complexity-sweep` routes changed `.gd` files to `scripts/godot/gdcomplexity.py` (`gdtoolkit.parser`), reporting per-function `cyclomaticComplexity` (counting nested lambdas in their enclosing function) and `parameterCount` with exact 1-based start and end lines in the analyser JSON shape consumed by `in_changed_functions()`, reading optional thresholds from the nearest `static-analysis` config, and failing with an install hint when neither `uv` nor `python3` with `gdtoolkit` is available. | - | grs_03_gdcomplexity_reports_exact_lines_and_metrics_for_changed_functions, grs_03_missing_gdscript_analyser_fails_with_install_hint; Python: test_gdcomplexity_counts_branches_and_lambdas, test_gdcomplexity_reads_config_thresholds |
| GRS-04 | When no changed file matches a routed language, `complexity-sweep` reports `"no changed file is in a language that the analyser (TypeScript, JavaScript, Python), clippy (Rust) or gdcomplexity (GDScript) reads"`, and ADR 0086 records the nested Rust, 3-lint clippy, and GDScript `gdcomplexity` rules. | - | grs_04_not_applicable_message_names_all_routed_languages_and_tools, grs_04_adr_0086_documents_rust_and_gdscript_routes |
| GRS-05 | `horch doctor` checks finish-step static-analysis tools from project facts even when no seat is offered: `cargo`, `rustc`, and `cargo clippy` when `Cargo.toml` is present, and `uv` or `python3` with `gdtoolkit` when `project.godot` is present. | - | grs_05_doctor_checks_finish_step_tools_from_project_facts |
| GRS-06 | The roster includes `rust-developer` (Opus implementation builder), `rust-reviewer` (Opus validation reviewer denying `Edit`, `Write`, `NotebookEdit`), and `rust-qa-engineer` (Sonnet validation QA), each setting `offer_when: ["Cargo.toml"]`, `requires: [rust]`, and `fallbacks: [codex-sol]`. | - | grs_06_rust_team_seats_are_offered_on_cargo_toml_and_reviewer_is_read_only |
| GRS-07 | The bundled skill catalog includes seven own-text Rust skills (`rust-unsafe-review`, `rust-async`, `rust-cargo-workspace`, `rust-perf`, `rust-fuzz`, `rust-testing`, `rust-error-handling`) within the size budget, with `"copied_files": []` in `skills/copied.json`, and every `rust-*` skill is expected or available on at least one `rust-*` seat. | - | grs_07_seven_rust_skills_are_bundled_own_text_and_assigned_to_seats |
| GRS-08 | `Requirement::Rust` exists and is spelled `rust`; `horch doctor` checks `cargo`, `rustc`, and `cargo clippy` when an offered seat requires `rust` and reports actionable install fixes when any of the three is missing or broken. | - | grs_08_rust_requirement_is_spelled_rust_and_checked_by_doctor |
| GRS-09 | `skills/godot-build-verify/scripts/godot-check.sh` is an executable wrapper that runs `godot-run.sh` (headless import, project parse check, and optional `dotnet build` / GUT suite), strips ANSI escapes from the log, and exits non-zero (`3`) when any log line contains `SCRIPT ERROR`, `Parse Error`, `Failed to load script`, or `Nothing was run`, even if Godot exited `0`. | - | grs_09_godot_check_fails_when_godot_exits_zero_with_script_or_gut_errors; Python: test_godot_check_catches_script_error_on_zero_exit, test_godot_check_strips_ansi_gut_errors |
| GRS-10 | `godot-native-engineer` covers both C++ (`godot-cpp`) and Rust (`gdext`), sets `skills_when: {"Cargo.toml": [rust-cargo-workspace, rust-unsafe-review, rust-testing]}` alongside `"*.csproj"`, and `skills/godot-gdextension/references/rust-gdext.md` documents `.gdextension` platform library paths, `reloadable = true`, per-target Cargo builds, and headless test verification. | - | grs_10_godot_native_engineer_adds_rust_skills_on_cargo_toml_and_gdext_ref_is_complete |
| GRS-11 | `godot-code-reviewer` and `godot-qa-engineer` set `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` while staying within the 5-to-11 expected-skills budget in both GDScript and C# projects, and `docs/specs/godot.md` (`GDW-03`) reflects that only `godot-tech-lead` omits `skills_when` for `*.csproj`. | - | grs_11_csharp_project_adds_csharp_skills_to_godot_reviewer_and_qa_seats |
| GRS-12 | `code-sweep` triage triggers `g` (contracts) on GDScript/C#/gdext contract keywords (`class_name `, `signal `, `@export`, `public class `, `public struct `, `public interface `, `#[godot_api]`), `d` (dependencies) on `.csproj`, `Directory.Packages.props`, `project.godot`, `.gdextension`, and `plugin.cfg`, and `u` (UI) on `.tscn` and `.tres`; round-1 reviewer selection assigns the first review slot to the matching domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`) when available while keeping cross-vendor coverage on the remaining slots. | - | grs_12_codesweep_triages_godot_and_csharp_signals_and_selects_domain_reviewer |
| GRS-13 | `comment-sweep` excludes `.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, and `project.godot` in `comments::NOT_CODE`, `skills/comment-pruning/references/pruning-pass.md` explicitly preserves Rust `// SAFETY:` comments and Rust/GDScript doc comments (`///`, `//!`, `##`), and `scripts/comments/comment_lint.py` scans `.cs` files. | - | grs_13_comment_sweep_skips_godot_data_files_and_keeps_safety_and_doc_comments |
| GRS-14 | `suggested_check(facts)` returns the canonical stack check command for Rust (`Cargo.toml`), Godot (`project.godot`), hybrid Godot+Rust, and Swift (`Package.swift`) projects; `horch ledger run new` without `--check` and missing-check error messages print the suggested command, and `skills/orchestrate/SKILL.md` documents the stack check table. | - | grs_14_suggested_check_proposes_canonical_command_per_stack |
| GRS-15 | Worktree cold vs warm build/import times are measured in `ai_docs/reports/godot-rust-worktree-timing.md`, and worktree creation seeds `.godot/imported/` (and class/UID caches) from the base checkout when present so Godot workers avoid cold re-imports without sharing `CARGO_TARGET_DIR`. | - | grs_15_worktree_new_seeds_godot_imported_cache_when_present |
| GRS-16 | In a Godot project whose `.gitattributes` contains `filter=lfs`, `horch doctor` checks that `git-lfs` is installed and working even when no seat explicitly lists `requires: [git-lfs]`. | - | grs_16_doctor_checks_git_lfs_on_godot_project_with_lfs_rules |
| GRS-17 | Codex worker launches ensure Cargo commands can run under `workspace-write` when `~/.cargo` is read-only by providing a writable `CARGO_HOME` fallback (or verified sandbox rules), documented with a live check. | - | grs_17_codex_launch_provides_writable_cargo_home_or_rule |
| GRS-18 | `docs/live-checks/godot-codex.md` records a direct `godot-run.sh` verification inside a Codex sandbox environment and `docs/specs/godot.md` marks the pending Codex `godot-run.sh` live check complete. | - | grs_18_godot_codex_live_check_is_recorded_and_spec_status_closed |
| GRS-19 | `skills/godot-hfr-hunt/` is an own-text bundled skill. `SKILL.md` (<= 12 KB) gives the 5-step workflow in order: uncap the frame rate, run `scripts/hfr_bench.py`, read p50/p95/p99/worst/1% low against the 5.00, 4.17 and 3.33 ms budgets, classify the hot spot as script CPU, render thread, GPU, physics or spikes by its signature, and apply the matching technique. `references/` holds 1 file per angle (`script-cpu.md`, `render-gpu.md`, `physics-servers-threads.md`, `measure-harness.md`, `native-memory-hitches.md`) taken from the 5 HFR reports, and each technique keeps its What, Why/thread, Measure, Version, Evidence label and Trap. | - | grs_19_hfr_hunt_skill_md_orders_the_five_workflow_steps, grs_19_hfr_hunt_references_have_one_file_per_angle_with_measure_evidence_and_trap |
| GRS-20 | `skills/godot-hfr-hunt/scripts/hfr_bench.py` (Python 3 standard library, mode `0755`) has `summarize <take.json>` and `compare --baseline --candidate --scenario --out [--pairs 5] [--target-fps 240]` (`--target-fps` an integer from 200 to 300, default 240). Each take runs `<export> --windowed --resolution 1920x1080 --disable-vsync --max-fps 0 -- --hfr-bench=<name> --warmup-seconds=5 --sample-seconds=30 --hfr-out=<path>`. The take JSON (`schema_version: 1`) holds `frame_ms_p50`, `frame_ms_p95`, `frame_ms_p99`, `worst_frame_ms`, `one_percent_low_fps` (`1000 / frame_ms_p99`), `over_3_333_ms`, `over_4_167_ms` and raw frames, with nearest-rank percentiles (`ceil(p * n) - 1`) in ms. `compare` prints exactly 1 of `PASS`, `FRAME_TIME_REGRESSION`, `<target>_FPS_BUDGET_FAILURE`, `INFRASTRUCTURE_INVALID`, where the budget is `1000 / target` ms against the median candidate p99. It reports a regression only with >= 3 valid pairs, >= 4 of 5 positive p99 deltas (all positive when 3 or 4 are valid) and a median p99 delta > `max(0.10 ms, baseline_p99 * 0.05)`. It exits `0` on `PASS`, `1` on a regression or the `--target-fps` budget failure (only the target budget exits `1`), and `2` on `INFRASTRUCTURE_INVALID`. | - | grs_20_hfr_bench_python_suite_passes; Python: test_hfr_bench_summarize_uses_nearest_rank_percentiles, test_hfr_bench_compare_flags_regression_on_four_of_five_pairs, test_hfr_bench_compare_ignores_delta_below_noise_floor, test_hfr_bench_compare_reports_budget_failure_without_regression, test_hfr_bench_compare_is_infrastructure_invalid_below_three_pairs, test_hfr_bench_runs_fake_export_without_godot |
| GRS-21 | `skills/godot-hfr-hunt/references/detection.md` groups the "Detection" sections of the 5 HFR reports by the 5 hot-spot classes and keeps every `rg` command, profiler signature and monitor threshold, including the frame-cap search, the 5%-of-budget triage threshold (0.20 ms at 240 fps, 0.17 ms at 300 fps), the 4.17 ms and 3.33 ms frame triggers, gameplay-time `Draw`/`Surface` pipeline compilations, `PHYSICS_*_COLLISION_PAIRS` growth at a flat entity count, and GC events that line up with slow frames. | - | grs_21_hfr_detection_reference_keeps_greps_signatures_and_thresholds |
| GRS-22 | `skills/godot-perf-measure/SKILL.md` (lines 54-56) no longer says saved settings can force V-Sync back on after `--disable-vsync`. It says the flag forces V-Sync off even when the project enables it, only driver-level enforcement can defeat it, and the scratch settings file stays for reproducibility. | - | grs_22_perf_measure_says_disable_vsync_overrides_project_setting |
| GRS-23 | `skills/godot-perf-measure/SKILL.md` (lines 48-50 and 122-123) says `Performance.TIME_PROCESS` and `TIME_PHYSICS_PROCESS` are 1-second peak values, not per-frame samples, and that per-frame rows use callback-to-callback `Time.get_ticks_usec()` wall time. | - | grs_23_perf_measure_labels_time_process_as_one_second_peak |
| GRS-24 | `skills/godot-perf-measure/SKILL.md` (lines 57-60) no longer names Metal as a driver that returns 0 GPU timers or prescribes MoltenVK for it. It says to detect and report 0 or unavailable GPU timers at run time on any driver. | - | grs_24_perf_measure_does_not_name_metal_as_zero_gpu_timer_driver |
| GRS-25 | `skills/godot-perf-measure/references/shader-warm-up.md` (lines 6-24) says Forward+ and Mobile have automatic ubershader and pipeline precompilation since Godot 4.4, the pipeline compilation monitors find the missing path, and only Compatibility needs a first-draw warm-up of every variant. | - | grs_25_shader_warm_up_scopes_first_draw_warm_up_to_compatibility |
| GRS-26 | `skills/godot-optimization/references/measuring-performance.md` (lines 105-110) uses nearest-rank percentiles (`ceil(p * n) - 1`), names that convention, and reports 1% low FPS as `1000 / p99_frame_ms` without averaging FPS. | - | grs_26_measuring_performance_uses_named_nearest_rank_and_one_percent_low |
| GRS-27 | `skills/godot-optimization/references/cpu-bottlenecks.md` (lines 13 and 82-97) and `skills/godot-optimization/SKILL.md` (line 145) no longer say a temporary `Vector2`/`Vector3` constructor allocates or causes a garbage-collector pause. They say small vectors are stored inline in `Variant`, GDScript has no .NET garbage collector, and `Array`, `Dictionary`, `String` and managed C# objects allocate in different ways. | - | grs_27_cpu_bottlenecks_says_vectors_are_inline_values_not_gc_allocations |
| GRS-28 | `skills/godot-optimization/references/cpu-bottlenecks.md` (lines 176-185) no longer says typed arrays skip per-element type checks. It says a typed `Array` write calls the type validator, typing gives safety and lets the compiler specialize nearby expressions, and packed arrays give contiguous value storage. | - | grs_28_cpu_bottlenecks_says_typed_array_writes_are_validated |
| GRS-29 | `skills/godot-optimization/references/cpu-bottlenecks.md` (line 119) no longer says `String` comparison allocates temporaries. It says the comparison scans without a new string, and `StringName == StringName` is cheaper because it compares interned pointers. | - | grs_29_cpu_bottlenecks_says_string_comparison_does_not_allocate |
| GRS-30 | `skills/godot-optimization/SKILL.md` (line 133) no longer tells the reader to move heavy `_process()` work to `_physics_process()` because it runs less often. It says to keep simulation in physics and give delay-tolerant work an explicit cadence or budget, scheduled on accumulated `delta` or a timer, not on a frame count. | - | grs_30_optimization_uses_explicit_update_tiers_not_physics_throttling |
| GRS-31 | `skills/godot-optimization/SKILL.md` (lines 98, 132 and 164) and `skills/godot-optimization/references/draw-calls.md` (lines 11-20) no longer prescribe `CanvasGroup` as the 2D batcher. They say Godot 4.7 batches compatible 2D work automatically in every renderer, and `CanvasGroup` is a backbuffer-compositing node that can add cost. | - | grs_31_optimization_does_not_prescribe_canvas_group_for_batching |
| GRS-32 | `skills/godot-optimization/references/draw-calls.md` (lines 33-40 and 54-61) no longer labels a `duplicate()`d `ShaderMaterial` as shared. It uses `instance uniform` with `set_instance_shader_parameter()` and duplicates only when an instance uniform cannot hold the difference. | - | grs_32_draw_calls_uses_instance_uniforms_not_duplicated_materials |
| GRS-33 | `skills/godot-optimization/references/draw-calls.md` (line 65) no longer says `GeometryInstance3D.gi_mode = BAKE_STATIC` lets the engine merge static geometry. It says that property opts geometry into GI, and static merging is done in the asset pipeline or with an HLOD mesh. | - | grs_33_draw_calls_says_gi_mode_does_not_merge_meshes |
| GRS-34 | `skills/godot-optimization/references/draw-calls.md` (lines 168-198) no longer uses `VisibleOnScreenNotifier3D` and a per-frame script for distance LOD. It uses imported mesh LOD and `GeometryInstance3D` visibility ranges with HLOD dependencies and hysteresis. | - | grs_34_draw_calls_uses_mesh_lod_and_visibility_ranges |
| GRS-35 | `skills/godot-optimization/references/physics-tuning.md` (lines 89-92) says `max_physics_steps_per_frame` trades catch-up for slow motion: at 60 TPS and the default cap of 8, game time slows below 7.5 rendered fps, and a higher TPS without a higher cap raises that threshold. | - | grs_35_physics_tuning_explains_max_physics_steps_slow_motion |
| GRS-36 | `skills/godot-optimization/SKILL.md` (line 106) and `skills/godot-optimization/references/physics-tuning.md` (line 108) no longer always prefer `Area2D/3D` to raycasts. They say an Area fits continuous enter/exit tracking, a bounded direct query is often cheaper for sparse or on-demand checks, and both are measured at the real entity count. | - | grs_36_physics_tuning_compares_areas_and_direct_queries |
| GRS-37 | `skills/godot-optimization/references/physics-tuning.md` (lines 63-77) says a convex decomposition into many hulls can be slower than 1 static concave shape, primitives suit dynamic bodies, and large static geometry uses a simplified concave mesh or `HeightMapShape3D`. | - | grs_37_physics_tuning_states_concave_versus_convex_crossover |
| GRS-38 | `skills/godot-optimization/references/physics-tuning.md` (line 81) says that at 240 rendered fps and 60 TPS most frames have no physics tick, that 240 TPS makes physics and `_physics_process` work about 4 times as frequent, and that physics interpolation exists to avoid that cost. | - | grs_38_physics_tuning_explains_tick_rate_at_high_frame_rate |
| GRS-39 | `skills/godot-optimization/references/memory-management.md` (line 136) no longer says each instantiate/free cycle re-runs `_ready()` for a pooled node. It says `_ready()` runs on the first tree entry, re-adding a node needs `Node.request_ready()` for another call, and a pool can cost more than it saves, so the full acquire/reset/release path is measured. | - | grs_39_memory_management_corrects_ready_on_pooled_nodes |
| GRS-40 | `skills/godot-multithreading/SKILL.md` (line 18) says separate physics and rendering threads are not automatically faster: physics API access is restricted to physics processing, Jolt separate-thread support is experimental in 4.7, and the rendering `Separate` thread model has known bugs. | - | grs_40_multithreading_says_separate_server_threads_are_not_automatically_faster |
| GRS-41 | `skills/godot-multithreading/SKILL.md` (line 22) separates CPU preparation from GPU synchronization: workers can fill plain `Image` and packed-array data, while texture creation, GPU-backed image reads and uploads from workers can synchronize with `RenderingServer` and stall. | - | grs_41_multithreading_separates_cpu_preparation_from_gpu_sync |
| GRS-42 | `skills/godot-multithreading/SKILL.md` (lines 192-210) says `instantiate()`, `_enter_tree`, `_ready`, server registration and pipeline surface preparation still run on the main thread after `load_threaded_get()`, and the example stages or time-slices scene integration across frames. | - | grs_42_multithreading_stages_scene_integration_after_threaded_load |
| GRS-43 | `teammates/godot-performance-engineer.md` lists `godot-hfr-hunt` in `skills:` and moves `godot-assets-pipeline` to `available_skills:`, so the seat expects 9 skills, or 11 in a `*.csproj` project, within the `GDW-08` cap. | - | grs_43_performance_engineer_expects_hfr_hunt_within_eleven_skill_cap |
| GRS-44 | `godot-hfr-hunt` is registered in `skills/copied.json` (`"copied_files": []`), `REPO_ORIGINAL` and `skills/README.md`, `godot-hfr-hunt/scripts/hfr_bench.py` is in `EXEMPT_FILES` with mode `0755`, the skill passes the `godot_skills` gate step (`scripts/godot/tests`, `xref_check.py`, `api_check.py`, `gdscript_blocks_check.py`, `csharp_blocks_check.py`) and `comment_lint.py`, and `scripts/godot/tests/test_hfr_bench.py` runs without the Godot binary. | - | grs_44_hfr_hunt_is_registered_exempt_and_executable; Python: test_hfr_bench_runs_fake_export_without_godot |

---

## Testing Decisions

- **What makes a good test**:
  - Test observable behavior at public module, CLI, and script boundaries: JSON violation output, `Outcome` verdicts and notes, `Roster::load_layered` and `with_project_skills` results, `doctor::requirement_problems` warning strings, `godot-check.sh` exit statuses on simulated engine outputs, and `codesweep::triage` / `comments::is_code` return values.
  - Never write tautological tests that copy production formulas into assertions (per `docs/specs/test-suite-audit.md`).
  - Zero code comments in any new or edited `.rs`, `.py`, or `.sh` file (`scripts/comments/comment_lint.py check`).
  - Warm every new fake executable script once with a `"warm"` argument before timing or running it in parallel tests (per ADR 0111 and `crates/horch-core/tests/common/warm.rs`).
- **Modules to be tested**:
  - `crates/horch-core/src/taskledger/finish/complexity.rs` and `scripts/godot/gdcomplexity.py`
  - `crates/horch-core/src/taskledger/finish/codesweep.rs`
  - `crates/horch-core/src/taskledger/finish/comments.rs`
  - `crates/horch-core/src/roster/offer.rs` and `teammates/rust-*.md`, `teammates/godot-*.md`
  - `crates/horch/src/cmd/doctor.rs`
  - `crates/horch/src/cmd/taskledger.rs`
  - `skills/godot-build-verify/scripts/godot-check.sh`
  - `skills/godot-hfr-hunt/scripts/hfr_bench.py`, `skills/godot-hfr-hunt/` content, the corrected files in `skills/godot-perf-measure`, `skills/godot-optimization` and `skills/godot-multithreading`, and `teammates/godot-performance-engineer.md`
- **Prior art in the codebase**:
  - `crates/horch-core/tests/godot_wave.rs` (`gdw_01_*` to `gdw_13_*`) for roster, `skills_when`, `Requirement`, skill size budgets, and `godot-run.sh` fake-engine testing.
  - `crates/horch-core/src/taskledger/finish/complexity.rs` (`#[cfg(test)] mod tests`) and `crates/horch-e2e/tests/taskledger.rs` (`trf_07_*`) for clippy JSON parsing and finish-step complexity e2e testing.
  - `crates/horch/src/cmd/doctor.rs` (`#[cfg(all(test, unix))] mod tests`) for hermetic `HostTools` and `PATH` doctor tests.

---

## Out of Scope / Future Work

- **Godot C# complexity route in `complexity-sweep` (Report A item 4)**:
  - Deferred by operator decision (`ai_docs/plans/godot-rust-support/decisions.md`, 2026-11-24). Routing `.cs` files through `StaticAnalysis.Cli` (in parse-only mode with `#[serde(alias = "function")]` and UTF-8 BOM stripping) or Roslyn `CA1502` requires installing the .NET 8 runtime on fleet hosts and packaging `StaticAnalysis.Cli.dll` with the repository. When C# static analysis is prioritized in a future run, add the `.cs` selector in `complexity.rs`, `Tools.csharp_cli` lookup via `HORCH_CSHARP_CLI`, and the `dotnet` + `StaticAnalysis.Cli.dll` doctor check.
- **Rust MCP servers**:
  - No third-party Rust MCP server is added to `rust-developer`, `rust-reviewer`, or `rust-qa-engineer`; `cargo check`, `cargo clippy`, `cargo test`, and `cargo doc` over the shell provide deterministic compiler ground truth without external npm dependencies.
- **Sharing `CARGO_TARGET_DIR` across worktrees**:
  - Explicitly excluded because Cargo's `build.rs` path caching across checkouts can embed another worktree's `teammates/` and `skills/` into `horch-core` without warning (`docs/testing-and-gates.md:209-219`).

---

## Open Questions

1. **Test seams and requirement ID prefix**:
   - **Question**: Do these seams (`crates/horch-core/tests/godot_rust_support.rs` with `grs_01_`..`grs_18_`, plus `doctor.rs`, `taskledger.rs`, and `scripts/godot/tests/`) match your expectations?
   - **Options**:
     - **Option 1 (Recommended)**: Use prefix `GRS-01`..`GRS-18` (`grs_01_`..`grs_18_`) centered in `crates/horch-core/tests/godot_rust_support.rs` (mirroring `crates/horch-core/tests/godot_wave.rs` for `GDW-01`..`GDW-13`), with doctor unit tests in `crates/horch/src/cmd/doctor.rs` and finish e2e tests in `crates/horch-e2e/tests/taskledger.rs`.
     - **Option 2**: Split tests across one test file per requirement (`crates/horch-core/tests/grs_<nn>_<slug>.rs`).
   - **Recommendation**: **Option 1**. Consolidating the integration tests into `crates/horch-core/tests/godot_rust_support.rs` avoids adding 18 separate test binaries to Cargo's sequential test runner (which `docs/specs/test-suite-audit.md` showed adds link and first-exec overhead per binary on macOS).
   - Decision: Option 1 (orchestrator).

2. **How `code-sweep` integrates the domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`)**:
   - **Question**: When a task's diff touches Godot (`.gd`, `.tscn`), Rust (`.rs`), or Swift (`.swift`) files in round 1 of `code-sweep`, how should the domain reviewer seat be scheduled alongside the cross-vendor generic reviewers (`codex-reviewer` / `architect-reviewer`)?
   - **Options**:
     - **Option 1 (Recommended)**: Assign the first selected review slot (the `"b"` bug-hunter / correctness lens) to the matching domain reviewer (`godot-code-reviewer`, `rust-reviewer`, or `swift-reviewer`) when that persona is available, and keep the remaining 1 to 2 slots on the cross-vendor generic reviewer (`codex-reviewer` or `architect-reviewer`). In rounds 2 and 3, continue using the cheaper generic reviewer (`codex-reviewer-light` / `architect-reviewer-light`) as today.
     - **Option 2**: Replace all round-1 reviewers with the domain reviewer.
   - **Recommendation**: **Option 1**. It respects the `max_reviewers` cap (2 for `light`, 3 for `full`), brings domain-specific review skills (`godot-code-review`, `rust-unsafe-review`, `swift-code-audit`) into round 1, and preserves the cross-vendor review rule from `skills/orchestrate/SKILL.md` §3 item 8 so a Claude-authored change is still reviewed by a Codex reviewer on the remaining lenses.
   - Decision: Option 1 (orchestrator).

3. **Where `gdcomplexity.py` lives and how `horch` locates it at runtime**:
   - **Question**: Should `gdcomplexity.py` live only in `scripts/godot/gdcomplexity.py`, or should `horch` also embed/materialize it so an installed `horch` binary can run GDScript `complexity-sweep` outside the `multi-herdr` repository?
   - **Options**:
     - **Option 1 (Recommended)**: Keep the source script at `scripts/godot/gdcomplexity.py` (tested by `scripts/godot/tests/`) AND embed it into `horch-core` via `include_str!` (writing it to `<evidence_root>/bin/gdcomplexity.py` when `HORCH_GDCOMPLEXITY_CLI` is unset), the same way `horch` materializes bundled skill files.
     - **Option 2**: Look up `scripts/godot/gdcomplexity.py` only inside the current repo or via `HORCH_GDCOMPLEXITY_CLI`.
   - **Recommendation**: **Option 1**. Target Godot projects run the installed `horch` binary in their own repository where `multi-herdr/scripts/godot/` does not exist on disk. Embedding the single short Python script and materializing it under `<evidence_root>` on demand makes GDScript `complexity-sweep` work in every target Godot repository with zero manual path setup while still letting `HORCH_GDCOMPLEXITY_CLI` override it in tests.
   - Decision: Option 1 (orchestrator).

4. **Adjusting `godot-qa-engineer` base skills for `GDW-08` (5 to 11 expected skills) when adding C# `skills_when` (G14)**:
   - **Question**: `godot-qa-engineer` currently has 11 expected `skills:` (the maximum allowed by `GDW-08`). Adding `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` would give it 13 expected skills in a C# project unless 2 base skills move to `available_skills:` or the `GDW-08` cap is raised. Which option should we take?
   - **Options**:
     - **Option 1 (Recommended)**: Move `godot-bot-balance-loop` and `godot-visual-verification` from `skills:` to `available_skills:` on `godot-qa-engineer` (leaving 9 base expected skills, and 11 in a C# project).
     - **Option 2**: Keep all 11 base skills on `godot-qa-engineer` and raise the `GDW-08` cap from 11 to 13 when `skills_when` matches.
   - **Recommendation**: **Option 1**. `godot-bot-balance-loop` and `godot-visual-verification` are specialized workflows compared to `godot-testing`, `godot-debugging`, `godot-build-verify`, `godot-test-loop`, and `godot-headless-runs`, and keeping the 11-skill cap avoids bloating the launch prompt of every `godot-qa-engineer` session.
   - Decision: Option 1 (orchestrator).

5. **Which frame budget makes `hfr_bench.py compare` fail**:
   - **Question**: Report 4 names both a 300 fps budget failure (median candidate p99 > 3.333 ms) and a 240 fps budget failure (> 4.167 ms). A game that targets 240 fps always fails the 300 fps budget. Which budget makes `compare` exit non-zero?
   - **Options**:
     - **Option 1 (Recommended)**: Add `--target-fps` (`240` or `300`, default `300`). Only the budget failure for the target exits `1`. The script prints a failed budget above the target as information.
     - **Option 2**: Exit `1` on either budget failure, as report 4 states.
     - **Option 3**: Add a 200 fps budget (5.00 ms) as a third target value.
   - **Recommendation**: **Option 1**. The operator target is a range (200 to 300 fps). Option 2 fails every 240 fps game. Option 3 adds a target that no report defines a verdict for. It can follow when a project needs it.
   - Decision: Option 1, with --target-fps taking an integer from 200 to 300, default 240. The budget is 1000/target ms against the median candidate p99. Only the target budget exits 1 (orchestrator).

6. **Where the in-game recorder lives**:
   - **Question**: The harness needs a GDScript recorder inside the game that writes the take file. Is it a `.gd` file under `skills/godot-hfr-hunt/scripts/`, or a code block in `references/measure-harness.md` that the teammate copies into the project?
   - **Options**:
     - **Option 1 (Recommended)**: A ```` ```gdscript ```` block in `references/measure-harness.md`.
     - **Option 2**: A file `skills/godot-hfr-hunt/scripts/hfr_recorder.gd`, added to `EXEMPT_FILES`.
   - **Recommendation**: **Option 1**. `gdscript_blocks_check.py --strict-own` already checks GDScript blocks in skill Markdown. A `.gd` file needs a second `EXEMPT_FILES` entry, and no gate step parses a bundled `.gd` file.
   - Decision: Option 1 (orchestrator).

---

## Further Notes

- **ADR updates required by the brief**:
  - Update [`docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md`](../adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md) for the nested Cargo crate grouping, the 3 clippy lints (`cognitive_complexity`, `too_many_lines`, `too_many_arguments`), and the `gdcomplexity.py` (`gdtoolkit.parser`) GDScript complexity route.
  - Add a new ADR (`docs/adr/0195-code-and-comment-sweeps-adapt-to-project-languages.md`, or next available number) recording why `code-sweep` pairs a domain reviewer with the cross-vendor reviewer and why `comment-sweep` excludes Godot structured data files (`.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, `project.godot`).
- **Requirement coverage gate (`scripts/check-req-coverage.sh`)**:
  - `scripts/check-req-coverage.sh` checks every `| <ID> |` row in any table whose second header cell is `Requirement` or `Check`. The `## Requirement Table` section uses the `Requirement` header so that the gate step (`scripts/check-req-coverage.sh`) verifies `grs_01_*` through `grs_44_*` as each unit lands. Every implementation ticket that introduces or owns a `GRS-NN` row must commit its `grs_<nn>_*` test in the same commit (`docs/testing-and-gates.md:95-96`).
