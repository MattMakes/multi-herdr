# Spec: Godot and Rust fleet support (GRS)

| | |
|---|---|
| Status | Proposed (not started). Planned requirements GRS-01 to GRS-18; Requirement Table (section 5) adopts the active `scripts/check-req-coverage.sh` column header (`Requirement`) alongside the tests that implement each requirement. |
| Date | 2026-11-24 |
| Author | spec-writer-1 |
| Inputs | Brief (`ai_docs/plans/godot-rust-support/brief.md`), operator decisions (`ai_docs/plans/godot-rust-support/decisions.md`), static-analysis report (`ai_docs/reports/godot-rust-static-analysis.md`, section 9 items 1, 2, 3, 5, 6), capability-audit report (`ai_docs/reports/godot-rust-capability-audit.md`, gaps G1 to G14). |
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

---

## Solution

Extend the fleet's static analysis, roster, skills, finish steps, doctor checks, and live checks so that Rust, Godot (GDScript and C#), and hybrid Godot + Rust (`gdext`) projects are first-class citizens:

1. **Static Analysis (`complexity-sweep`)**:
   - Discover nested Cargo crates by walking up from each changed `.rs` file to the nearest ancestor `Cargo.toml` within the work directory, grouping changed `.rs` files by crate root, running `cargo clippy` per crate root, and prefixing reported paths back to work-dir-relative paths.
   - Expand the clippy complexity pass to enforce `-W clippy::cognitive_complexity -W clippy::too_many_lines -W clippy::too_many_arguments --cap-lints warn`, mapping them to `cognitiveComplexity`, `functionLength`, and `parameterCount`, extracting the function identifier after `fn` from signature spans, and deduplicating by `(file_path, line, metric)`.
   - Add a GDScript complexity route for changed `.gd` files using a bundled Python script (`scripts/godot/gdcomplexity.py` / embedded asset) powered by `gdtoolkit.parser` (`gdtoolkit==4.5.0`) that computes per-function `cyclomaticComplexity` (branch nodes + 1, including lambdas inside their enclosing function) and `parameterCount` with exact AST start and end lines, reading optional threshold overrides from the nearest `static-analysis` config and emitting the standard analyser JSON schema consumed by `in_changed_functions()`.
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
   - Update `code-sweep` (`crates/horch-core/src/taskledger/finish/codesweep.rs`) to include the domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`) when the task's changed files or project facts match that stack and the author runs on the opposite vendor (or as the first domain specialist slot alongside the cross-vendor reviewer), and expand triage signals: `contracts` (`class_name `, `signal `, `@export`, `public class`, `public interface`, `public struct`, `#[godot_api]`), `touches_dependencies` (`.csproj`, `Directory.Packages.props`, `project.godot`, `.gdextension`, `plugin.cfg`), and `touches_ui` (`tscn`, `tres`).
   - Update `comment-sweep` (`crates/horch-core/src/taskledger/finish/comments.rs`) to exclude Godot non-code data files (`.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, `project.godot`) in `NOT_CODE`, and update `skills/comment-pruning/references/pruning-pass.md` to keep Rust `// SAFETY:` comments on `unsafe` blocks/impls and GDScript `##` / Rust `///` and `//!` public doc comments.
   - Add stack-based default check suggestion/inference (`crates/horch/src/cmd/taskledger.rs` and `skills/orchestrate/SKILL.md`): when `horch ledger run new <slug>` is called without `--check`, print or apply the recommended check command from project facts (Rust: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`; Godot: `bash skills/godot-build-verify/scripts/godot-check.sh` or project equivalent; hybrid Godot+Rust: both), while preserving `--check` override and `**Check:** none`.
5. **Low-Severity Hardening and Live Checks (G9, G11, G12, G13)**:
   - Measure cold versus warm worktree Cargo build and Godot import times on a sample Godot + Rust project (`ai_docs/reports/godot-rust-worktree-timing.md`), and only if measured savings justify it, seed `.godot/imported/` on `horch ledger worktree new` and document `CARGO_TARGET_DIR` / `sccache` guidance (G9).
   - Check Git LFS in `horch doctor` for Godot projects whenever `.gitattributes` contains `filter=lfs` (G11).
   - Ensure Codex launches set a writable `CARGO_HOME` under the workspace or state root when `~/.cargo` is not writable in `workspace-write` mode, verified by a live check (G12).
   - Run and record `godot-run.sh` inside a real Codex pane in `docs/live-checks/godot-codex.md` and update `docs/specs/godot.md` status (G13).

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

---

## Test Seams

Following `docs/specs/godot.md` and `docs/testing-and-gates.md`, every requirement ID (`GRS-01` to `GRS-18`) is pinned by at least one Rust test whose function name starts with `grs_<nn>_` so that `scripts/check-req-coverage.sh` enforces 100% requirement coverage in `just gate`. We use four existing seams and one new integration test file (`crates/horch-core/tests/godot_rust_support.rs`):

1. **Primary Seam — Roster, Skills, and Finish Unit/Integration Suite (`crates/horch-core/tests/godot_rust_support.rs`)**:
   - **Test prefix**: `grs_01_` through `grs_18_`.
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
2. **Existing Seam — Doctor Unit Tests (`crates/horch/src/cmd/doctor.rs`)**:
   - **Test prefix**: `grs_05_`, `grs_08_`, `grs_16_`.
   - **What it tests**: `Requirement::Rust` (`cargo`, `rustc`, `cargo clippy` checks) and project-fact finish-step tool warnings (`Cargo.toml` without `cargo`/`clippy`; `project.godot` without `uv`/`gdtoolkit`; `project.godot` + `.gitattributes` `filter=lfs` without `git-lfs`) using hermetic temp directories and warmed fake binaries.
3. **Existing Seam — Task-Ledger E2E Finish Suite (`crates/horch-e2e/tests/taskledger.rs`)**:
   - **Test prefix**: `grs_01_`, `grs_03_`.
   - **What it tests**: Full `horch ledger finish` runs on a worktree with a nested `rust/Cargo.toml` crate and a `.gd` script change.
4. **Existing Seam — Godot Python Gate (`scripts/godot/tests/`)**:
   - **Test prefix**: `test_gdcomplexity_*` and `test_godot_check_*` in `scripts/godot/tests/`, invoked both by `just gate` (`godot_skills` step) and from `grs_03_` / `grs_09_` in `crates/horch-core/tests/godot_rust_support.rs`.

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
  - Ship an own-text Python script `scripts/godot/gdcomplexity.py` (and resolve it via `HORCH_GDCOMPLEXITY_CLI` or `<repo>/scripts/godot/gdcomplexity.py` or a materialized copy under state/data root on `finish::Tools.gdcomplexity_cli`).
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

2. **How `code-sweep` integrates the domain reviewer (`godot-code-reviewer`, `rust-reviewer`, `swift-reviewer`)**:
   - **Question**: When a task's diff touches Godot (`.gd`, `.tscn`), Rust (`.rs`), or Swift (`.swift`) files in round 1 of `code-sweep`, how should the domain reviewer seat be scheduled alongside the cross-vendor generic reviewers (`codex-reviewer` / `architect-reviewer`)?
   - **Options**:
     - **Option 1 (Recommended)**: Assign the first selected review slot (the `"b"` bug-hunter / correctness lens) to the matching domain reviewer (`godot-code-reviewer`, `rust-reviewer`, or `swift-reviewer`) when that persona is available, and keep the remaining 1 to 2 slots on the cross-vendor generic reviewer (`codex-reviewer` or `architect-reviewer`). In rounds 2 and 3, continue using the cheaper generic reviewer (`codex-reviewer-light` / `architect-reviewer-light`) as today.
     - **Option 2**: Replace all round-1 reviewers with the domain reviewer.
   - **Recommendation**: **Option 1**. It respects the `max_reviewers` cap (2 for `light`, 3 for `full`), brings domain-specific review skills (`godot-code-review`, `rust-unsafe-review`, `swift-code-audit`) into round 1, and preserves the cross-vendor review rule from `skills/orchestrate/SKILL.md` §3 item 8 so a Claude-authored change is still reviewed by a Codex reviewer on the remaining lenses.

3. **Where `gdcomplexity.py` lives and how `horch` locates it at runtime**:
   - **Question**: Should `gdcomplexity.py` live only in `scripts/godot/gdcomplexity.py`, or should `horch` also embed/materialize it so an installed `horch` binary can run GDScript `complexity-sweep` outside the `multi-herdr` repository?
   - **Options**:
     - **Option 1 (Recommended)**: Keep the source script at `scripts/godot/gdcomplexity.py` (tested by `scripts/godot/tests/`) AND embed it into `horch-core` via `include_str!` (writing it to `<evidence_root>/bin/gdcomplexity.py` when `HORCH_GDCOMPLEXITY_CLI` is unset), the same way `horch` materializes bundled skill files.
     - **Option 2**: Look up `scripts/godot/gdcomplexity.py` only inside the current repo or via `HORCH_GDCOMPLEXITY_CLI`.
   - **Recommendation**: **Option 1**. Target Godot projects run the installed `horch` binary in their own repository where `multi-herdr/scripts/godot/` does not exist on disk. Embedding the single short Python script and materializing it under `<evidence_root>` on demand makes GDScript `complexity-sweep` work in every target Godot repository with zero manual path setup while still letting `HORCH_GDCOMPLEXITY_CLI` override it in tests.

4. **Adjusting `godot-qa-engineer` base skills for `GDW-08` (5 to 11 expected skills) when adding C# `skills_when` (G14)**:
   - **Question**: `godot-qa-engineer` currently has 11 expected `skills:` (the maximum allowed by `GDW-08`). Adding `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}` would give it 13 expected skills in a C# project unless 2 base skills move to `available_skills:` or the `GDW-08` cap is raised. Which option should we take?
   - **Options**:
     - **Option 1 (Recommended)**: Move `godot-bot-balance-loop` and `godot-visual-verification` from `skills:` to `available_skills:` on `godot-qa-engineer` (leaving 9 base expected skills, and 11 in a C# project).
     - **Option 2**: Keep all 11 base skills on `godot-qa-engineer` and raise the `GDW-08` cap from 11 to 13 when `skills_when` matches.
   - **Recommendation**: **Option 1**. `godot-bot-balance-loop` and `godot-visual-verification` are specialized workflows compared to `godot-testing`, `godot-debugging`, `godot-build-verify`, `godot-test-loop`, and `godot-headless-runs`, and keeping the 11-skill cap avoids bloating the launch prompt of every `godot-qa-engineer` session.

---

## Further Notes

- **ADR updates required by the brief**:
  - Update [`docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md`](../adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md) for the nested Cargo crate grouping, the 3 clippy lints (`cognitive_complexity`, `too_many_lines`, `too_many_arguments`), and the `gdcomplexity.py` (`gdtoolkit.parser`) GDScript complexity route.
  - Add a new ADR (`docs/adr/0195-code-and-comment-sweeps-adapt-to-project-languages.md`, or next available number) recording why `code-sweep` pairs a domain reviewer with the cross-vendor reviewer and why `comment-sweep` excludes Godot structured data files (`.tscn`, `.tres`, `.import`, `.uid`, `.gdextension`, `project.godot`).
- **Requirement coverage gate (`scripts/check-req-coverage.sh`)**:
  - `scripts/check-req-coverage.sh` checks every `| <ID> |` row in any table whose second header cell is `Requirement` or `Check`. The `## Requirement Table` section uses the `Requirement` header so that the gate step (`scripts/check-req-coverage.sh`) verifies `grs_01_*` through `grs_18_*` as each unit lands. Every implementation ticket that introduces or owns a `GRS-NN` row must commit its `grs_<nn>_*` test in the same commit (`docs/testing-and-gates.md:95-96`).
