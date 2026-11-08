# 11: QA fails a dark flash at a cut and a missing gate result

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-qa/scripts/qa_video.py --self-test`
**Own:** `skills/video-qa/`

## Goal

qa_video.py fails a scene whose last 2 frames differ in luma, and fails when an expected gate result file is missing.

## Lesson

Lesson A: a 1-frame dark flash showed before every cut. Gate: the last 2 frames of each scene have equal luma.
Lesson B: a production tool called check_audio.py from a deleted path, so the check silently never ran while reports said "passes". A missing check must fail loudly.

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.

## Steps

1. In `qa_video.py`, add a cut-flash check: for each scene of `timeline-master.json` (or `timeline.json`), measure the mean luma of the last 2 frames before the scene end. Fail when they differ by more than 6 % of full scale, unless the scene has `static_hold` with a fade reason. Report the scene id, the frame numbers and both luma values.
2. Add a gate-result check: `qa_video.py` gets the list of expected gate result files (default: `qa/audio-check.json`, `audio/music-continuity.json`, `qa/timeline-check.json`, `qa/plate-check.json`; `--expect` overrides it). Fail when a file is missing, when it has no `pass` field, or when it is older than the file it checks (master.mp4 or mix.wav). Replace the earlier "read music-continuity.json when it exists" rule: a missing file now fails.
3. Add `--self-test` cases: equal luma (pass), a dark last frame (fail), a missing gate result (fail), a stale gate result (fail).
4. Add a self-review rule to `skills/video-qa/references/rubric.md`. Lesson: 4 of 5 solo-built episodes passed 15-18 of 18 checks in self-review, then scored 3.00-3.75 in the blind review. Every fault was "the key action does not read at phone size" (a 100 px boss, a 2 px ring, a ring lost in the ground texture, a hero weaker than an earlier shot, a health bar that rises between shots). A self-review views the hero frame and every key-action frame scaled to 360 px high and answers 2 questions: "Can a new viewer see what happened within 1 s?" and "Is the hero the strongest moment?". A no to either is a BLOCKER. The self-review never replaces the blind critic.
5. Add a `--phone-frames <dir>` option to `qa_video.py`. It writes the hero frame and each key-action frame (from `locked_actions_sec`, or the scene midpoint) scaled to 360 px high, so the reviewer looks at them. Add a self-test case that checks the frame count and size.
6. Document both checks in `skills/video-qa/references/motion-checks.md` and add 1 line to SKILL.md.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.

## Done when

- `qa_video.py --self-test` passes with the new cases. The check passes.

## Report

Run `horch ledger done` with the files changed, each SKILL.md size in bytes, each self-test result, and each part of the lesson you did not apply, with the reason.
