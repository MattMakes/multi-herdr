# 23: Add the sound-engineer teammate and wire the skills into the catalog

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test golden_prompts`
**Own:** `teammates/sound-engineer.md`, `skills/copied.json`, `crates/horch-core/tests/skills_catalog.rs`, `crates/horch-core/tests/golden_prompts.rs`, `crates/horch-core/tests/fixtures/`

## Goal

`horch spawn sound-engineer` starts a Claude Opus sound engineer for video SFX and game SFX that carries the hark and sound-sourcing skills, and the skill catalog accepts both skills.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Read `teammates/_template.md`, `teammates/video-audio.md` and `teammates/video-critic.md`.
2. Write `teammates/sound-engineer.md`: `agent: claude`, `model: opus`, `effort: high`, `skills: [hark, sound-sourcing]`, `available_skills: [video-audio, godot-sound-design]`, a brief_description of at most 120 characters ("Sound design for video and game SFX: forge, fit, source, mix and verify by listening with hark."), and a body that states the job: design or source each sound, aim it at picture or game events, vary repeated sounds, verify by measuring and listening with hark, and hand off a mix or an asset pack with a manifest.
3. Add `hark` and `sound-sourcing` to `skills/copied.json` with empty `copied_files` and `verbatim: false`. Add `budget_exempt: "clean-room engine package under engine/"` to `hark` only.
4. Run the check. Fix each expectation in the tests or fixtures that the new teammate or skills change, and only those.
5. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Done when

- The check passes. `horch teammates` lists sound-engineer.

