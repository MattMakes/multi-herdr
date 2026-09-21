# Bake the herdr orchestration instructions into this repo

GOAL: A fleet boots from this repo alone. The orchestrator briefing, the worker
briefing, and the orchestration playbook all ship inside `teammates/` and
`skills/`. No `~/.claude/skills` entry, no external plugin, and no
`matts-robot-skills` checkout is needed or consulted at boot. The stale
external skills can no longer mislead a fleet pane.

## CONTEXT

### What is already true (do not hunt for a dependency that is not there)

- `horch fleet` builds the orchestrator prompt from
  `teammates/_base/fleet-orchestrator.md` plus the persona in
  `teammates/orchestrator.md` (or `orchestrator-codex.md`). Nothing in
  `crates/` writes `.herdr-orchestrator/ORCHESTRATOR.md` any more. The
  directory `.herdr-orchestrator/` at the repo root is a leftover from
  2026-09-11. Do NOT delete it. Do NOT reference it.
- Worker prompts come from `teammates/_base/fleet-worker.md` plus each
  teammate's persona. Nothing in `crates/` references `herdr-worker` except
  one test string at `crates/horch-core/src/prompts.rs:240-242`, which is an
  arbitrary name in a test and needs no change.
- Bundled skills live in `skills/<name>/SKILL.md`. `crates/horch-core/build.rs`
  collects every file under `skills/` into `BUNDLED_SKILL_FILES` at build
  time. A new directory registers itself. `skills/provenance.json` is not
  parsed by Rust; it is documentation.
- Phase catalogs are the static lists at `crates/horch-core/src/skills.rs:25-27`.
  A teammate's `skills:` frontmatter list adds named bundled skills on top of
  its phase catalog. `crate::skills::selected(&Teammate)` at `skills.rs:75`
  resolves the final list.
- `disabled_skills:` in teammate frontmatter becomes a `skillOverrides`
  entry with value `off` in the Claude settings overlay. See
  `crates/horch-core/src/launch.rs:345-370` (`overlay_skill_switches`) and
  the tests at `crates/horch-core/src/skills.rs:430-470`.

### What is actually missing

1. The operator's machine has two symlinks:
   `~/.claude/skills/herdr-orchestrator` and `~/.claude/skills/herdr-worker`,
   both pointing into `/Users/mascott/projects/matts-robot-skills/plugins/herdr/skills/`.
   Claude Code loads every `~/.claude/skills` entry into every fleet pane. The
   skill descriptions trigger on the words "orchestrator", "herdr", "horch",
   so a pane invokes them. Their content is STALE for this horch. The
   orchestrator in this very run followed the skill, ran `horch status`, and
   got "unrecognized subcommand". The orchestrator briefing must be complete
   on its own, and the stale skills must be switched off by name.
2. The useful playbook content exists only in those external files: the core
   loop, the briefing-file template, the shape of a good BLOCKED message and
   a good DONE summary, scope discipline, and the "things that go wrong"
   checklists. That content must move into this repo, reconciled against the
   real horch CLI.

### Known-wrong claims in the external skills (verified in this run)

These commands, flags, and claims do NOT exist in this horch. Never copy them:

- `horch status`, `horch board`, `horch brief new`, `horch retire`,
  `horch task add`, `horch task set`, `horch tiers`, `horch ready`,
  `horch blocked`, `horch whoami`
- flags `--brief`, `--effort`, `--task` on `horch spawn` or `horch assign`
- the claim that `horch assign` rejects inline task text (it takes `<TASK>...`)
- `$HORCH_ROLE` as the role source
- a `haiku` tier, a `fable` tier that workers can spawn
- "the orchestrator closes your pane" (here `horch done` closes the worker's
  own pane)

The real CLI is in `horch --help`, `horch spawn --help`, `horch assign --help`,
and the two `_base` files. Trust those.

## FILES

Own (you may edit):
- `teammates/_base/fleet-orchestrator.md`
- `teammates/_base/fleet-worker.md`
- `teammates/orchestrator.md`
- `teammates/orchestrator-codex.md`
- `teammates/opus.md`, `teammates/sonnet.md`, and the 8 Claude specialist
  files: `architect-reviewer.md`, `backend-developer.md`, `designer.md`,
  `frontend-developer.md`, `product-lead.md`, `qa-engineer.md`,
  `researcher.md`, `staff-engineer.md` (frontmatter only, see step 5)
