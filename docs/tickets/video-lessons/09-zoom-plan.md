# 09: Plan every zoom before capture

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-storyboard/scripts/check_timeline.py --self-test && skills/video-capture/scripts/check_plate.py --self-test`
**Own:** `skills/video-storyboard/`, `skills/video-capture/`

## Goal

The storyboard lists every zoom before capture, capture records zoomed shots at a matching render scale, and both checks fail a zoom that the plate cannot serve.

## Lesson

A 1.6x push-in or a 2x inset on a 1080p capture fails the 1.2x raster gate, and the render cannot fix it. The storyboard must list every zoom before capture. Capture records those shots at a 2x-3x engine render scale, or with a closer camera.

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.

## Steps

1. In `check_timeline.py`, compute the zoom need of each scene: the largest of `reframe.scale` and `gauge_inset.scale`. Add `capture_scale` (number, default 1.0) as an optional scene field. Fail when zoom need / capture_scale > 1.2. Print the scene id, the zoom need and the capture scale it needs.
2. Add a `--zoom-plan <out.json>` option to `check_timeline.py`. It writes 1 row per scene with a zoom need above 1.0: scene id, plate, zoom need, required capture scale (zoom need / 1.2, rounded up to 0.5 steps).
3. Document `capture_scale` and the zoom plan in `skills/video-storyboard/references/scene-fields.md`. Say that the storyboard writes the zoom plan before any capture unit starts.
4. In `skills/video-capture/references/app-engine-capture.md`, add: read the zoom plan; record each listed shot at its required scale with the engine render scale (2x-3x) or a closer camera; record the scale in the sidecar as `render_scale`.
5. In `check_plate.py`, validate `render_scale` in the sidecar (number, 1.0 or more). When a `--zoom-plan` file is given, fail a listed plate whose `render_scale` is under the required scale.
6. Add a wall-clock timer rule to `app-engine-capture.md`. Lesson: a game hit-stop timed on the wall clock (0.04 s) made each fixed-fps take play a different fight. Before capture, the worker searches the product code for timers that read the wall clock (delta time, real time, timers in seconds). The capture driver forces each one to a frame count at the capture fps. The sidecar records `timers_forced` (list of names).
7. Add the self-test cases.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.

## Done when

- Both self-tests pass with the new cases. The check passes.
- `scene-fields.md` documents `capture_scale` and the zoom plan.

## Report

Run `horch ledger done` with the files changed, each SKILL.md size in bytes, each self-test result, and each part of the lesson you did not apply, with the reason.
