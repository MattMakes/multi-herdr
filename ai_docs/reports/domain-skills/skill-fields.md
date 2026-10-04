# V1 skill-fields: `available_skills:` and `operator_skills:`

Branch `ds/skill-fields`. Commits:
`Roster: Add available_skills and operator_skills teammate fields`,
`Docs: Document the skill fields and report V1`.

## What it does

- `available_skills: [<id>, ...]` names catalog skills (bundled or
  marketplace) that a teammate gets by name only. `plan_activation` gives
  them the new policy `InvocationPolicy::Offered`. They are in `activated`,
  so the launch materializes them. The briefing names them under "Also
  available" (or "Available native skills" when the teammate has no
  expected skills), without a description.
- `operator_skills: {dir, names}` names skill directories on the
  operator's machine. The launch reads `<dir>/<name>/`, copies it into the
  skill bundle, and checks the copy's tree digest. The skills are expected:
  the briefing names each one with its SKILL.md description, after the
  `skills:` entries.
- Every harness that exposes skills reads the bundle's `skills/` directory,
  so no adapter changed. The e2e tests prove Claude (plugin `horch`) and
  Codex (private `CODEX_HOME`).
- `horch teammates --check` reads the same directory the launch reads and
  fails on:
  - a missing `dir`;
  - a missing name (no `<dir>/<name>/SKILL.md`);
  - an invalid SKILL.md (the catalog rules: `name:` equals the directory,
    a description of 1 to 1024 bytes);
  - a name that is named twice;
  - a name that a bundled or marketplace skill has;
  - a subagent skill: `device-interaction` by name, or a SKILL.md that
    contains "SUBAGENT skill" or "Agent tool". The error says "is a
    subagent skill (...); fleet panes start no subagents";
  - a tree that the marketplace digest rules refuse, for example a symlink;
  - a teammate whose harness cannot load skills, or with `disable_skills`.
- `--check` also fails on `available_skills`:
  - an unknown name;
  - a name that is also in `skills:`;
  - `orchestrate` or `skill-creator` on a teammate that is not an
    orchestrator.

## Decisions

- **No `operator:` prefix.** A name clash with a catalog skill fails
  `--check` and fails the launch. One bundle directory holds one skill per
  name, so a clash cannot be resolved by a prefix in the briefing alone.
  The operator renames one skill, or names the catalog skill in `skills:`.
  On Claude the skill appears as `horch:<name>`, the same as a bundled one.
- **Operator skills are catalog entries.** `CatalogSource::Operator { dir }`
  with version `operator+<digest12>` and the marketplace tree digest
  (`horch_marketplace::integrity::tree_digest`).
  `SkillCatalog::with_operator_skills(teammate, home)` adds them per
  teammate. Activation, materialize and the briefing then treat them like
  other entries. The digest check is the marketplace one: the copy must
  match the digest that planning read. A source that changes between plan
  and copy fails the launch.
- **`~` expands from the launch context.** `Bundle::install_from` takes the
  home from `RuntimeContext::inherited.home_var`. `--check` takes it from
  `Roster::home`. Neither reads `std::env`.
- **`plan_launch` stays pure.** `plan_activation` makes an operator skill
  Explicit only when the catalog holds it as an Operator entry. The roster
  catalog that `horch spawn` passes to `plan_launch` does not hold them, so
  the ledger record's `skills` list does not include operator skills. The
  launch bundle does.
- **Precedence.** Explicit (`skills:` and operator skills), then
  Deterministic (the phase), then Offered. A phase skill named in
  `available_skills` stays Deterministic.
- **Both fields serialize always** (no `skip_serializing_if`). The test
  `the_template_documents_exactly_the_teammate_fields` compares the
  serialized default with `_template.md`. No oracle serializes a teammate,
  so no oracle changed.

## Files

- `crates/horch-core/src/roster/teammate.rs`: the 2 fields and
  `OperatorSkills`. `roster/mod.rs`: 1 re-export.
- `crates/horch-core/src/roster/validation.rs`: the `--check` rules.
- `crates/horch-core/src/skills/activation.rs`: `Offered`, precedence.
- `crates/horch-core/src/skills/selection.rs`: `selected` includes
  `available_skills`; `operator_names`.
- `crates/horch-core/src/skills/catalog.rs`: `CatalogSource::Operator`,
  `with_operator_skills`, `subagent_skill`.
- `crates/horch-core/src/skills/materialize.rs`: copy and digest check for
  operator entries. The marketplace messages did not change.
- `crates/horch-core/src/skills.rs`: `Bundle::install_from` takes `home`;
  the briefing declares operator skills as expected.
- `crates/horch-core/src/harness/launch.rs`: passes the launch home.
- `crates/horch-core/src/routing/decision.rs` (approved by the
  orchestrator): `merge` keeps both fields from the original teammate.
- `crates/horch/src/cmd/cost.rs` (approved): `expected_skills` includes
  operator skills.
- `teammates/_template.md`, `docs/recipes/add-teammate.md`: the fields.

## Tests

- `crates/horch-core/tests/skills_catalog.rs` (4 new):
  `available_skills_are_materialized_and_named_without_description`,
  `available_skills_check_errors`, `operator_skills_check_errors`,
  `operator_skills_materialize_with_a_digest_and_an_expected_line`.
- `crates/horch-e2e/tests/skills_exposure.rs` (3 new):
  `operator_skills_e2e_exposure_claude`, `operator_skills_e2e_exposure_codex`,
  `operator_skills_e2e_check_fails_on_a_missing_name`. A temp operator
  directory under the harness home, written as `~/.agents/skills`.
- `routing/decision.rs`: `merge_keeps_the_original_available_and_operator_skills`.
- No existing test changed. No oracle or golden changed.
- Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green.

## Gotchas

- `Bundle::install` (tests) has no home, so a `~/` operator `dir` does not
  expand there. Use an absolute path in a unit test.
- The subagent search is a plain substring search of SKILL.md for
  "SUBAGENT skill" and "Agent tool", as the plan says. A skill that only
  mentions "the Agent tool" in a warning is refused too. Rename the text or
  leave the skill out.
- The harness still lists the description of every materialized skill in
  its own skill list. `available_skills` saves briefing text, not the
  harness's own skill-list cost.
- Steps 1 and 2 are in 1 code commit. The 2 fields share the activation,
  selection and briefing edits, so a split commit would not be useful.

## Follow-ups (not done)

- The ledger record does not list operator skills (see Decisions). A
  follow-up can pass the home into `PlanInputs` and extend the catalog
  before `plan_launch`, which then reads files.
- `horch teammates --matrix` shows `skills` and `plugin_skills` only. It
  does not show `available_skills` or `operator_skills`
  (`crates/horch/src/cmd/teammatescmd.rs`, not in this unit's files).
- No shipped teammate uses the fields yet. The Swift and Unreal units
  attach them.