- `teammates/_template.md` (field docs only)
- `teammates/README.md`
- `skills/orchestrate/SKILL.md` (new)
- `skills/README.md`, `skills/provenance.json`
- `crates/horch-core/src/skills.rs` (tests only, unless step 3 finds a
  hard requirement; then say so in a `horch note` before editing logic)
- `crates/horch-core/tests/golden_prompts.rs` and `crates/horch-core/tests/golden/*`
- `README.md`, `docs/phase-skills.md`
- `ai_docs/reports/bake-in-orchestration-inventory.md` (new)

Do NOT touch:
- `teammates/orchestration-orchestrator.md`, `teammates/orchestration-worker.md`
  and their goldens `orchestration-orchestrator.txt`, `orchestration-worker.txt`.
  They belong to the fixed 5-pane recipe and reference no external skill.
- `teammates/_base/codex-execpolicy.md`, `teammates/_base/codex-orchestrator-execpolicy.md`
- `crates/horch-core/src/launch.rs`, `teammates.rs`, `prompts.rs`
- anything under `~/.claude/`, `~/.codex/`, or `matts-robot-skills`. Read
  the external skills; never edit or delete them.
- `.herdr-orchestrator/`

## STEPS

Do the steps in order. Run `horch note "..."` after each numbered step.

### Step 1. Inventory and reconcile (gate; no repo edits yet)

Read in full:
- `~/.claude/skills/herdr-orchestrator/SKILL.md`
- `~/.claude/skills/herdr-orchestrator/references/layout.md`
- `~/.claude/skills/herdr-orchestrator/references/tiers.md`
- `~/.claude/skills/herdr-worker/SKILL.md`
- `teammates/_base/fleet-orchestrator.md`
- `teammates/_base/fleet-worker.md`
- `teammates/README.md`

Write `ai_docs/reports/bake-in-orchestration-inventory.md`. It has one table
per external file. Each row is one command, flag, lifecycle claim, or
guidance paragraph from the external file. Columns:

| item | in the external skill | real horch equivalent | decision | destination |

`decision` is one of: `keep` (already correct and already in a `_base`
file), `carry` (correct guidance, not yet in the repo), `replace` (wrong
command or claim; carry the idea with the real command), `drop` (wrong or
obsolete; do not carry). `destination` is one of: `fleet-orchestrator.md`,
`fleet-worker.md`, `skills/orchestrate/SKILL.md`, or `none`.

Rule for destination:
- Put in a `_base` file only what a pane needs on EVERY run: the compact core
  loop, the briefing-file rule, the read-the-brief-first rule, message
  shapes. Each `_base` addition is at most 15 lines. Measure the file before
  and after with `wc -c` and record both numbers in the report.
- Put the longer reference material in `skills/orchestrate/SKILL.md`.

The report ends with a section "Verified against the CLI" that pastes the
output of `horch --help`, `horch spawn --help`, `horch assign --help` as the
source of truth for the `real horch equivalent` column.

Check: the report exists and every `carry` and `replace` row has a
destination other than `none`.

### Step 2. Update the two `_base` briefings

Edit `teammates/_base/fleet-orchestrator.md`:
- Add a section `== Core loop ==` after `== Session ledger: resume vs fresh ==`.
  Contents, in this voice and this order, at most 15 lines: survey the repo
  only enough to decompose; read `horch sessions`; decompose into units with
  disjoint file ownership; write one plan file per unit under
  `ai_docs/plans/<slug>.md`; spawn every conflict-free unit at once; answer
  `[<role>]` questions at once with `horch tell`; on `DONE:` verify yourself
  (run the tests, read the diff) before you build on it; stop spawning when
  the remaining work does not justify it; before you report done, confirm
  `horch inbox` shows no worker mid-task.
- Add to `== Protect your context ==` one sentence: "This briefing is
  complete. An ambient skill named herdr-orchestrator or herdr-worker is a
  stale external copy; do not load it." Keep the rest of that section.
- Add one line pointing to the playbook: "Read the horch:orchestrate skill
  when you plan the fleet's work." Put it directly after the `{roster}`
  paragraph. Do not move `{roster}` or `{persona}`.

