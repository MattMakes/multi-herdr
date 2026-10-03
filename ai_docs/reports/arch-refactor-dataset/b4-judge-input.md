# Report: U23 b4-judge-input

Branch `ard/b4-judge-input`. Phase B4 (part). Requirements JDG-01, JDG-02,
JDG-10.

## Files

| File | Change |
|---|---|
| `teammates/judge.md` | new; hidden, headless-only evaluator |
| `crates/horch-core/src/roster/teammate.rs` | `HEADLESS_ONLY: [&str; 1] = ["judge"]` |
| `crates/horch-core/src/roster/validation.rs` | `Roster::is_spawnable` refuses `HEADLESS_ONLY`; test `builtin_phase_defaults_...` expects `judge => None` (approved) |
| `crates/horch-core/src/prompts.rs` | test `briefings_use_horch_subcommands` skips `HEADLESS_ONLY` names (approved) |
| `crates/horch-core/src/evaluation/judge_input.rs` | new; `build_judge_input`, `blind_labels`, `scan_blindness` |
| `crates/horch-core/src/evaluation/mod.rs` | `pub mod judge_input;` |
| `crates/horch-core/tests/baseline_oracles.rs` | `SKIP_NEW_TEAMMATES`, `oracle_names()` in 3 loops |
| `crates/horch-core/tests/skills_catalog.rs` | `SKIP_NEW_TEAMMATES` in `skl_08` (approved) |
| `crates/horch-core/tests/judge_input.rs` | new; 5 tests |
| `crates/horch-e2e/tests/judge.rs` | new; 1 test |

`roster/mod.rs` is unchanged. Use `horch_core::roster::teammate::HEADLESS_ONLY`.

## Tests

- 5 integration tests in `crates/horch-core/tests/judge_input.rs`:
  `jdg_01_judge_teammate_check`, `jdg_02_bundle_digest_and_readonly`,
  `jdg_02_labels_seeded_shuffle`, `jdg_02_no_identity_cost_latency`,
  `jdg_10_identity_leak_flagged`.
- 1 e2e test in `crates/horch-e2e/tests/judge.rs`: `jdg_01_spawn_judge_refused`.
- 2 unit tests in `judge_input.rs`: `bundle_labels_continue_past_z`,
  `scan_names_the_changed_file`.
- `check-req-coverage.sh --phase B4`: JDG-01, JDG-02 and JDG-10 are ok.

## Oracle loops (the orchestrator asked for this list)

The rule: skip a teammate only when it is in `SKIP_NEW_TEAMMATES`
(`["judge"]`) and its oracle file is missing.

| Test | Reads oracle files per teammate | Change |
|---|---|---|
| `baseline_oracles.rs` `oracle_launch_matches` | yes | skip rule |
| `baseline_oracles.rs` `oracle_routing_matches` | yes (1 doc per fixture over all names) | skip rule |
| `baseline_oracles.rs` `oracle_skills_match` | yes | skip rule |
| `skills_catalog.rs` `skl_08_briefing_matches_baseline_modulo_path` | yes | skip rule |
| `skills_catalog.rs` `skl_02_activation_matches_legacy_selection` | no (legacy vs new, no file) | none |
| `routing.rs` `arc_12_decisions_match_baseline` | iterates oracle keys, not the roster | none |
| `crates/horch/tests/baseline_cli.rs` | no roster loop | none |

## Bundle layout

```
experiments/<exp>/artifacts/<round>/judge-input/      0500 after the manifest
  task.md                                             0400, caller text as is
  rubric.md                                           0400, rubric_text()
  schema.json                                         0400, schema_text()
  candidates/<L>/diff.patch                           0400, ≤ 1 MiB
  candidates/<L>/validation.json                      0400, BlindValidation
  manifest.json                                       0400, JudgeInputManifest
```

- `JudgeInputManifest`: `schema_version` "1.0.0", `round_id`,
  `rubric_version`, `schema_digest`, `labels`, `truncated_diffs`, `files`
  (every other file → sha256). `labels` and `truncated_diffs` are additions
  to design §4.7.
- `JudgeInput.digest` = sha256 of the `manifest.json` bytes.
- `BlindValidation` keeps `label` (bundle label), `gates[].{name, status,
  log_truncated}`, `mechanical_score`, `eligible`. It drops
  `validation_id`, `head_sha`, `duration_ms`, `log_ref`, `log_digest`.

## Label rule

1. Take the round's labels (`round.created.labels` order) that have a
   candidate in the input. The caller's slice order has no effect.
2. Shuffle with `SplitMix64::new(seed_from_digest(sha256(round_id)))`.
3. Name them `A`, `B`, … `Z`, `AA`, … in shuffled order.
4. Return the map in `JudgeInput.label_map` (bundle → original). It is never
   written into the bundle.

## Blindness tokens

`claude`, `anthropic`, `codex`, `openai`, `gpt-`, `opencode`, `gemini`,
`qwen`, `ollama`, `prime-agent`. Case-insensitive, on every line of each
diff, file headers too. One `BlindnessFlag` per (label, token, changed
file). `file` is the `b/` path of the last `diff --git` header, or
`diff.patch` before the first one. The diff is not edited.

## Decisions

- **Signature.** `build_judge_input(round_id, round, task_text, candidates,
  paths, git, repo)`. `RoundView` has no round id, so the caller passes it.
  `repo` is the main repository; `diff_patch` never reads a worktree
  (b2-vcs security review).
- **Input checks.** Each candidate label must be a round label, unique, and
  equal to `FrozenCandidate.label` and `ValidationReport.label`. The
  report's `head_sha` must equal the frozen `head_sha`. An empty list is an
  error.
- **Idempotency.** If `manifest.json` exists, the build reads every
  expected file and compares bytes. It writes nothing. A difference is
  `FsxError::Conflict`. Without a manifest, each file is
  `create_immutable` (an identical file from a crashed build is accepted),
  the manifest is written last, then the dirs become 0500.
- **Diff cap.** `DIFF_CAP_BYTES` = 1 MiB per candidate.

## SPEC-TODO

- `teammates/judge.md`: `SPEC-TODO(Spec B §10)` the verbatim evaluator
  prose. The body is provisional.

## Gotchas for later units

- `agent_prompt` cannot render `judge.md`: `{rubric}` and `{schema}` are
  unknown placeholders there. The headless harness (`harness/headless.rs`)
  must substitute them itself, with `rubric_text()` and `schema_text()`.
- The bundle dirs are 0500. Cleanup, retention or export code must chmod
  them to 0700 before it removes the bundle. The test helper `State` in
  `tests/judge_input.rs` does this.
- The verify path does not detect extra files in a sealed bundle.
- A crashed build that left a different file (without a manifest) gives a
  `Conflict` on rebuild. The coordinator must treat that as
  NEEDS_INTERVENTION, not retry forever.
- The blindness scan reads only diffs. `task.md` is the caller's text and
  is not scanned.
- `horch spawn` calls `is_spawnable` before any herdr, ledger or pane side
  effect, on the fresh and the resume path. A ledger record with tier
  `judge` cannot be resumed either.
- `tel_02_fleet_writes_orchestrator_record` (e2e) timed out once at a load
  average of about 12. It passed 3 of 3 alone and in the next full gate.
- Commit 1 was amended after its gate run to add the `skl_08` skip (a
  no-op without `judge.md`). The gate of commit 2 covers it.
