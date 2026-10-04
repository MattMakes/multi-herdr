# V0 vendoring-infra report

Branch: `ds/vendoring-infra`. Plan: `ai_docs/plans/domain-skills/v0-vendoring-infra.md`.

## What changed

- `crates/horch-core/src/skills/catalog.rs`: `Provenance` has `vendored: bool`
  (serde default `false`). `parse_provenance` reads `"vendored": true` from a
  `provenance.json` entry. The JSON of `horch skills show --json` now always
  carries `provenance.vendored`.
- `crates/horch/src/cmd/skillscmd.rs` (granted by the orchestrator for this
  1 change): the text output of `horch skills show` prints
  `vendored:     true` after `adaptation:`, only when the flag is true. No CLI
  test or oracle changed.
- `crates/horch-core/build.rs`: `collect_skill_files` skips every file and
  directory whose name starts with `.`.
- `crates/horch-core/tests/skills_catalog.rs`:
  - `skills_bundled_text_only`: a file named exactly `LICENSE` is allowed in
    any skill directory. `skill-creator` stays exempt (it has `LICENSE.txt`
    and scripts). Vendored skills are not exempt.
  - `skills_bundled_size_budget`: exempt = `EXEMPT_FROM_BUDGET`
    (`skill-creator`) plus every skill whose provenance has `vendored: true`.
  - `skl_01_bundled_catalog_versions_and_digests`: the skill-marketplace
    source keeps its exact pin. Any other source must pass
    `assert_pinned_upstream_shape`: `https://github.com/<owner>/<repo>`,
    40-hex revision, 64-hex sha256, non-empty licence.
  - `DESIGN_SOURCE_PINS` is dropped. All 7 design repositories pass the shape
    check (GitHub URL, 40-hex revision, licence `MIT` on every source).
  - `skills_multi_source_provenance_parses` asserts `vendored` parses and
    defaults to `false`.
  - New `skills_bundled_skip_dotfiles`, with the fixture
    `skills/.dotfile-fixture`. I proved it: with the `build.rs` skip disabled,
    the test fails with `.dotfile-fixture is a dotfile`.
- `skills/README.md`: new "Vendored skills" section (vendor vs adapt, LICENSE
  kept, `vendored: true`, the 4-step re-vendor procedure). The budget
  sentence in "Multi-source skills" names the LICENSE file and the vendored
  exemption.

## Decisions

- `skill-creator` keeps its `EXEMPT_FROM_BUDGET` entry and keeps its skip in
  the pin check. Its entry uses the single-source fields, which cannot carry a
  licence, so it cannot pass the shape check. Marking it vendored needs a
  `provenance.json` edit, which this unit does not own.
- The fixture is a top-level dotfile in `skills/`, not a file inside a skill
  directory. That keeps every skill digest unchanged even if the skip
  regresses.
- `vendored` is serialized always (no `skip_serializing_if`), so the JSON
  view of `skills show` is uniform.

## Follow-ups (outside scope)

- `skill-creator` could move to the `sources` shape with
  `"license": "Apache-2.0"` and `"vendored": true`. Then `EXEMPT_FROM_BUDGET`
  shrinks to the text-only exemption, and the pin-check skip goes.
- The shape check does not verify a revision against the `_sources/*/PINS.txt`
  files. Those files are outside the repository, so a hermetic test cannot
  read them.

## Gate

`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`: GATE GREEN.
`skills_catalog`: 11 of 11 pass.
