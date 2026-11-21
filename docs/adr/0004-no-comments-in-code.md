# No comments in code; a ratchet removes the old ones

Code in this repository carries no comments. A comment drifts from the code
it sits on, and it stands in for a design that should explain itself
(`docs/specs/comment-sweep.md`). What a comment would say goes where a
program or a reader can hold it to account: a name, a type, a test, an
issue, or an ADR in this folder.

`scripts/comments/comment_lint.py check` enforces it, in the gate
(`comment_ban` in `scripts/phase-gate.sh`) and in CI (the `comments` job).
The ban is a ratchet, because the code held about 23 700 comment lines when
it began. `scripts/comments/baseline.json` records, per file, how many
lines hold a comment. A file may never hold more than its baseline, and a
file that the baseline does not name may hold none. The numbers only go
down: `check --update-baseline` lowers them and never raises one.

The `/delete-comment` skill (`.claude/skills/delete-comment/SKILL.md`) is
the way to take comments out. It sorts each one into 1 of 4 categories. All
4 are deleted; the category decides what else happens:

| Category | The comment | Besides deleting it |
|---|---|---|
| Tautology | repeats what the code already shows | nothing |
| Stale | describes code or history that is gone | nothing |
| Excuse | lets a hack stand | an issue to fix the hack |
| Decision record | holds an important, non-obvious decision | an ADR |

## Consequences

- **A comment that a program reads stays, and the lint does not count it.**
  These are: a shebang; a lint or type-checker directive (`# noqa`,
  `# type:`, `# shellcheck`, `// @ts-`, `// eslint-`); a `// SAFETY:` note,
  which clippy's `undocumented_unsafe_blocks` demands
  (`[workspace.lints.clippy]` in `Cargo.toml`); a licence header; the line
  above a `justfile` recipe, which `just --list` prints; the version behind
  a pinned GitHub action, which Dependabot reads; and a `///` inside a
  `clap` derive, which is the text of `--help`.
- **A Python docstring is not a comment.** It is a string that programs read
  through `__doc__` (the Blender scripts build their `--help` from it). The
  lint does not count it.
- **Rust items have no rustdoc text.** `cargo doc` shows names, signatures
  and types. The contract of a public item is its signature and its tests.
- **Copied files are out of scope.** A file that `skills/copied.json` lists
  under `copied_files` stays as its source wrote it.
- **A moved or renamed file starts at 0.** The baseline is keyed by path.
  Delete the file's comments in the change that moves it, or move its
  baseline entry by hand in the same change.
- **A rise in the baseline is a hand edit of `baseline.json`.** No command
  writes one, so a reviewer always sees it in the diff.
- **The ban is stricter than the fleet's `comment-sweep` step.** The keep
  list of the `comment-pruning` skill allows a public contract doc, a note
  on an outside program and a ticket link. The lint counts all 3, so `check`
  rejects them on new lines before the sweep runs.
- **`scripts/live/context.sh` keeps 15 step banners (`# ---- A2 ...`).**
  `crates/horch/tests/context_cli.rs` finds a step by its banner. They are
  the whole baseline after the first sweep, and issue #463 removes them.
