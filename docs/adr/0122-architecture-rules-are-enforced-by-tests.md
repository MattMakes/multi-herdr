# Architecture rules are enforced by tests

Rules about where code lives, how it is named and what ships are checked by
integration tests that scan the source tree and the build, not left to
review. A rule that only lives in a document is not enforced. The scans are
`crates/horch-core/tests/arch_scan.rs`, `tests/architecture_doc.rs`,
`tests/skills_catalog.rs` and `tests/w4_145_build_script_pins.rs`.

## Consequences

- A child's environment is cleaned in 1 place, `scrub_child_env` (#88). A
  hand-built removal of the API key is the sign of a child spawn that also
  keeps the caller's `GIT_DIR`. Only the runtime and the launch builder
  name the key.
- `docs/architecture.md` is the "what lives where" map. A test fails when a
  module or a command file has no row in it (#213).
- No horch-core type is named plain `Ledger` (#215). horch has 2 ledgers,
  the JSON session ledger and the SQLite task ledger, and a plain name
  hides which one a reader holds.
- No bundled skill ships a licence, notice or credit file
  (`skills_bundled_no_licence_files`). Operator rule of 2026-10-04: the
  bundled copies are personal and carry no licence or credit.
- Cargo never runs the tests of a build script. `w4_145_build_script_pins`
  compiles `crates/horch-core/build.rs` again as a `#[path]` module and
  calls `skill_digests`, the tree digest of each bundled skill, on a small
  skill tree in a temp dir. Build-time and run-time digests must agree.
  `build_time_digests_equal_the_run_time_digest` (in `skills/catalog.rs`)
  holds the real `skills/` tree to the run-time rule. The build-script test
  holds the cases that tree may not have: a `SKILL.md` below the top level,
  a skill whose name begins another's, a folder with no skill file, and the
  execute bit.
