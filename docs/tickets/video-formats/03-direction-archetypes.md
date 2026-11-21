# 03: Direction formats and a long-form episode archetype

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** none
**Own:** `skills/video-direction/`

## Goal

`video-direction` offers both formats. `references/archetypes.md` has a long-form episode archetype, and every existing archetype is labeled `short`.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. In `skills/video-direction/SKILL.md` section "BRIEF.md", add a `Format` row. Its value is `short` or `long`, from `PRODUCTION.md`.
2. Replace the target duration line near line 64. `short`: 15, 30, 60 or 90 s. `long`: 180 to 900 s, `16:9` only.
3. Change the `Channel` row near line 50. Sound-off defaults apply to `short`. A `long` film plays on a YouTube page with captions only.
4. Change the hook sentence near line 126 and the checklist near line 213 so each one has a `short` rule and a `long` rule.
5. In `skills/video-direction/references/archetypes.md`, label the 4 existing archetypes `short`. Label the hook rules near line 28 `short` and add the `long` opening rules (cold open in motion, studio intro of 5 s or less allowed after it, first 30 s prove the promise).
6. Add archetype 5: "Long-form episode (180 s to 900 s)". Beat table by share of runtime: cold open 0-5 %, promise 5-10 %, chapters 10-90 % (3 or more, each with a hook, build and payoff), hero chapter last, CTA and end screen 90-100 %. Add rules: chapters of 30 s to 180 s, music bed changes between chapters, shot length varies, end screen of 5 s to 20 s.
7. Fix the intro sentence near line 8 that says the archetypes scale only to 15, 30, 60 or 90 s.
8. Read both files again. Confirm every rule names its format.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- `grep -c 'long' skills/video-direction/references/archetypes.md` prints 3 or more.
- The checklist has a format line.

## Report

Send the DONE summary with `horch ledger done`. List each changed section.
