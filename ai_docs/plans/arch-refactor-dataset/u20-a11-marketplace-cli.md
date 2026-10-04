# U20 a11-marketplace-cli: horch skills / horch marketplace commands and just recipes

Unit slug: `a11-marketplace-cli`. Branch: `ard/a11-marketplace-cli`. Phase: A11.
Requirements: MKT-08 (A11 parts), MKT-09.

## GOAL

The operator can list, show, install, update and check skills, and list and
refresh the marketplace, from `horch` and from `just`. The legacy
`horch skills [--phase] [--json]` output is byte-identical. An installed git
skill is pinned and materialized locally, so a later spawn needs no network.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD3 (marketplace store
  `${XDG_DATA_HOME:-~/.local/share}/horch/` = `ctx.paths.data_root`), §3
  "A8" and "A11", §4 MKT rows, §5 LA-5.
- Design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`:
  marketplace types and the A11 section.
- Merged reports in `ai_docs/reports/arch-refactor-dataset/`:
  `a8-marketplace.md` (public API: `parse_source`,
  `Installer::new(store_root, git)`, `install(source, opts, observer)`,
  `update(id)`, `reinstall_from_lock()`, `GitRunner`; store layout
  `staging/`, `skills/<id>/<version>/`, `marketplace.lock`),
  `a9a-skills.md` (`SkillCatalog::bundled()`, `with_lock`, `plan_activation`,
  `MaterializedSkills::materialize` which refuses marketplace entries today;
  marketplace entries have an empty description; lock override rule),
  `a2-runtime.md` (`RuntimeContext`: `paths.data_root`,
  `bins.overrides.git` / `HORCH_GIT_BIN`), `a0-oracles.md` (CLI skills
  oracles in `crates/horch/tests/oracles/skills/`).
- Today `crates/horch/src/main.rs` defines `Command::Skills { phase, json }`
  (about line 289) and renders it at about line 408 from
  `horch_core::skills::describe(phase)`.
- Parallel units: U18 `a4-harness` (harness modules, `worker.rs`,
  `recipes.rs`), U19 `a6a-store` (`ledger.rs`, `execution/`). Do not touch
  their files. A10 (later) wires the activation plan into each harness's
  skill exposure; your job is to make marketplace skills materializable.

## FILES

own:
- `crates/horch/src/cmd/skillscmd.rs`, `crates/horch/src/cmd/marketplacecmd.rs` (new)
- `crates/horch/src/cmd/mod.rs` (your `pub mod` lines), `crates/horch/src/main.rs` (the `Skills` and new `Marketplace` subcommands only)
- `crates/horch-core/src/skills/materialize.rs` and `skills/catalog.rs`
  (only to materialize marketplace entries from the store and to load the
  lock from `data_root`)
- `justfile` (new recipes only)
- `crates/horch/tests/skills_cli.rs` (new), `crates/horch-e2e/tests/marketplace.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a11-marketplace-cli.md`

do not touch: oracle data, `crates/horch-marketplace/**` (ask first if its
API lacks something), U18 and U19 files.

## STEPS

1. Create the worktree (conventions §2).
2. Restructure `horch skills` as a subcommand group without changing the
   legacy forms: `horch skills`, `horch skills --phase <p>`,
   `horch skills --json`, `horch skills --phase <p> --json` print exactly
   what they print today (also accept `horch skills list ...` as an alias).
   Move the rendering into `cmd/skillscmd.rs`.
3. New subcommands (all take `&RuntimeContext`; the store root is
   `ctx.paths.data_root`, the git bin is the resolved git from ctx):
   - `horch skills show <id> [--json]`: id, version, source, digest,
     description, provenance, install path.
   - `horch skills install <source>[@rev] [--path <subdir>]`: marketplace
     `install`; print the pinned commit and version.
   - `horch skills update [<id>]`: re-resolve and reinstall; print old →
     new version per id.
   - `horch skills doctor`: verify every lock entry's digest on disk and
     report missing or tampered versions (exit 1 on any problem).
   - `horch marketplace list [--json]`: lock entries;
     `horch marketplace refresh`: `reinstall_from_lock()` for any missing
     version dir.
4. Materialize marketplace skills: `SkillCatalog` loads the lock from
   `data_root` (when present) via `with_lock`, and
   `MaterializedSkills::materialize` copies a marketplace entry's files from
   `<data_root>/skills/<id>/<version>/` (validate `<version>` and `<id>`
   are plain path segments first) instead of refusing it. Fill the
   marketplace entry's description from its SKILL.md frontmatter.
5. `justfile` recipes (each a thin call, with a one-line comment):
   `skills *ARGS` → `horch skills {{ARGS}}`, `skills-install SOURCE`,
   `skills-update *ID`, `skills-doctor`, `marketplace-refresh`.
6. Tests:
   - `mkt_09_legacy_skills_flags_output_unchanged` (in
     `crates/horch/tests/skills_cli.rs`): the 4 legacy forms equal the A0
     oracle files.
   - `mkt_09_cli_list_show_json`: `skills show <bundled id> --json` and
     `marketplace list --json` have the documented keys.
   - `mkt_08_e2e_install_update_repeatable` (in
     `crates/horch-e2e/tests/marketplace.rs`, with `Harness::with_git()`
     and a local bare fixture repo holding one valid skill): install pins a
     SHA; a second install of the same spec is a no-op; moving the branch
     and running `update` changes the version; `doctor` passes.
   - `mkt_08_runtime_needs_no_network`: after an install, run a spawn (or
     the narrowest command that materializes skills for a teammate whose
     `skills:` lists the installed id; read the e2e spawn tests for the
     pattern) with `HORCH_GIT_BIN=/nonexistent`; it succeeds and the
     materialized bundle contains the skill's SKILL.md.
7. Gate after each step. Commits: `A11: Move horch skills into a subcommand group`,
   `A11: Add skills show/install/update/doctor and marketplace commands`,
   `A11: Materialize marketplace skills from the store`, `A11: Add just recipes`,
   `A11: Add MKT-08 and MKT-09 tests`.
8. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The 4 named tests pass; `oracle_cli_skills_match` passes unchanged.
- `just skills` prints the legacy output.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: commands, store paths, gotchas for A10.
