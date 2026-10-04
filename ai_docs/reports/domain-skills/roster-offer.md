# roster-offer: offer domain teammates only where the project needs them

Unit `roster-offer`, branch `ds/roster-offer`. Plan:
`ai_docs/plans/domain-skills/v2-roster-offer.md`.

## What changed

| file | change |
|---|---|
| `crates/horch-core/src/roster/offer.rs` (new) | `ProjectFacts`, `Requirement`, the pure filter `offered_in(teammate, Option<&ProjectFacts>)`, the glob matcher, `pattern_problem`, 7 tests |
| `crates/horch-core/src/roster/teammate.rs` | 2 fields: `offer_when: Vec<String>`, `requires: Vec<Requirement>`. Both are skipped in serialization when empty |
| `crates/horch-core/src/roster/repository.rs` | `Roster.project: Option<ProjectFacts>`, `Roster::with_project_facts`. `offered()` (and so `roster_lines()`) applies the filter |
| `crates/horch-core/src/roster/mod.rs` | module and re-exports |
| `crates/horch-core/src/roster/validation.rs` | `check` refuses an empty `offer_when` pattern or a pattern with a path separator |
| `crates/horch-core/src/prompts.rs` | test only: the orchestrator briefing hides a non-matching domain teammate, shows a matching one, and keeps every other teammate |
| `crates/horch/src/cmd/recipes.rs` | `project_facts(dir)` reads the project. `pane_launch` attaches the facts for the 2 fleet orchestrator pane kinds. 1 test |
| `crates/horch/src/cmd/doctor.rs` | `requirement_problems` and the Xcode check. `doctor` counts the offered teammates with the project facts. 4 tests with a fake `xcodebuild` |
| `crates/horch/src/cmd/teammatescmd.rs` | 1 extra line, `offered when the project has: <globs>`, under a teammate that sets `offer_when`. The orchestrator approved this file. The template test lists the 2 new fields |
| `teammates/_template.md`, `docs/recipes/add-teammate.md` | the 2 fields are documented |

## Decisions

- **Field shape.** `offer_when` is a list of name globs. Any 1 match is
  enough. The globs support `*` and `?`, are case-sensitive, and match an
  entry NAME (file or directory). `*.xcodeproj` and `*.xcworkspace` are
  directories, so directories count. A pattern with `/` can never match, so
  `--check` refuses it.
- **Facts.** `project_facts` collects the names at the top level and 1 level
  down. It does not go into dot-directories (`.git`, `.worktrees`). An
  unreadable directory gives empty facts, which hides every `offer_when`
  teammate. The filter is pure. Only the CLI reads the filesystem.
- **No facts means offer everything.** `Roster::builtin()`, `horch spawn`,
  `horch teammates`, and the worker briefings do not attach facts. So the
  goldens and oracles do not change, and `horch spawn <name>` works in any
  project. Only the fleet orchestrator panes and `horch doctor` attach facts.
  The fixed `orchestration` recipe keeps the full list.
- **`requires: [xcode]`** is an explicit field, not derived from a name
  prefix. It is an enum, so a typo fails at load (`deny_unknown_fields`
  style). `doctor` checks a requirement only when an OFFERED teammate (after
  the filter) names it.
- **The Xcode check.** `which xcodebuild`, then `xcodebuild
  -checkFirstLaunchStatus`. Exit 0 is OK. On this host it exits 0 with Xcode
  selected. Any other exit gives a warning that names
  `sudo xcodebuild -runFirstLaunch` and `xcode-select -s`. With only the
  Command Line Tools selected, the `/usr/bin/xcodebuild` shim fails, and
  doctor quotes its first stderr line. The warning is not fatal, the same as
  roster problems.

## Tests

- `roster::offer::tests`: 7 tests. They cover a top-level match, a match 1
  level down, no match, no `offer_when`, no facts, hidden, and bad patterns.
- `prompts::tests::orchestrator_briefing_offers_a_domain_teammate_only_where_it_matches`.
- `cmd::recipes::tests::project_facts_reach_one_level_down`: covers 2
  levels down and a dot-directory.
- `cmd::doctor::tests`: 4 tests. They cover first launch done, first launch
  pending, no `xcodebuild`, and no offered teammate that needs Xcode.
- The gate is green. No golden or oracle changed.

## Gotchas

- The first gate run failed 1 test,
  `horch-e2e --test fakes fake_opencode_writes_transcript_rows_when_asked`
  (`launch.status.success()`). It passed 3 of 3 times alone, and the second
  full gate was green. After the rebase, 1 gate run failed
  `fake_opencode_session_list_matches_cwd` (`opencode --version` gave empty
  stdout), and the next run was green. Both tests run the fake opencode
  binary and do not touch the roster. They are flaky under load from
  parallel worktree builds. Commit a4a0640 already works on this area. I did
  not fix it because it is outside this unit.
- `pane_launch` wiring itself has no direct test, because the function
  launches an agent. The prompts test covers the same roster path.

## Follow-ups (not done)

- No shipped teammate sets `offer_when` or `requires` yet. The Unreal and
  Swift teammate units add `offer_when: ["*.uproject"]` and
  `offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]` with
  `requires: [xcode]`.
- `horch spawn` does not warn when a teammate's `offer_when` does not match
  the project. The plan says spawn works anywhere, so I left it.
