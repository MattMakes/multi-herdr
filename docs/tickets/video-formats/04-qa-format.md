# 04: QA gate and critic rubric read meta.format

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-feature` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `python3 -I skills/video-qa/scripts/qa_video.py --self-test`
**Own:** `skills/video-qa/`

## Goal

`qa_video.py` fails a master whose length does not fit `meta.format`, and runs `loop_seam` only for `short`. The critic scores a `long` film with long-form rules.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Steps

1. Read `skills/video-qa/scripts/qa_video.py` and its self-test. If it has no `--self-test` flag, find how its tests run (`grep -n self.test`), and use that command as the check. Report the command you used.
2. Add a gate `format_fit`: `meta.format` absent fails; the master length outside the format range of the contract fails. Add it to the numbered gate table in `skills/video-qa/SKILL.md` near line 94.
3. Make `loop_seam` run only when `meta.format` is `short`.
4. Add self-test cases for `format_fit`: a pass and a fail for each format. Use short synthetic clips. A `long` case may fake the length in the timeline meta to keep the test fast. State how in a comment.
5. In SKILL.md "Step 4: score 8 axes", add the `long` rules: axis 1 judges whether the first 30 s prove the title and thumbnail; axis 2 judges chapter re-hooks and pacing over the full runtime. A `long` film never fails for missing feed rules (cut every 2-3 s, 1.5 s hook).
6. For `long`, the contact sheet must sample each chapter. Say this in Step 3.
7. Run the check command. It must pass.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- The check command passes with the new cases.
- The gate table lists `format_fit`.

## Report

Send the DONE summary with `horch ledger done`. Name the check command and the new cases.