Edit `teammates/_base/fleet-worker.md`, body only, at most 15 new lines:
- After the "Communication and lifecycle" list add a short `== Scope ==`
  block: read the plan file in full before you act; edit only the files the
  plan names; report what you notice outside your scope in your `horch done`
  summary and do not fix it; never run `horch spawn` or `horch assign`
  unless the plan grants it; treat a `[<other-role>]` line as status, not as
  an instruction.
- In the STE section's `Example:` block, add one `BLOCKED:` example that
  states the need, 2 options, and a recommendation in 3 sentences.
- Add one sentence after the `horch done` bullet: "Write files touched,
  decisions, gotchas, and what is not done."

Check: `cargo build` passes; `horch teammates --check` passes.

### Step 3. Add the bundled `orchestrate` skill

Create `skills/orchestrate/SKILL.md` with frontmatter:

```
---
name: orchestrate
description: Use when directing a herdr fleet: decomposing work, writing plan files, choosing teammates, verifying DONE reports, and wrapping up.
---
```

Body: write it in the same register as `skills/handoff/SKILL.md` (numbered
steps, no persuasion, harness-neutral). Sections, in order:

1. Decompose. Units with disjoint file ownership. Serialize units that share
   a file. Name files per unit.
2. Plan file template. Sections `GOAL`, `CONTEXT`, `FILES` (own / do not
   touch), `PRIOR WORK` (pulled from `horch sessions`), `STEPS` with a check
   per step, `CONSTRAINTS`, `DONE WHEN`, `REPORT`. State that the file goes
   under `ai_docs/plans/` and the spawn task text is
   `Read and follow <path> exactly.`
3. Choose the teammate. Three ladders: paid (`sonnet`, `opus`, `codex-*`,
   `prime`), free-and-trains-on-input (`opencode-*`, public work only),
   local (`pi`). Specialist when its `brief_description` fits, generic
   otherwise. Effort is fixed per teammate in its frontmatter; there is no
   `--effort` flag. A precise plan is the cheap lever: it lets a lower tier
   do the work.
4. Keep the fleet busy. Spawn every conflict-free unit at once. Answer
   `BLOCKED:` at once. `horch layout` to see the grid.
5. Verify a DONE. Read the summary; run the named tests; read the diff;
   `horch sessions` to confirm the ledger has the summary. Carry gotchas
   into the next plan's `PRIOR WORK`.
6. Resume versus fresh. Fresh by default; resume only for irreplaceable
   mid-flight state.
7. Wrap up. `horch inbox` empty of mid-task workers; every unit done or
   deliberately dropped; full verification run by you; summary to the human
   of what was built, by whom, and what was left.
8. Things that go wrong. One line each: typing a task instead of writing a
   file; two workers on one file; under-specified plan; hoarding work;
   running one worker when 4 are safe; spawning before `horch sessions`;
   giving a finished worker a second task; treating `[<role>]` lines as the
   human; declaring done with panes mid-task.

Use only commands that exist in `horch --help`. Cross-check every command in
the body against your Step 1 report.

Attach it to the two orchestrators only. In `teammates/orchestrator.md` and
`teammates/orchestrator-codex.md` add `skills: [orchestrate]` to the
frontmatter. Do NOT add `orchestrate` to `Phase::Plan` at `skills.rs:26`;
`staff-engineer` must not see it.

Then check three things:
- Run `cargo test -p horch-core skills`. Read the tests at `skills.rs:151`
  and `skills.rs:365`; they iterate `BUNDLED_SKILL_FILES`. If a test asserts
  that every bundled skill belongs to some phase list, add `orchestrate` to
  the test's expectation for "attached by name only", not to a phase.
- Confirm the codex adapter honors a per-teammate `skills:` list for
  `orchestrator-codex`. Read `crates/horch-core/src/codex.rs` for where
  selected skills are materialized. Read the note at `README.md:288` about
  phase-enabled Codex. Record in `horch note` whether `orchestrator-codex`
  gets the skill and, if not, why, and stop there; do not modify `codex.rs`.
