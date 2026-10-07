# Spec: the Godot wave (GDW)

| | |
|---|---|
| Status | Implemented: 64 `godot-*` skills, 19 `godot-*` seats with a `codex-sol` fallback, `Requirement::Godot`, `skills_when`, the `godot-run.sh` wrapper and 4 Godot checks in `just gate`. The real-project seat runs and the Codex trial are live checks (§4). A run of `godot-run.sh` inside a real Codex pane is pending. |
| Date | 2026-10-07 |
| Authors | opus-2 (this spec). The wave: units GW1 to GW13, U-22, U-25, U-49. |
| Evidence | The wave report (`ai_docs/reports/godot-wave.md`, committed text at `c6b1a48`) and its status check (`ai_docs/reports/godot-wave-status.md`, items 1 to 21, checked at `32639cd`). |

---

## 1. Scope

The Godot wave gives the fleet a Godot team: a catalog of Godot skills, the
seats that load them, a host-tool check for Godot, and gate checks that keep
the skills true to Godot 4.7.2. This spec records the requirements and the
tests that prove them. User docs: [`docs/skills-and-teams.md`](../skills-and-teams.md)
(the `skills_when` and `requires: [godot]` rows, the Godot team) and
[`docs/testing-and-gates.md`](../testing-and-gates.md) (the gate step
`godot_skills`).

The roster types this wave adds or uses:

- `Requirement` (`roster::Requirement`): a host tool a teammate cannot work
  without. `Requirement::Godot` is spelled `godot`.
