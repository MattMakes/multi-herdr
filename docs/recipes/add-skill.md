# Recipe: add a bundled skill

## When

You want a new skill to ship inside `horch`, so a worker can load it in any
harness that exposes skills.

## Before you start

- `skills/README.md`: what the bundles are and the "Deliberate adaptations"
  rules.
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
3. Add an entry to `skills/copied.json`, in alphabetical order by name:
   `name`, `copied_files` (the copied files, relative to `skills/<id>/`) and
   `verbatim` (`false` unless the directory is an unchanged copy). A skill
   written here has `"copied_files": []`. Check:
   `skl_01_bundled_catalog_versions_and_digests`. It fails when the entry is
   missing, when a listed file does not exist, and when a skill with no
   copied files is not in `REPO_ORIGINAL`. A new own-text skill therefore
   also needs its id in `REPO_ORIGINAL` in
   `crates/horch-core/tests/skills_catalog.rs`; ask before you edit it.
   A skill over the size budget in step 1 that is not verbatim (renamed,
   adapted or combined) states why in `"budget_exempt": "<reason>"`; see the
   `format` string at the top of `skills/copied.json`. Set it only on a skill
   over the budget, because a test fails a stale reason.
   To copy a skill verbatim instead, see
   [Copy a skill verbatim](#copy-a-skill-verbatim).
4. Add a row to `skills/README.md`, in alphabetical order: "Process skills"
   for a process skill, "Design skills" for a design skill. The second column
   is the kind (see the list at the top of that file). Check: read it.
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
| `skills/copied.json` | 1 entry | required |
| `skills/README.md` | 1 row | required |
| `teammates/<name>.md` | `skills:` entry | when attached by name |
| `crates/horch-core/src/skills/selection.rs` | `phase_skills` | when attached to a phase |
| `docs/phase-skills.md` | phase table | when attached to a phase |
| `crates/horch-core/tests/skills_catalog.rs` | `REPO_ORIGINAL` | when an own-text skill |

## Copy a skill verbatim

A verbatim skill is an unchanged copy. Use it when a skill is good as it is
(the 30 verbatim `ue-*` skills are an example).

1. Copy the skill directory into `skills/<id>/`. Leave out dotfiles and any
   licence or notice file. The directory name must equal `name:` in
   `SKILL.md`.
2. Add the `copied.json` entry with `"verbatim": true` and every copied file
   in `copied_files`. That flag exempts the skill from the 12 KB and 160 KB
   size budget. It does not exempt it from the text-only rule.
3. Check: `cargo test -p horch-core --test skills_catalog` and
   `horch skills show <id>` prints `verbatim:     true`.
4. To refresh it, follow "Refresh a copied skill" in
   [skills-and-teams.md](../skills-and-teams.md#refresh-a-copied-skill).

## Tests and oracles

- `crates/horch-core/tests/skills_catalog.rs` checks the catalog against the
  directories, the digests, `copied.json` and the size budget.
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
directories and 1 README row each. No Rust file changed. G10 replaced the
skill provenance file with `skills/copied.json`.
