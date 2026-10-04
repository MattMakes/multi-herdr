# GW11 godot-code: Requirement::Godot, horch doctor, skills_when

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Background: `ai_docs/reports/godot-wave.md` "Code changes" 1 and 3, and
"C# is a language, not a domain".

## GOAL

1. `requires: [godot]` works: `Requirement::Godot` in
   `crates/horch-core/src/roster/offer.rs`, mirroring `Blender`. Lookup order:
   `GODOT_PATH`, then `godot` on PATH, then
   `/Applications/Godot.app/Contents/MacOS/Godot` (macOS only). `--version`
   must run and report 4.3 or later (parse `4.7.2.stable.official.<hash>`).
   `GODOT_PATH` goes into `runtime::context::Inherited` next to
   `blender_path` (sonnet-15 added that in 9d983e4: copy the pattern; arc_05
   forbids `std::env` in core). `horch doctor` checks it like Blender, with
   the same warning style. Tests: each lookup branch, the version floor, a
   missing engine.
2. `skills_when: {"<glob>": [skills]}` teammate field: when a project fact
   matches the glob (the same facts and matching rules `offer_when` uses —
   read `offer.rs`), the listed skills are added as expected skills for that
   launch and recorded in the ledger like any other. Validation: each skill
   must exist (`horch teammates --check`). Template doc in
   `teammates/_template.md` and the template-fields test. Example for the
   report: `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}`.

## ORDER

Do goal 1 first and commit it. Goal 2 touches `skills/activation.rs`, which
opus-72 (unit G4) owns now. Before you edit `skills/activation.rs`,
`execution/plan.rs` or `harness/launch.rs`, ask the orchestrator; it tells you
when G4 has committed. Design goal 2 meanwhile (where project facts reach the
activation plan at spawn and at fleet start, see `ada702d` for the
orchestrator record) and put the design in your report.

## FILES

own: `crates/horch-core/src/roster/offer.rs`, `roster/teammate.rs`,
`roster/validation.rs`, `crates/horch-core/src/runtime/context.rs`
(`godot_path` only), `crates/horch/src/cmd/doctor.rs` (the Godot check),
`teammates/_template.md` (the 2 fields), `docs/recipes/add-teammate.md`
(`requires` values), their tests, and after the orchestrator's go,
`skills/activation.rs`. Report: `ai_docs/reports/godot/gw11-godot-code.md`.

do not touch: `skills/` content, `teammates/godot-*` (GW12),
`competition/`, `messaging/`, `dataset/`.

## CHECKS

The tests you add, roster lib tests, `arch_scan`, doctor tests,
`horch teammates --check` (built binary), clippy -D warnings on horch-core
and horch, rustfmt on your files. COMMITTED per goal.
