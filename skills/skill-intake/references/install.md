# Install

## Target

`intake install --task T` picks the target by the mode rule, and its output
names the rule that decided. **repo** (multi-herdr mode): `R` =
`git rev-parse --show-toplevel`, and `R/skills/copied.json`,
`R/teammates/_template.md` and `R/crates/horch-core/tests/skills_catalog.rs`
are all regular files. A worktree of that repo counts. **standalone**:
anything else. `--target repo|standalone` overrides the rule. Pass the same
target to `intake cr context` and `intake verify`.

## Target rules

The writer gets these rules in `K/context.json`. `intake verify` checks them.

| Rule | repo | standalone |
|---|---|---|
| Files | `.md` and `.txt` only; each script is a fenced code block in `references/script-<name>.md` | real script files allowed |
| Size | `SKILL.md` at most 12 KB (fails GATE 2); folder at most 160 KB, else a `budget_exempt` reason | the same, as guidance |
| Licence and notice files | none | none |
| Dotfiles | none | none (install writes `.intake-provenance.json`) |
| Frontmatter | `name` and `description` only | the same |
| Interpreters | `node` 18+, `python3` 3.10+, POSIX `sh`; standard library only | the same |
| Fleet rules | no subagents, no human approval gate (a message to the orchestrator instead), no machine paths, no `${CLAUDE_*}` variables, no runtime downloads, no dependence on another skill or an external script | no runtime downloads, no machine paths |

A repo skill over 160 KB after its detail moved to `references/`: pass
`intake verify --budget-exempt <skill>=<reason>`. Install writes the reason
into `copied.json`, and the report flags it.

## Declared capabilities

A re-sweep hit on the output is accepted only when all 3 hold:

1. a handed-off spec declares the access in `capabilities` (`network`,
   `reads`, `writes`, `runs`);
2. the reviewer gave `capability:<key>` a `pass` verdict for that spec;
3. the output `SKILL.md` has a section headed `What this skill touches`
   that tells the agent about the access in plain words.

The report lists every accepted capability. The rules of
[the threat model](threat-model.md#never-acceptable-in-output) are never
accepted.

## Standalone

Target: `~/.claude/skills/<name>/`, 1 folder per skill.

- Never replace an existing skill automatically. An existing name gets
  `<name>-<owner>`, then `-2`, `-3`. Install renames the `name` field and every
  self-reference. The report lists each rename.
- `--resolve '{"<skill>": "<new-name>" | "SKIP" | "REPLACE"}'` is used only
  when the invoker passed it. `REPLACE` moves the old folder to
  `C/replaced/<name>/`.
- Install is atomic per skill: a temp folder, a sha256 check against
  `verify.json`, then a rename. It writes `.intake-provenance.json` and appends
  1 line to `installed.jsonl` in the workspace base.
- Exit 7: a collision no rule resolves. Report it.

## Repo

Work on a branch, never on the default branch, and never push. `install`
exits 15 on the default branch and writes nothing. Before a repo install,
make a branch: in a ledger run use your ledger worktree; outside one, run
`horch worktree new intake-<first skill>` and install in the path it prints.

### Mapping answer

The answer goes in a JSON file that you pass with `--mapping FILE`:

```json
{"option": 1, "teammate": "pdf-specialist", "brief_description": "Edits and checks PDF files.",
 "offer_when": ["*.pdf"], "requires": [], "existing": [], "field": "available_skills"}
```

- `option` 1: a new teammate. `teammate` and `brief_description` (at most
  120 characters, 1 line) are needed. `existing` stays empty.
- `option` 2: a new teammate, and `existing` teammates also get the skills.
- `option` 3: `existing` teammates only. No `teammate` and no
  `brief_description`.
- `offer_when` and `requires` are optional. Set them only when the skills
  clearly need a project marker or a tool. `requires` takes `xcode`,
  `blender`, `godot` or `git-lfs`.
- `field` is `available_skills` (the default) or `skills`.

Install fills the new teammate's frontmatter. The persona body is
`K/persona.md` from the writer. A name that collides gets `-2`.

### Steps

1. `intake install --task T --target repo [--mapping FILE]`. Without
   `--mapping` it writes only the answer-independent set: the skills,
   `skills/copied.json`, the skills README section, `docs/cleanroom/<name>/`
   and `REPO_ORIGINAL` in `skills_catalog.rs`. With `--mapping` it also writes
   the option's files: the new teammate, `SKIP_NEW_TEAMMATES` entries, edits
   of existing teammates, `teammates/README.md`, `docs/skills-and-teams.md`
   and `CHANGELOG.md`. A run with a different mapping removes the files that
   an earlier run of the same task created.
2. Options 2 and 3 change the frozen oracles of each existing teammate that
   gets the skills. Then:
   1. Run `HORCH_BLESS=1 cargo test -p horch-core --test baseline_oracles --test skills_catalog`.
   2. Run `git status --porcelain`. Confirm that only the oracle files of the
      edited teammates changed. If any other oracle file changed, stop and
      report. Do not commit.
   3. Run both tests again without `HORCH_BLESS`. Both pass.
3. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.
   Options 1 and 2 add a Claude teammate, so some count tests fail. Update
   exactly the count tests that fail with `left: N, right: N+1`. A count
   test that fails in any other way is a real failure. Run the gate again
   until it is green. In a ledger run, `horch ledger finish` runs the run's
   own check.
4. Commit the exact files that install wrote and the count tests you
   changed.
5. `intake report --task T --gate-result "<the gate result>"`. The report
   records the repo gate only when you pass it.

### No answer yet

If the mapping answer has not arrived at step 1: run step 1 without
`--mapping`, commit those files, record
`horch ledger progress "waiting: mapping answer"`, and end your turn. When the
answer arrives, write the mapping file and run the steps again with
`--mapping`.
