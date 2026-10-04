# V0 vendoring-infra: let the catalog carry vendored skills safely

Unit slug: `vendoring-infra`. Branch: `ds/vendoring-infra`.

## GOAL

The bundled catalog accepts vendored upstream skills: a `vendored: true` provenance flag that exempts the skill
from the size budget, provenance pins checked by shape instead of a
hard-coded list, and a build script that never bundles dotfiles. Six skill
units rebase on this, so land it fast.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`.
- `crates/horch-core/tests/skills_catalog.rs`: `skills_bundled_text_only`
  (only `.md`/`.txt`, `skill-creator` exempt), `skills_bundled_size_budget`
  (`EXEMPT_FROM_BUDGET`), and the pin check in
  `skl_01_bundled_catalog_versions_and_digests` (`PINNED_REPOSITORY`,
  `DESIGN_SOURCE_PINS`).
- `crates/horch-core/src/skills/catalog.rs` (provenance parsing),
  `crates/horch-core/build.rs` (`collect_skill_files` bundles every regular file).

## FILES

own: `crates/horch-core/src/skills/catalog.rs` (the `vendored` flag only),
`crates/horch-core/build.rs` (skip dotfiles), `crates/horch-core/tests/skills_catalog.rs`,
`skills/README.md` (a "Vendored skills" paragraph explaining verbatim vs adapt,
re-vendor procedure: bump the pin, re-copy, re-hash, re-run
the gate), `ai_docs/reports/domain-skills/vendoring-infra.md`.

## STEPS

0. Create the worktree.
1. `vendored: bool` (serde default false) on provenance; `horch skills show`
   prints it.
2. Text-only test: unchanged.
3. Budget test: exempt every skill whose provenance says `vendored: true`
   (keep `EXEMPT_FROM_BUDGET` for `skill-creator`, or mark it vendored).
4. Pin check: a non-marketplace source needs an `https://github.com/<o>/<r>`
   repository, a 40-hex revision and a 64-hex sha256.
   Keep the exact check for the skill-marketplace pin. Drop
   `DESIGN_SOURCE_PINS` only if the design entries still pass the shape check.
5. `build.rs`: skip any file or directory whose name starts with `.`.
   Test: a fixture or a unit test that a dotfile is not bundled.
6. Gate. Commit `Skills: Accept vendored skills`. Report.
