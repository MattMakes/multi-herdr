# 01: Storyboard plan gate checks meta.format, chapters and end screen

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-feature` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `python3 -I skills/video-storyboard/scripts/check_timeline.py --self-test && python3 -I skills/video-audio/scripts/reconcile_timing.py --self-test`
**Own:** `skills/video-storyboard/`, `skills/video-audio/scripts/reconcile_timing.py`

## Goal

`check_timeline.py` refuses a timeline with no `meta.format`, and enforces the `short` and `long` rules of the contract. The storyboard skill tells the author how to plan a long film in chapters.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. Read `skills/video-storyboard/scripts/check_timeline.py`, mainly the meta schema checks near line 160 and `self_test` near line 1066.
2. Add the checks of the contract section "Gates" for `check_timeline.py`. Use 1 clear error message for each rule. Each message names the field and the allowed range. Check: run the script on a hand-made 74 s `long` timeline. It must print the duration error.
3. Keep the hero window rule (30 % to 90 % of the film) for both formats.
4. Update every existing self-test fixture: add `"format": "short"`. Check: the old cases still pass.
5. Add self-test cases: missing format fails; 74 s `long` fails; `long` in `9:16` fails; `long` with 2 chapters fails; a chapter of 200 s fails; `long` with no `end_screen_sec` fails; `short` with `chapters` fails; a 300 s `long` film with 4 valid chapters and `end_screen_sec` 10 passes.
6. Update `skills/video-storyboard/SKILL.md`: the meta table, the order of work (a `long` film is planned chapter by chapter, each chapter opens on its `hook` caption), the gate list and its count. Update `skills/video-storyboard/references/fix-gates.md` with a fix line for each new error.
7. For `long` with no narration, the caption read-hold rule carries the story. Say this in SKILL.md. Do not add a voice-over requirement.
8. `skills/video-audio/scripts/reconcile_timing.py` imports `check_timeline` in its self-test (near line 353). Add `"format": "short"` to its fixture meta near line 295. Change nothing else in that file.
9. Run the check command. Both self-tests must pass.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- The check command passes and lists the new cases.
- `grep -c 'meta.format' skills/video-storyboard/SKILL.md` prints 1 or more.

## Report

Send the DONE summary with `horch ledger done`. List each new error message and each new self-test case.
