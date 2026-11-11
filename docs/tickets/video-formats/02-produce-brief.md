# 02: Producer picks the format; promo brief and batch manifest carry it

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `python3 -I skills/video-produce/scripts/validate_batch.py --self-test`
**Own:** `skills/video-produce/`

## Goal

`video-produce` sets `Format: short` or `Format: long` from the operator's words before any other step. The promo brief and the batch manifest follow the format rules of the contract.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. In `skills/video-produce/SKILL.md` section "1. Pick the workflow", add the format choice of the contract section "Choosing the format". Put it first. Say: ask the operator when the words do not decide it.
2. Change the defaults line near line 58. A silent request stays `short`, 30 s, 16:9. Add: a request for episodes or a YouTube video is `long`, 16:9, captions only.
3. Change the PRODUCTION.md template near line 70. Add the line `Format: <short | long>`. Change `Length:` to `<15 | 30 | 60 | 90> s (short) or <180-900> s (long)`.
4. Rewrite `skills/video-produce/references/promo-brief.md` section "Write for the viewer". Split it in 2 lists: `short` (hook in the first 2 s, as today) and `long` (first 30 s prove the title and thumbnail; 3 or more chapters with a hook caption each; 3 title options, 1 thumbnail concept, chapter timestamps). A devlog series with shorts is 2 productions per episode: 1 `long`, 1 `short`.
5. Change `skills/video-produce/scripts/validate_batch.py`: each film needs `"format"`. Apply the batch rules of the contract section "Fields". Update the docstring near line 26 and `LENGTHS` near line 44. Update the example in `skills/video-produce/references/ab-and-batch.md`.
6. Add self-test cases: no format fails; `long` 74 s fails; `long` with `9:16` fails; `long` 300 s `16:9` passes; `short` 30 s passes.
7. Run the check command. It must pass.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- The check command passes and counts the new cases.
- `grep -n 'Format:' skills/video-produce/SKILL.md` finds the template line.

## Report

Send the DONE summary with `horch ledger done`. List the changed files and the new self-test cases.
