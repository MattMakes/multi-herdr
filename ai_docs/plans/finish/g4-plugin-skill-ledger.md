# G4 plugin-skill-ledger: the ledger records the plugin skills a launch loads

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

A worker's ledger record lists the plugin skills its launch loads (from
`plugin_skills`, F6's filtered copy), with id, version and digest, and the
launch's skill-drift check (SKL-04, `harness/launch.rs` `skill_drift`)
covers them, so a plugin skill that changed between spawn and launch fails
the launch like a bundled skill does.

## CONTEXT

- F6 (`ai_docs/reports/finish/plugin-filter.md`, "Limits"): the ledger does
  not record the plugin skills; the copy check proves only copy == source at
  launch time.
- F1 (`ai_docs/reports/finish/operator-digest.md`) added `skill_drift` and
  the record check in `install_skills`.
- `ada702d`: `horch fleet` records the orchestrator's activated skills with
  `plan_activation`; keep fleet and spawn recording the same thing.
- Decide how a plugin skill is named in `ResolvedSkillRef` (for example
  `<plugin>:<skill>` with source `plugin:<plugin>@<marketplace>` and the
  installed version), and how the digest is taken (the same `tree_digest` F6
  checks). Record the decision in the SKL design text.
- Spawn records skills through `execution/plan.rs` `plan_activation`
  (spawn.rs builds the catalog). G5 owns `execution/plan.rs` for a resume
  check: put the plugin skills into `SkillActivationPlan` (skills/activation.rs)
  or the catalog, so `plan.rs` needs no change. If you cannot avoid
  `plan.rs`, tell the orchestrator first.

## FILES

own: `crates/horch-core/src/skills/activation.rs`, `crates/horch-core/src/skills/`
(catalog), `crates/horch-core/src/harness/claude_plugins.rs`,
`crates/horch-core/src/harness/launch.rs` (`install_skills`, `skill_drift`),
`crates/horch-core/tests/skills_exposure.rs` and related tests, the SKL
design text, `ai_docs/reports/finish/plugin-skill-ledger.md`.

do not touch: `execution/plan.rs`, `execution/lifecycle.rs`,
`crates/horch/src/cmd/spawn.rs` (G3), `messaging/`, `competition/`.

## STEPS

1. Plugin skills in the plan + record. 2. Drift check covers them (test:
   edit the installed plugin's skill after spawn → launch fails naming it).
3. Fleet orchestrator records them too (test). 4. Design text.
5. Targeted checks (skills_exposure, skills_catalog, launch lib tests,
   `cargo build --workspace --bins` then `tel_02` e2e, clippy -D warnings,
   rustfmt on your files). COMMITTED.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
