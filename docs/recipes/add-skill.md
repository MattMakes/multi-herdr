# Recipe: add a bundled skill

## When

You want a new skill to ship inside `horch`, so a worker can load it in any
harness that exposes skills.

## Before you start

- `skills/README.md`: what the bundles are and the "Deliberate adaptations"
  rules.
- `ai_docs/plans/design-skills/01-skill-authoring.md`: shape, size budget,
  section order, and how to make an upstream skill our own.
- [phase-skills.md](../phase-skills.md): how a phase selects skills and how
  each harness sees them.

In the steps, `<id>` is the skill id, for example `ui-taste`.

## Steps

1. Create `skills/<id>/SKILL.md` with YAML frontmatter `name: <id>` and
   `description:` ("Use when ...", at most 300 characters). Put deep material
   in `skills/<id>/references/*.md`. Only `.md` and `.txt` files; `SKILL.md`
   at most 12 KiB; the whole directory at most 160 KiB. Check:
   `skills_bundled_size_budget` and `skills_bundled_text_only` in
   `crates/horch-core/tests/skills_catalog.rs`.
2. Do not register it in Rust. `crates/horch-core/build.rs` embeds every file
   under `skills/` on the next build. Check:
   `cargo build --workspace --bins && ./target/debug/horch skills show <id>`
   prints the id, a `bundled+` version and the description.
3. Add an entry to `skills/provenance.json`, in alphabetical order by name:
   `name`, `sources` (1 item per adapted file: `repository`, `revision`,
   `path`, `sha256`, `license`) and `adaptation`. A skill written here has
   `"sources": []`. Check: `skl_01_bundled_catalog_versions_and_digests`.
   It fails when the entry is missing, when a source is not at a pinned
   revision (`PINNED_REPOSITORY`/`PINNED_COMMIT` or `DESIGN_SOURCE_PINS`),
   and when a skill with no sources is not in `REPO_ORIGINAL`. A new upstream
   or a new repo-original skill therefore also needs a line in
   `crates/horch-core/tests/skills_catalog.rs`; ask before you edit it.
4. Add a row to `skills/README.md`, in alphabetical order: "Source mapping"
   for a process skill, "Design skills" for a design skill. Check: read it.
5. Attach the skill. Choose 1:
   - By name, to specific teammates: add `<id>` to their `skills:` list.
     This is how the design skills and `orchestrate` attach. Check:
     `./target/debug/horch teammates --check`.
   - To a phase, for every teammate in that phase: add `<id>` to
     `phase_skills` in `crates/horch-core/src/skills/selection.rs`, and
     update the table in `docs/phase-skills.md`. Check:
     `./target/debug/horch skills --phase <phase> --json` lists it.
6. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Files this recipe touches

| file | change | required or optional |
|---|---|---|
| `skills/<id>/SKILL.md` | the skill | required |
| `skills/<id>/references/*.md` | deep material | optional |
| `skills/provenance.json` | 1 entry | required |
| `skills/README.md` | 1 row | required |
| `teammates/<name>.md` | `skills:` entry | when attached by name |
| `crates/horch-core/src/skills/selection.rs` | `phase_skills` | when attached to a phase |
| `docs/phase-skills.md` | phase table | when attached to a phase |
| `crates/horch-core/tests/skills_catalog.rs` | `DESIGN_SOURCE_PINS` or `REPO_ORIGINAL` | when a new upstream or a repo-original skill |

## Tests and oracles

- `crates/horch-core/tests/skills_catalog.rs` checks the catalog against the
  directories, the digests, provenance and the size budget.
- `horch skills` and `horch skills --json` oracles
  (`crates/horch/tests/oracles/skills/skills.txt`, `skills-json.txt`) do not
  change for a new skill: `oracle_cli_skills_match` in
  `crates/horch/tests/baseline_cli.rs` compares only the A0 skills. They
  change when you edit an A0 skill (for example `orchestrate` changes
  `skill_file_bytes` in `skills-json.txt`).
- A change to `phase_skills` changes the phase oracles
  `crates/horch/tests/oracles/skills/skills-phase-*.txt` and the per-teammate
  briefings `crates/horch-core/tests/oracles/skills/<teammate>-<phase>.txt`.
- Attaching a skill by name to an A0 teammate changes its oracles
  `crates/horch-core/tests/oracles/skills/<teammate>-*.txt` and
  `crates/horch-core/tests/oracles/launch/<teammate>.json`. Attaching it to a
  teammate in `SKIP_NEW_TEAMMATES` changes no oracle.
- Bless only what your plan names, 1 test at a time (`oracle_skills_match`,
  then `oracle_launch_matches`), and check `git status` (see
  [testing-and-gates.md](../testing-and-gates.md#horch_bless)).

## Gotchas

- A skill must work in every harness that exposes skills. Say "a browser
  tool, if available", not a tool name of one CLI.
- Never tell the worker to start a subagent or a nested agent CLI.
- Antigravity exposes no skills. A teammate on `antigravity` that names a
  skill is refused at spawn.

## Worked example

`git show --stat e4b0262` (D01) added `ui-taste` and `ui-redesign`: 2 skill
directories, 1 README row each, and their provenance entries. No Rust file
changed. `git show --stat 9d7fa5a` (D00) added multi-source provenance to
`crates/horch-core/src/skills/catalog.rs`.