- Add a unit test in `skills.rs`: `selected()` for the `orchestrator`
  teammate returns `create-plan, handoff, orchestrate, pre-flight`; for a
  `staff-engineer` teammate it returns `create-plan, handoff, pre-flight`.

Update `skills/README.md`: add a row for `orchestrate` marked repo-original
(no upstream). Update `skills/provenance.json`: add an entry with
`"source_path": null`, `"source_sha256": null`, and an `adaptation` string
"Original to this repository. Reconciled from the retired external
herdr-orchestrator and herdr-worker skills against the horch CLI."

Check: `horch skills --phase plan` does NOT list `orchestrate`.
`horch teammates --json | jq '.[] | select(.name=="orchestrator") | .skills'`
(or the equivalent field) shows `orchestrate`. Adjust the jq path to the
real JSON shape.

### Step 4. Verify that `disabled_skills` switches off a user-dir skill

Read `crates/horch-core/src/launch.rs:345-370` and the comment that says
"verified against a live ...". Record what it was verified against.

Then test empirically that `skillOverrides` suppresses a `~/.claude/skills`
symlinked skill, not only a plugin skill. Run, from the repo root:

```
claude --settings '{"skillOverrides":{"herdr-orchestrator":"off","herdr-worker":"off"}}' -p 'List the names of every skill available to you, one per line, nothing else.' --max-turns 1
```

and the same command without `--settings`. Compare the two lists. Paste both
outputs into `ai_docs/reports/bake-in-orchestration-inventory.md` under a
section "skillOverrides check". If the flag format differs on this Claude
Code version, read `claude --help` and adapt; note the exact command you ran.

Outcome A, the names disappear with the override: continue to Step 5.

Outcome B, they do not disappear: do NOT add `disabled_skills` anywhere.
Send `horch tell orchestrator "[<role>] NOTE: skillOverrides does not hide
user-dir skills. Evidence in the inventory report. I skip Step 5 and rely
on the base-prompt guard from Step 2."` Then continue with Step 6.

### Step 5. Switch the stale skills off by name (only after Outcome A)

Add to the frontmatter of `teammates/orchestrator.md`, `teammates/opus.md`,
`teammates/sonnet.md`, and the 8 Claude specialist files:

```
disabled_skills: [herdr-orchestrator, herdr-worker]
```

Not `orchestrator-codex.md` and not any `codex-*`, `opencode-*`, `pi`,
`prime`, or `smoke` file; `horch teammates --check` rejects Claude-only
fields elsewhere. Add a one-line comment above the field in each file:
`# Stale external copies of the fleet briefing; the repo carries the real one.`

Update the `disabled_skills` doc lines in `teammates/_template.md` to name
this use.

Check: `horch teammates --check` passes. `horch teammates --json` shows the
two names under each of the 11 teammates.

### Step 6. Goldens and measurement

Read `crates/horch-core/tests/golden_prompts.rs:28` (`fn golden`). If there
is no refresh path, add one: when the environment variable
`HORCH_UPDATE_GOLDEN=1` is set, write the rendered prompt to the golden
file instead of asserting. Then run
`HORCH_UPDATE_GOLDEN=1 cargo test -p horch-core --test golden_prompts`,
then run it again WITHOUT the variable and confirm it passes.

Read the diff of every changed golden with `git diff crates/horch-core/tests/golden`.
Confirm each diff contains only the lines you added in Step 2 and the
`horch:orchestrate` catalog line for the orchestrator. If a golden changed
in any other way, stop and `horch tell orchestrator` with the unexpected
diff.

Record before and after in the inventory report:
- `wc -c teammates/_base/fleet-orchestrator.md teammates/_base/fleet-worker.md`
- `wc -c crates/horch-core/tests/golden/fleet-orchestrator.txt crates/horch-core/tests/golden/worker-opus-task.txt`
- `horch skills --phase plan --json | wc -c`
- the `Metadata:` line from `horch skills --phase plan`

Compare with the size of `~/.claude/skills/herdr-orchestrator/SKILL.md`
(228 lines) and `~/.claude/skills/herdr-worker/SKILL.md` (115 lines), which
every Claude pane previously loaded on trigger. State the net change in the
PR body.

### Step 7. Docs

