# 06: Render publish pack for long films: chapters.txt, thumbnail frame, end screen

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `node skills/video-render/scripts/assemble_master.mjs --self-test`
**Own:** `skills/video-render/`

## Goal

For `meta.format: long`, the publish pack holds `chapters.txt` (YouTube timestamps) and skips the `9:16` and `1:1` variants. `SKILL.md` documents both formats.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. Read `skills/video-render/SKILL.md` "Step 4 - publish pack" near line 135, and find which script writes the pack (`grep -n "preview.gif" skills/video-render/scripts/*.mjs`).
2. For `long`, write `chapters.txt`: 1 line per chapter, `M:SS <title>`, from `meta.chapters`. The first line is `0:00`.
3. For `long`, do not make `9:16` or `1:1` variants. A short cut is a separate `short` production.
4. Add a self-test case that checks `chapters.txt` for a 3-chapter meta. Find how the script runs its self-test first. If the flag differs from the check command, report the right command.
5. In SKILL.md, add both format rules: `long` keeps the end screen clear of captions for `meta.end_screen_sec`; the cover frame for `long` is from the hero chapter.
6. Run the self-test. It must pass.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- The self-test passes with the chapters case.
- SKILL.md names `chapters.txt`.

## Report

Send the DONE summary with `horch ledger done`. Name the self-test command.
