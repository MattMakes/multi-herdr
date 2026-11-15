# 01: Import the create-formal-verification-skill skill clean-room

<!-- horch-ticket: 1 -->
**Persona:** primary `skill-importer` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog`
**Own:** `skills/create-formal-verification-skill/`, `skills/copied.json`, `teammates/opus-verify.md`, `teammates/opus-hardening.md`, `crates/horch-core/tests/skills_catalog.rs`
**Do not touch:** `crates/horch-core/src/taskledger/`, `skills/skill-intake/`, `skills/patch-around-audit/`, `teammates/root-cause-auditor.md`, `docs/tickets/test-suite/`

## Goal

Import 1 third-party skill through the `skill-intake` pipeline. Install the
clean-room rewrite in this repo. Give it to 2 existing teammates.

## Source

- Repo: `https://github.com/cursor/plugins`
- Ref: `main`
- Path: `pstack/skills/create-formal-verification-skill`
- Start command: `intake run https://github.com/cursor/plugins --ref main --path pstack/skills/create-formal-verification-skill`

## Mapping answer (decided before the run)

The operator already answered the mapping question. Do not ask it again.

- Choose option 3: existing teammates only.
- Teammates: `opus-verify` and `opus-hardening`.
- Each teammate gets the skill in `available_skills`, not in `skills`.
- Do not create a new teammate.
- Pass this answer to `intake install --target repo --mapping ...` as
  `references/install.md` describes.

## Steps

1. Follow the `skill-intake` skill exactly. Run `intake mode` first.
   Check: the mode is `fleet`.
2. Run the pipeline through GATE 1, the clean-room stages, GATE 2 and install.
   Check: `intake verify` passes.
3. Install with the mapping answer above.
   Check: `skills/<installed name>/SKILL.md` exists.
   Check: `teammates/opus-verify.md` and `teammates/opus-hardening.md` list the skill under `available_skills`.
4. Run `cargo test -p horch-core --test skills_catalog`.
   Check: it exits 0.
5. Commit only the exact files that install wrote. Use explicit file paths.
   Do not use a directory pathspec. Other workers have uncommitted edits in
   this checkout.
6. Run `horch ledger finish`.

## Constraints

- The installed skill keeps no LICENSE or NOTICE file, no licence text and no
  upstream credit. This is personal use.
- If GATE 1 aborts (exit 4), install nothing and commit nothing. Report the
  report path with `horch ledger done`.
- If the installed skill name differs from `create-formal-verification-skill`,
  tell the orchestrator the new name in your report. The Own line names the
  expected folder only.
- Do not push, open a PR or release.
- Do not use `ANTHROPIC_API_KEY`.

## Done when

- The skill folder exists under `skills/` and `skills/copied.json` records it.
- `opus-verify` and `opus-hardening` list it in `available_skills`.
- `cargo test -p horch-core --test skills_catalog` exits 0.
- `horch ledger finish` passed every step.

## Report

Send the one-paragraph summary from `intake report` with `horch ledger done`.
Name every file you committed, the installed skill name, and any finding that
stays open.
