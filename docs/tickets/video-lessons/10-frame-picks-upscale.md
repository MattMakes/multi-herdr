# 10: Frame picks use floor, and the raster gate detects pre-upscaled plates

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && node skills/video-motion/scripts/seek-timeline.selftest.mjs && skills/video-motion/scripts/scan_linear_motion.py --self-test`
**Own:** `skills/video-motion/`, `skills/video-render/`

## Goal

Seekable scenes never pick a plate frame past the export, and the 1.2x raster gate fails a plate that was upscaled before the gate.

## Lesson

Lesson A: seekable scenes must pick plate frames with floor, clamped to the last exported plate frame. Motion-blur subframes with Math.round(t * fps) asked for a frame past the export, so a 1-frame dark flash showed before every cut.
Lesson B: pre-upscaling the frames to pass the 1.2x raster gate is cheating, and the gate must detect it.

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.

## Steps

1. Find every plate frame pick in `skills/video-motion/scripts/seek-timeline.js` and `skills/video-render/scripts/render_frames.mjs` (for example `Math.round(sec * fps)` near line 339 of render_frames.mjs). A plate frame pick uses `Math.min(Math.floor(t * fps + 1e-6), lastFrame)`. Do not change frame counts of the output timeline: only plate frame picks.
2. Add a test case to `seek-timeline.selftest.mjs`: a motion-blur subframe at the last 0.5 frame of a scene picks the last exported frame, never lastFrame + 1.
3. Document the rule in `skills/video-motion/references/seek-contract.md`.
4. Find the 1.2x upscale check that ticket 05 added (in `scan_linear_motion.py` or the render scripts). Add a pre-upscale detector: downscale the plate frame by its claimed upscale factor, upscale it back, and compare to the original. A near match (PSNR above 40 dB, or a documented equivalent) means the frame holds no detail above its real resolution. Fail it with the measured real resolution.
5. Add `--self-test` cases: a sharp frame (pass), the same frame downscaled 2x and upscaled back (fail).
6. Document the detector in `skills/video-render/references/raster-sizes.md` or `encoded-gates.md`.
7. Run `node --check` on each changed .js and .mjs file.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.

## Done when

- The selftests pass with the new cases. `node --check` passes. The check passes.
- No plate frame pick uses Math.round.

## Report

Run `horch ledger done` with the files changed, each SKILL.md size in bytes, each self-test result, and each part of the lesson you did not apply, with the reason.