- `README.md`, the section that starts near line 150 ("A fresh `claude`
  inherits every globally-enabled plugin..."): add a short paragraph
  "Booting without external plugins": the fleet briefings and the
  `orchestrate` playbook ship in `teammates/` and `skills/`; no
  `~/.claude/skills` entry and no plugin is required; the operator may
  remove the old `herdr-orchestrator` and `herdr-worker` symlinks from
  `~/.claude/skills`; if they stay, the fleet's Claude teammates switch them
  off by name (only if Step 5 happened; otherwise say the base briefing
  tells the pane to ignore them).
- `teammates/README.md`: in "The two orchestrators", one paragraph on
  `skills: [orchestrate]` and why it is attached by name, not by phase.
- `docs/phase-skills.md`: after the phase table at line 8, one line:
  `orchestrate` is attached to the two orchestrators by name and belongs to
  no phase. Update the measurement table if the doc has one for the plan
  phase.
- `ai_docs/teams.md`: search for `herdr-orchestrator`, `herdr-worker`, and
  `matts-robot-skills`. Remove or rewrite any sentence that says the plugin
  is needed at boot. Leave everything else.

### Step 8. Full verification, commit, PR

Run all of these and paste the results in the PR body:

```
cargo build --release --bin horch
cargo test --workspace
horch teammates --check
horch skills --phase plan
horch skills --phase implementation
horch smoke fleet
rustfmt --check teammates 2>/dev/null; cargo fmt --check -p horch-core -- crates/horch-core/src/skills.rs crates/horch-core/tests/golden_prompts.rs
```

`cargo fmt --check` on other files has pre-existing diffs; check only the
files you touched.

Then, with the release binary:

```
HORCH_TEAMMATES_DIR=$PWD/teammates target/release/horch teammates
HORCH_TEAMMATES_DIR=$PWD/teammates target/release/horch teammates --check
```

Branch: `bake-in-orchestration` from `main`. Commit with a message whose
first line is `Bake the fleet orchestration playbook into teammates/ and skills/`.
Push. Open a PR against `main` with `gh pr create`. The PR body has: the
list of files changed, the Step 4 outcome (A or B) with the evidence, the
Step 6 measurements, and the test output.

## CONSTRAINTS

- Write every new prompt line in the voice already used in the `_base`
  files: short declarative sentences, one instruction per line, no hedges.
- Use only commands that exist in `horch --help`. If you are unsure whether
  a command exists, run `horch <cmd> --help` and use what it prints.
- Do not change `Phase` catalogs in `skills.rs:25-27`.
- Do not change any logic in `launch.rs`, `teammates.rs`, `codex.rs`,
  `prompts.rs`. If a step cannot complete without such a change, stop and
  `horch tell orchestrator "[<role>] BLOCKED: ..."` with the reason.
- Do not delete, edit, or move anything under `~/.claude` or in
  `matts-robot-skills`.
- Keep `.herdr-orchestrator/` untracked and untouched.
- Do not start a fleet or spawn workers.

## DONE WHEN

- `ai_docs/reports/bake-in-orchestration-inventory.md` exists with the
  reconciliation tables, the CLI source-of-truth section, the
  `skillOverrides check` section, and the measurements.
- `skills/orchestrate/SKILL.md` exists, is attached via `skills:` on
  `orchestrator.md` and `orchestrator-codex.md`, and is absent from
  `horch skills --phase plan`.
- `teammates/_base/fleet-orchestrator.md` has the `== Core loop ==` section,
  the stale-skill guard sentence, and the playbook pointer.
- `teammates/_base/fleet-worker.md` has the `== Scope ==` block, the
  `BLOCKED:` example, and the DONE summary sentence.
- Step 5 done (Outcome A) or explicitly skipped with evidence (Outcome B).
- `cargo test --workspace` passes. `horch teammates --check` passes.
  `horch smoke fleet` passes. Goldens are refreshed and their diffs reviewed.
- Docs in Step 7 updated.
- PR open against `main`, body complete.

## REPORT

- `horch note "..."` after each step, with the step number.
- `horch tell orchestrator "[<role>] BLOCKED: ..."` if stuck, then wait.
- `horch done "..."` at the end. The summary names the PR URL, the branch,
  the commit, the Step 4 outcome, the measurements, the files changed, and
  anything you noticed outside scope and left alone.
