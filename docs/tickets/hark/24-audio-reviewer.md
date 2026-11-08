# 24: Add the audio-reviewer teammate on agy

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test golden_prompts`
**Own:** `teammates/audio-reviewer.md`, `skills/audio-review/`

## Goal

If `docs/specs/hark/agy-audio-probe.md` says HEARS, `horch spawn audio-reviewer` starts an agy (Gemini) teammate that listens to audio files and writes a scored audio review. If it says DOES-NOT-HEAR, add nothing.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Read `docs/specs/hark/agy-audio-probe.md`. If the verdict is DOES-NOT-HEAR, run `horch ledger done` with "no teammate: agy does not hear audio" and stop.
2. Read `teammates/spec-writer.md` and `teammates/antigravity.md` for the agy frontmatter, and `teammates/video-critic.md` for a reviewer.
3. Write `skills/audio-review/SKILL.md` and references: a blind review rubric for SFX and mixes (clarity at phone and laptop speakers, timing to picture or game event, loudness and peaks, masking, repetition fatigue, tonal fit, realness), a 1-5 score per axis, fix lines ranked BLOCKER or POLISH with the file to change, and the exact agy listening invocation of the probe. The reviewer also runs `hark measure` and `hark timeline` for numbers.
4. Write `teammates/audio-reviewer.md`: `agent: antigravity`, the model the probe found best, read-only review (it writes only its review file), skills [audio-review, hark], brief_description at most 120 characters.
5. Add `audio-review` to `skills/copied.json` (own text). Run the check and fix only the expectations the new teammate changes.
6. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Done when

- The check passes, and `horch teammates` lists audio-reviewer; or the report says the probe verdict was DOES-NOT-HEAR.

