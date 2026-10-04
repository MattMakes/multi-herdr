# GW11 godot-code: report

Plan: `ai_docs/plans/godot/gw11-godot-code.md`. Worker: opus-84.

## Goal 1: `requires: [godot]` (COMMITTED 8ae17be)

- `crates/horch-core/src/roster/offer.rs`: `Requirement::Godot` (`"godot"`).
- `crates/horch-core/src/runtime/context.rs`: `Inherited::godot_path` reads
  `GODOT_PATH` (empty is unset), next to `blender_path`. Core reads no
  `std::env` (arc_05 holds).
- `crates/horch/src/cmd/doctor.rs`: `godot_problem` looks in this order:
  1. `GODOT_PATH`
  2. `godot` on PATH
  3. `/Applications/Godot.app/Contents/MacOS/Godot` (macOS only, and only
     when the file exists; `GODOT_APP`)

  Then it runs `--version`. The first line that starts `X.Y.` gives
  `(major, minor)`, for example `4.7.2.stable.official.ed1daf0bf` gives
  4.7. The floor is `GODOT_MIN = (4, 3)`. A version below the floor, a
  failed `--version`, an executable that cannot run, and output with no
  version are each a warning. Blender treats unparseable output as fine;
  Godot does not, because the plan says `--version` "must report 4.3 or
  later".
- `requirement_problems` takes `godot_path` as a 5th argument. Tests call
  `godot_problem` directly with an injected app path, because this host has
  `/Applications/Godot.app` (4.7.2) and a real lookup would make a "missing"
  test pass or fail by host.
- Docs: `teammates/_template.md` (`requires` values) and
  `docs/recipes/add-teammate.md`.
- Tests (doctor, 7 new): PATH, `GODOT_PATH` wins over PATH, the app bundle
  as the last fallback (used, ignored when PATH has one, skipped when
  missing), missing engine with the "needed by" line, the 4.3 floor, no
  version, the version parser. `offer.rs`: `requires: [xcode, blender,
  godot]` parses.

## Goal 2: `skills_when` (COMMITTED c3a0a73, 83bca97, 599727f)

Step 1, committed:

- `roster/teammate.rs`: `skills_when: BTreeMap<String, Vec<String>>`,
  skipped when empty.
- `roster/offer.rs`: `project_skills(t, facts)` gives the skills of every
  matching pattern, sorted, without the ones in `skills:`.
  `with_project_skills(t, facts)` moves them into `skills:` (and out of
  `available_skills:`). Then the activation plan marks them `Explicit`, the
  briefing lists them as expected, and the ledger records them like any
  other. Matching is `ProjectFacts::has_match`, the same facts and glob as
  `offer_when`.
- `roster/validation.rs` (`horch teammates --check`): each pattern follows
  `offer_when`'s rules; a pattern with no skill; an unknown catalog skill; a
  skill also in `skills:`; a skill also in `disabled_skills`; an
  orchestrator-only skill; a teammate that cannot load skills.
- `teammates/_template.md`: the field, with the example
  `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}`.
- Outside the FILES list, needed: `roster/mod.rs` (re-export) and
  `crates/horch/src/cmd/teammatescmd.rs` (the template-fields test gives
  `skills_when` 1 entry, because the field is skipped when empty).

### Wiring (step 2 83bca97, step 3 599727f)

Project facts reach a launch in 3 places, and all 3 must agree, because the
launch fails when its skills differ from the record (SKL-04,
`harness/launch.rs::install_skills`):

1. Spawn record: `execution/plan.rs::plan_launch` calls `plan_activation`
   on `draft.teammate`. The brief carries that teammate (`brief.resolved`),
   and `execution/lifecycle.rs::launch` passes it to the launch bundle. So
   one call there covers the spawn record and the worker launch, for fresh,
   resume and substituted (fallback) spawns.
2. Fleet orchestrator record: `cmd/recipes.rs::fleet` (ada702d) calls
   `plan_activation` on a roster loaded without facts.
3. Fleet orchestrator launch: `cmd/recipes.rs` pane launch takes the
   teammate from a roster that has facts (`offer_when`) and passes it to
   `run_flow`.

Done as designed. `skills/activation.rs` and `harness/launch.rs` do not
change.

- `roster/repository.rs`: `Roster::project_facts()` and
  `Roster::for_launch(t) -> Teammate`, which applies `with_project_skills`
  when the roster has facts and returns `t` unchanged otherwise.
- `execution/plan.rs`: `plan_launch` sets
  `draft.teammate = inputs.roster.for_launch(draft.teammate)` before
  `ensure_supported_in`. Planning stays pure: the facts are data on the
  roster.
- `cmd/spawn.rs`: `roster.with_project_facts(project_facts(&project))`.
  `offered()` then filters, but the spawn path does not read it (spawn by
  name works in any project).
- `cmd/recipes.rs`: add the facts to the roster in `fleet()` and apply
  `for_launch` at the record and at the pane launch.

Rejected: a `project_facts` field on `PlanInputs`. It changes every
`PlanInputs` literal, and one is in `competition/coordinator.rs` (do not
touch). The dataset coordinator therefore gets no `skills_when` skills;
its roster has no facts.

Test: `execution/plan.rs::skills_when_reaches_the_record_and_the_launch_teammate`.
A `*.csproj` fact puts the skills in the record as `Explicit` and on the
launch teammate. No match, and no facts, add nothing.

## Checks

- `cargo test -p horch --bin horch doctor`: 17 of 17 pass.
- `cargo test -p horch --bin horch teammatescmd`: 4 of 4 pass.
- `cargo test -p horch-core --lib roster::offer`: 11 of 11 pass.
- `cargo test -p horch-core --lib roster`: 43 of 43 pass.
- `cargo test -p horch-core --test arch_scan`: 13 of 13 pass.
- `cargo clippy -p horch-core -p horch --all-targets -- -D warnings`: clean.
- `rustfmt --check` on my files: clean.
- `horch teammates --check` (built binary): "roster ok: 60 teammates".
- `cargo test -p horch-core --lib execution::plan`: 4 of 4 pass.
- `cargo test -p horch-core --test execution_plan`: 15 of 15 pass.
- `cargo test -p horch --bin horch`: 75 of 75 pass.
- `cargo test -p horch-e2e --test skills_exposure`, `lifecycle`, `smoke`:
  17, 13 and 1 pass.
- `horch doctor` on this host: no Godot line (no offered teammate needs
  Godot yet).

## Notes outside scope

- 1 run of `cargo test -p horch-core --lib roster` took 461 s, and 1 run of
  a test that called `Roster::check()` 4 times took 285 s. The final run
  took 0.72 s (43 of 43 pass). I did not find the cause; other cargo jobs
  ran at the same time. My validation test calls `check_teammate` on 1
  teammate, not `check()`.
- The dataset coordinator (`competition/coordinator.rs`) plans with a roster
  that has no project facts, so a candidate gets no `skills_when` skills.
- opus-85 was not registered when I sent it the release message for
  `cmd/spawn.rs` and `cmd/recipes.rs`.
- Files I edited outside the FILES list, with the orchestrator's go:
  `roster/mod.rs`, `roster/repository.rs`, `execution/plan.rs`,
  `cmd/teammatescmd.rs`, `cmd/spawn.rs`, `cmd/recipes.rs`.
