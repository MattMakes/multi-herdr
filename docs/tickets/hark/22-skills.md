# 22: Write the hark and sound-sourcing skills

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog`
**Own:** `skills/hark/SKILL.md`, `skills/hark/references/`, `skills/sound-sourcing/`

## Goal

2 skills teach an agent to use hark for video SFX and game SFX: `skills/hark/` (the tool: listen, measure, forge, fit, bank, kit, timeline, critique) and `skills/sound-sourcing/` (the real-recording workflow: brief, sourcing, usage-rights audit, arrange, mix, master, QA, variants, credits, mux).

**Side: clean.** Read only the specs and the repo.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Read `docs/specs/hark/skills-and-docs.md`, `docs/specs/hark/ARCHITECTURE.md`, and the CLI sections of the other specs.
2. Read how this repo writes skills: `skills/video-audio/SKILL.md`, `skills/godot-sound-design/SKILL.md`, and the limits in `crates/horch-core/tests/skills_catalog.rs` (SKILL.md under 12288 bytes).
3. Write `skills/hark/SKILL.md`: frontmatter `name: hark` and a trigger description; when to use each command; the video SFX workflow (link to the `video-audio` skill for the mix and beat grid); the game SFX workflow (link to `godot-sound-design`); variation rules for repeated SFX; how to call the launcher `skills/hark/scripts/hark` and how a project tool resolves it at run time; the optional ML install. Put detail in `skills/hark/references/`.
4. Write `skills/sound-sourcing/`: SKILL.md, references, scripts and templates as the spec describes. Scripts are Python 3.11+ with a `--self-test`. Role prompts become reference files in our own words.
5. Write no upstream names, credits, LICENSE or licence text. Usage rights of downloaded sounds are a feature, not a credit.
6. Run the check. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Done when

- Both SKILL.md files are under 12288 bytes. Each script's `--self-test` passes. The check passes, or the report names the catalog entry that ticket 23 must add.