- `requires:` (`Teammate.requires`): the teammate's requirements. `horch
  doctor` checks each one when the project is offered a teammate that names it.
- `offer_when:` (`Teammate.offer_when`): the entry names that make a teammate
  relevant. `ProjectFacts` holds the names at the top level and 1 level down.
- `skills_when:` (`Teammate.skills_when`): a pattern and the skills it adds as
  expected skills when the project has a matching entry.

## 2. Requirements

The Python tests named in the table run in the gate step `godot_skills`
(`python3 -m unittest discover -s scripts/godot/tests`). The coverage check
(`scripts/check-req-coverage.sh`) counts only the Rust tests, so every ID has
a `gdw_<nn>_` test in `crates/horch-core/tests/godot_wave.rs`.

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| GDW-01 | `Requirement::Godot` exists and is spelled `godot`. Every `godot-*` seat sets `requires: [godot]`. `horch doctor` finds Godot through `GODOT_PATH` (unchecked, an empty value counts as unset), then `godot` on PATH, then the macOS app (`/Applications/Godot.app/Contents/MacOS/Godot`). It reports a Godot older than 4.3 (`GODOT_MIN`) or without a version, and a missing Godot with both fixes. | - | gdw_01_godot_requirement_is_spelled_godot_and_every_seat_requires_it, gdw_01_godot_lookup_is_the_variable_then_path_then_the_app, requires_parses_every_host_tool, host_tool_bin_tries_the_variable_then_path_then_the_app, godot_on_path_is_fine, godot_path_wins_over_path, the_app_bundle_is_the_last_fallback, missing_godot_names_both_fixes, godot_below_the_floor_or_without_a_version_is_reported, godot_version_reads_the_dotted_line |
| GDW-02 | Every `godot-*` seat sets `offer_when: ["project.godot"]`. The match is on an entry name at the top level of the project or 1 level down; dot-directories are skipped. A project without `project.godot` is offered no Godot seat. With no project facts (`horch teammates`), every seat is listed. | - | gdw_02_godot_seats_are_offered_only_in_a_godot_project, offered_when_a_top_level_entry_matches, offered_when_an_entry_one_level_down_matches, hidden_when_nothing_matches, no_facts_offers_everything_and_hidden_is_never_offered, project_facts_reach_one_level_down |
| GDW-03 | In a project with a `*.csproj`, `skills_when` adds `godot-csharp-godot` and `godot-csharp-signals` to the 15 builder seats as expected skills (they leave `available_skills:`). `godot-csharp-engineer` names both in `skills:`. `godot-tech-lead`, `godot-code-reviewer` and `godot-qa-engineer` have no `skills_when`. A GDScript-only project adds nothing. The rule applies at spawn, in the dataset run and in the ledger record. | - | gdw_03_a_csharp_project_adds_the_csharp_skills_to_the_builder_seats, skills_when_adds_the_skills_of_every_matching_pattern, skills_when_without_a_match_or_with_a_named_skill_adds_nothing, skills_when_parses_from_frontmatter, skills_when_reaches_the_record_and_the_launch_teammate, dataset_roster_carries_the_project_facts |
| GDW-04 | A bundled skill meets the size budget (`SKILL.md` at most 12 KB, the directory at most 160 KB), or its `skills/copied.json` entry gives a `budget_exempt` reason. A renamed or combined `godot-*` skill is not `verbatim`. A reason on a skill under the budget is stale and fails. | - | gdw_04_a_godot_skill_over_the_size_budget_gives_a_reason, skills_bundled_size_budget, skills_budget_exempt_only_over_budget, skills_verbatim_lists_every_file |
| GDW-05 | The gate checks every `Class.member` and bare class name in the `godot-*` skills against the Godot API (`scripts/godot/api_check.py`): an unknown name fails with its file and line. Deprecated names come from the pinned `scripts/godot/deprecated-4.7.2.json`; addon classes from `scripts/godot/known_classes.txt`. Without Godot the check prints `skipped: no Godot` and passes. | - | gdw_05_api_check_flags_unknown_names_against_the_dump, gdw_05_api_check_skips_without_godot, gdw_05_the_gate_checks_names_against_the_pinned_4_7_2_data; Python: test_known_names_pass, test_unknown_names_fail_with_file_and_line, test_no_godot_skips, test_pinned_deprecations_for_4_7_2_are_committed, test_auto_mode_uses_the_pinned_deprecations_of_the_version |
| GDW-06 | The gate step `godot_skills` also runs the Godot unit tests, `xref_check.py` (every `godot-<name>` mention names a skill, every relative link resolves), `gdscript_blocks_check.py` (every GDScript block parses; `skipped: no Godot` without Godot) and `csharp_blocks_check.py` (every C# block compiles; `skipped: no dotnet` without dotnet). A failure in a copied file counts per skill; a count above `scripts/godot/copied-baseline.json` fails the step, and a count at or below it passes. | - | gdw_06_xref_check_passes_on_the_bundled_godot_skills, gdw_06_block_checks_skip_without_godot_or_dotnet, gdw_06_copied_failures_fail_only_above_the_baseline; Python: test_broken_references, test_clean_skill_passes, test_no_godot_skips, test_strict_own_counts_copied_and_fails_own, test_copied_count_above_the_baseline_fails, test_copied_blocks_are_counted_against_the_baseline, test_counts_at_the_baseline_pass_with_a_summary_only, test_a_count_above_the_baseline_fails_and_lists_that_skill |
| GDW-07 | The bundled catalog holds 64 `godot-*` skills. It holds none of the excluded upstream skills (`using-godot-prompter`, `godot-mentor`, `godot-master`, `godot-agent-vision`, `godot-monte-carlo-balancer`, `godot-theme-easter`). No `godot-*` file is a licence or notice file, and no text has a licence line or an upstream credit. | - | gdw_07_the_catalog_has_64_godot_skills_and_no_excluded_one, gdw_07_godot_skills_carry_no_licence_or_credit_line, skills_bundled_no_licence_files |
| GDW-08 | The roster has 19 `godot-*` seats. Each seat expects 5 to 10 skills, and every skill a seat names is bundled. Every `godot-*` skill is expected or available in at least 1 seat. | - | gdw_08_every_godot_seat_expects_5_to_10_skills_from_the_catalog, gdw_08_every_godot_skill_has_a_seat |
| GDW-09 | Every `godot-*` seat runs on Claude with `base: fleet-worker` and `mcp_servers: {}` (no Godot MCP server in wave 1). `godot-code-reviewer` denies `Edit`, `Write` and `NotebookEdit`. | - | gdw_09_godot_seats_are_claude_fleet_workers_without_mcp_servers |
| GDW-10 | Every `godot-*` seat falls back to `codex-sol`. `godot-build-verify` ships `scripts/godot-run.sh`: private user dirs, the 2 TLS overrides under `CODEX_SANDBOX` on macOS, `--headless`, and exit 3 on a sandbox line. | - | gdw_10_every_godot_seat_falls_back_to_codex_sol, gdw_10_godot_build_verify_ships_an_executable_godot_run_script; Python: test_no_project_exits_2, test_user_dirs_are_private, test_headless_is_added_once, test_exit_code_passes_through, test_output_is_logged, test_e3_line_exits_3, test_error_path_outside_project_exits_3, test_error_path_inside_project_is_not_sandbox, test_without_sandbox_writes_no_tls_files, test_sandbox_writes_override_and_editor_setting, test_sandbox_keeps_other_editor_settings, test_sandbox_keeps_override_with_key, test_sandbox_refuses_override_without_key |

## 3. Decisions

| Decision | Date | Source |
|---|---|---|
| Licence: the copied skills keep no licence file, licence reference or upstream credit. gd-agentic content is rewritten in own words, not copied. | 2026-10-04 | Operator rule; the wave report, "Operator decisions (2026-10-04)", item 1 (`c6b1a48`). |
| Genres: `godot-genre-blueprints` ships all 27 genres. | 2026-10-04 | The wave report, operator decision 2 (`c6b1a48`). |
| Version migration: the latest only. The skills target Godot 4.7 (4.7.2); no per-version tables for old engines. | 2026-10-04 | The wave report, operator decision 3 (`c6b1a48`). |
| Re-vendoring: frozen at GodotPrompter v1.14.0. `scripts/godot/rename.py` stays, so the rename is reviewable and repeatable; no re-vendor process is planned. | 2026-10-04 | The wave report, operator decision 4 (`c6b1a48`). |
| C#: open: the operator has not named GDScript, C# or both; `skills_when` supports both, so nothing waits on it. | 2026-10-07 | The status check, item 19 (`ai_docs/reports/godot-wave-status.md`). |
| Codex fallback: every `godot-*` seat falls back to `codex-sol`, and every Godot command runs through `godot-build-verify`'s `scripts/godot-run.sh`. | 2026-10-07 | Operator: "Codex fallback is good"; `docs/live-checks/godot-codex.md`. |

## 4. Live checks

- [`docs/live-checks/godot-csharp.md`](../live-checks/godot-csharp.md): a C#
  project gets the C# skills at launch.
- [`docs/live-checks/linux.md`](../live-checks/linux.md): Godot's XDG
  directories stay in the workspace on Linux.
- [`docs/live-checks/godot-seats.md`](../live-checks/godot-seats.md): 1 real
  task each for `godot-gameplay-programmer` and `godot-qa-engineer`.
- [`docs/live-checks/godot-codex.md`](../live-checks/godot-codex.md): 1 Codex
  pane on a Godot project under `workspace-write`, and the `godot-run.sh`
  runs that adopt its settings (GDW-10).
