# 05: Audio skill: captions-only long films and chapter music beds

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** none
**Own:** `skills/video-audio/SKILL.md`, `skills/video-audio/references/`

## Goal

`skills/video-audio/SKILL.md` tells the audio worker what changes for `meta.format: long`.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. In "Steps" near line 23, add 1 line: read `meta.format` first.
2. In "1. Voice-over", state that a `long` film has no voice-over by default. Skip the voice-over and timing reconciliation when no scene has narration.
3. In "3. Beat grid" and "5. Ducking and master", add the `long` rules: 1 music bed for each chapter or group of chapters from `meta.chapters`; crossfade beds at a chapter start on a beat; never loop 1 song for the whole film; the end screen (`meta.end_screen_sec`) keeps a music tail.
4. Note that 2-pass `loudnorm` is required for long films (the existing note near line 201 of `references/lavfi-sfx-recipes.md` covers short clips).
5. Read the file again. Confirm every `short`-only rule says `short`.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- `grep -c 'meta.format' skills/video-audio/SKILL.md` prints 1 or more.

## Report

Send the DONE summary with `horch ledger done`. List each changed section.
