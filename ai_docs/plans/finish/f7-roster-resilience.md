# F7 roster-resilience: one bad teammate file never takes the roster down

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

When one teammate file fails to parse (an unknown field value from a newer
roster, a typo), `horch spawn`, `horch teammates`, `horch fleet` and the
orchestrator briefing keep working with every other teammate, print one
warning that names the file and the error, and refuse only that teammate.
`horch teammates --check` still fails on it (the gate stays strict).

## CONTEXT

- 2026-10-04: the installed `horch` reads `HORCH_TEAMMATES_DIR` live from
  `/Users/mascott/projects/multi-herdr/teammates`. `blender-artist.md` gained
  `requires: [blender]` before the installed binary knew the value, and
  every `horch spawn` failed: "in .../blender-artist.md: parsing frontmatter:
  requires[0]: unknown variant `blender`, expected `xcode`".
- Decide per layer: built-in teammates compiled into the binary must always
  parse (a test pins it); overlay and `HORCH_TEAMMATES_DIR` files are the
  ones that can be ahead of the binary.
- A teammate that overrides a built-in and fails to parse: fall back to the
  built-in, with the warning (say so in the warning).
- Spawning the broken teammate by name gives the parse error, not "unknown
  teammate".

## FILES

own: `crates/horch-core/src/roster/repository.rs` (loading), `roster/mod.rs`,
`roster/validation.rs` (check stays strict), the CLI places that print
roster warnings, tests, `ai_docs/reports/finish/roster-resilience.md`.

## STEPS

1. Loading change + tests (bad overlay file, bad override of a built-in,
spawn by name). 2. `--check` still fails (test). 3. Targeted checks; COMMITTED.
