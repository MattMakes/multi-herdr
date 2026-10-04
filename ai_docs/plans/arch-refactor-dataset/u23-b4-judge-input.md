# U23 b4-judge-input: the judge teammate and the blind judge input bundle

Unit slug: `b4-judge-input`. Branch: `ard/b4-judge-input`. Phase: B4 (part).
Requirements: JDG-01, JDG-02, JDG-10.

## GOAL

A read-only, headless-only `judge` teammate exists and passes
`horch teammates --check`, `horch spawn judge` is refused, and the
coordinator can build an immutable, anonymous judge input bundle (task,
rubric, schema, per-candidate diff and validation report) with a digest, a
seeded label shuffle and a blindness scan.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "B4" (`teammates/judge.md` frontmatter,
  `HEADLESS_ONLY`, `evaluation/judge_input.rs`), §4 JDG and SEC-04 rows.
- Design doc `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.7
  (`JudgeInput`, `JudgeInputManifest`, `BlindnessFlag`, `build_judge_input`)
  and §2.1 (`artifacts/<round>/judge-input/`).
- Spec B §10 (the evaluator prose that `judge.md` must hold verbatim) is not
  available. Write a provisional body (see step 2) and mark it
  `SPEC-TODO(Spec B §10)`.
- Merged code and reports (`ai_docs/reports/arch-refactor-dataset/`):
  `b4-evaluation.md` (`evaluation::rubric::{rubric_text, schema_text, RUBRIC_VERSION}`,
  `judge_policy_digest`), `b2-vcs.md` (`vcs::git::GitClient::diff_patch`
  takes the MAIN repo, not the worktree; `FrozenCandidate`;
  `evaluation::validator::ValidationReport`), `b1-measure.md`
  (`measure::paths::DatasetPaths`, `measure::projection::RoundView`),
  `b1-primitives.md` (`fsx`, `digest`, `testkit::SplitMix64`,
  `seed_from_digest`), `a3-roster.md` (roster validation; hidden teammates
  are spawnable by name today), `a0-oracles.md` (gotcha 4: the oracle tests
  iterate the roster; a new teammate must be skipped by the oracle test code,
  never given oracle files).
- Teammate frontmatter is strict (`deny_unknown_fields`). Do not add a
  frontmatter key. The headless-only rule is a constant list in code.
- Parallel units: U18 `a4-harness` edits `harness/**`, `cmd/worker.rs`,
  `cmd/recipes.rs`, 1 line of `cmd/spawn.rs`; U22 `b3-planner`; U24
  `b6-export`. The headless `claude -p` command (`harness/headless.rs`) and
  the scheduler come in a later unit; do not write them.

## FILES

own:
- `teammates/judge.md` (new)
- `crates/horch-core/src/roster/teammate.rs` (the `HEADLESS_ONLY` constant only)
- `crates/horch-core/src/roster/validation.rs` (`is_spawnable` refuses `HEADLESS_ONLY`)
- `crates/horch-core/src/evaluation/judge_input.rs` (new), `evaluation/mod.rs` (your line)
- `crates/horch-core/tests/baseline_oracles.rs`, `crates/horch/tests/baseline_cli.rs`
  (only to skip teammates that have no oracle file; never edit oracle data)
- `crates/horch-core/tests/judge_input.rs` (new)
- `crates/horch-e2e/tests/judge.rs` (new; the spawn refusal e2e)
- `ai_docs/reports/arch-refactor-dataset/b4-judge-input.md`

do not touch: oracle and golden data, U18's files, `cmd/spawn.rs`.

## STEPS

1. Create the worktree (conventions §2).
2. `teammates/judge.md`. Frontmatter exactly: `name: judge`,
   `brief_description:` (one line, under 120 chars, "Blind evaluator for
   dataset rounds. Headless only; never spawned."), `hidden: true`, no
   `base`, `agent: claude`, `model: opus`, `effort: high`,
   `inherit_plugins: false`, `mcp_servers: {}`,
   `tools: [Read, Grep, Glob]`,
   `disallowed_tools: [Agent, Edit, Write, NotebookEdit, Bash]`. Read
   `teammates/_template.md` and `roster/teammate.rs` for the exact key
   spellings; if a key in this list does not exist in the schema, send
   `QUESTION:`. Body: a provisional evaluator prompt (read only the bundle
   in the cwd; compare candidates by the rubric; never run code; answer
   with exactly one JSON object that matches the schema, no prose, no code
   fence), headed `<!-- SPEC-TODO(Spec B §10): replace with the verbatim evaluator prose. -->`,
   and ending with the placeholders `{rubric}` and `{schema}` on their own
   lines. Check: `HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
   passes. If `--check` rejects a body with no `base`, or the placeholders,
   send `QUESTION:` with the error.
3. `HEADLESS_ONLY: [&str; 1] = ["judge"]` in `roster/teammate.rs`;
   `Roster::is_spawnable` returns an error naming the rule for these names.
   Find where `horch spawn` calls `is_spawnable` (do not edit `spawn.rs`; if
   it does not call it, send `QUESTION:`).
4. Oracle tests: make the oracle loops skip teammates that have no oracle
   file and that are listed in a `SKIP_NEW_TEAMMATES: &[&str] = &["judge"]`
   constant, so a missing file for any other teammate still fails.
5. `evaluation/judge_input.rs`: `build_judge_input(round, task_text, candidates, paths, git, repo)`
   per design §4.7, writing `artifacts/<round>/judge-input/`:
   `task.md`, `rubric.md` (from `rubric_text()`), `schema.json` (from
   `schema_text()`), `candidates/<L>/diff.patch` (via `diff_patch` on the
   main repo, capped; record truncation), `candidates/<L>/validation.json`
   (the `ValidationReport` with every identity field removed: no
   execution id, no teammate, harness, model, cost, latency or history),
   and `manifest.json` (`JudgeInputManifest`). Every file 0400 after
   writing; the dir 0500 after the manifest is written. Labels: the round's
   candidate labels shuffled with `SplitMix64` seeded by
   `seed_from_digest(sha256(round_id))`, renamed to `A`, `B`, `C`, … in
   shuffled order; the label → original-label map is returned to the
   caller and never written into the bundle. Digest = digest of
   `manifest.json` bytes. Building twice for the same round is idempotent
   (same digest; existing read-only files are verified, not rewritten).
   Blindness scan: search each diff for vendor and model tokens
   (`claude`, `anthropic`, `codex`, `openai`, `gpt-`, `opencode`, `gemini`,
   `qwen`, `ollama`, `prime-agent`, case-insensitive) and return a
   `BlindnessFlag` per hit (label, token, file). Do not edit the diff.
6. Tests:
   - `jdg_01_judge_teammate_check`: the repo roster loads `judge` with the
     exact frontmatter values above; `teammates --check` passes.
   - `jdg_01_spawn_judge_refused` (in `crates/horch-e2e/tests/judge.rs`):
     `horch spawn judge "x"` exits non-zero with the rule in stderr and
     makes no herdr call.
   - `jdg_02_bundle_digest_and_readonly`, `jdg_02_labels_seeded_shuffle`
     (same round id → same mapping; different round ids → at least 2
     different mappings over 20 ids), `jdg_02_no_identity_cost_latency`
     (no bundle file contains the execution ids, teammate names, model
     names, harness names or cost fields given in the input).
   - `jdg_10_identity_leak_flagged`: a diff that contains
     `Co-Authored-By: Claude` is flagged with token `claude`.
7. Gate after each step. Commits: `B4: Add the judge teammate`,
   `B4: Refuse spawning headless-only teammates`, `B4: Skip new teammates in oracle loops`,
   `B4: Build the blind judge input bundle`, `B4: Add JDG-01, JDG-02, JDG-10 tests`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass; `teammates --check` passes; oracle tests pass.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: bundle layout, label rule, blindness tokens,
  SPEC-TODOs.
