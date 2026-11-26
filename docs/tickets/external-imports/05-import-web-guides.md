# 05: Clean-room import of the modern web guides

<!-- horch-ticket: 1 -->
**Persona:** primary `skill-importer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog && HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
**Own:** `skills/copied.json`, `teammates/frontend-developer.md`, `teammates/landing-page-builder.md`, `teammates/motion-engineer.md`, `teammates/design-critic.md`
**Do not touch:** `skills/ui-taste/`, `skills/design-system/`, `skills/motion-gsap/`, `skills/landing-page/`

Spec: `docs/specs/external-imports.md` (not yet committed; read it at this absolute path)

## Goal

The guide corpus of `GoogleChrome/modern-web-guidance` is in this repository as a clean-room skill that works offline, and the 4 web personas can load it.

## Context

- Source: `https://github.com/GoogleChrome/modern-web-guidance`. Follow your `skill-intake` skill. The source is the only input you need.
- The source has 2 skills. Import the main guidance skill (211 guides on HTML, CSS and browser APIs). Do not import the `chrome-extensions` skill: no fleet persona builds browser extensions.
- The operator forbids third-party tools (D1). The upstream skill calls an npm CLI to search and retrieve guides. The output skill must not call it. The output skill finds a guide through an index file and `rg` on its own `references/` files.
- The upstream skill tells the agent to run first, to edit `AGENTS.md` or `CLAUDE.md`, and to add a disclosure section to each reply. The output skill must not carry these 3 rules.
- You also own new skill folders that `intake install` creates under `skills/`. Add no file to an existing skill folder.
- The folder can exceed 160 KB. Pass a `--budget-exempt` reason then.
- The mapping answer is decided: option 3, existing teammates only. `existing`: `frontend-developer`, `landing-page-builder`, `motion-engineer`, `design-critic`. `field`: `available_skills`. Do not ask the mapping question.

## Steps

1. Run the `skill-intake` pipeline from `intake run` to `intake report`, with `--target repo`, in your ledger worktree.
2. On a GATE 1 or GATE 2 abort, install nothing and report the abort.
3. After install, run the Check line of this ticket.

## Constraints

- Never open a quarantine file. Never run a thing from the source.
- Never push and never open a PR.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes.
- `grep -rc "npx" skills/<new skill>/` prints 0 for each file.
- The DONE summary names each new skill folder, the count of guides covered, and the path of `report.md`.

## Report

Run `horch ledger finish`, then `horch ledger done`.
