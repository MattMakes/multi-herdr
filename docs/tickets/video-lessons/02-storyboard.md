# 02: Storyboard fields, caption hold check and moved-beat reports

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-storyboard/scripts/check_timeline.py --self-test`
**Own:** `skills/video-storyboard/`

## Goal

video-storyboard defines the 5 new scene fields and check_timeline.py enforces the caption and sync numbers.

## Context

- A horch video fleet produced a product promo: 1 film of 83 s and 3 vertical shorts. The operator called it "really good" and wants many more.
- The lessons file is the spec for this run: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read sections 1, 2 and 7 first. Then read the section this ticket names.
- Evidence, when a lesson is unclear: `/Users/mascott/games/lawn-mower-man/ai_docs/plans/devblog-ep01-video/decisions.md`. If it is still unclear, send a QUESTION to the orchestrator and end your turn.
- 7 units run in parallel. Each unit owns 1 skill folder. The shared contract below fixes every name that crosses 2 units. Use these names exactly.
- The lessons file was written for 1 game. Keep every example generic: "the product", "the app", "the subject". Do not copy a game name, a song name or a path of that repo into a skill.

## Shared contract (fixed by the orchestrator)

Timeline scene fields (optional, in `timeline.json` scenes; times are seconds from the scene start):
- `reframe`: `{"scale": 1.5, "focal_point": [x, y], "from_sec": 0.0, "to_sec": 1.2}`. A push-in on a wide plate.
- `gauge_inset`: `{"source_rect": [x, y, w, h], "scale": 2.0, "event_sec": 1.1, "lead_sec": 0.3, "tail_sec": 1.2, "anchor": "top-right"}`. A proof inset. Rects are normalized 0-1.
- `plate_hold`: `{"at_sec": 2.0, "duration_sec": 0.5}`. Hold 1 plate frame.
- `hud_crop`: `[x, y, w, h]`. A normalized region of product UI to crop or cover.
- `static_hold`: `true`, with `static_hold_reason`: a string. The dead-time check skips this scene.

Capture sidecar: `plates/<clip>.json` next to `plates/<clip>.mp4`:
`{"clip": "plates/<clip>.mp4", "fps": 60, "frame_count": 0, "seed": 0, "build": "<id>", "key_actions_sec": {"<name>": 1.67}, "pixel_facts": {"subject_width_px": 120, "nearest_distance_px": 40}, "max_frozen_sec": 0.017}`.

Audio files:
- `audio/music-edit.md`: the music edit plan. 1 table row per edit: source, section, source in, source out, film in, film out, edit type, crossfade ms.
- `audio/music-continuity.json`: the output of the new `check_music.py` (video-audio). Fields: `window_ms` 50, `phrase_bars` 4, `max_drop_db` 12, `digital_silence` (bool), `worst` (list of `{"sec", "drop_db"}`), `edits` (list, see ticket 04), `loop_joins` (int), `pass` (bool).

Render and QA files:
- `timeline-master.json`: the timeline of the real master. Intro scenes come first, then the film scenes. QA checks this file.
- `qa/accepted-exceptions.md`: the critic fixes that the orchestrator rejected, with reasons. Every critic reads it before it scores.

Report rule (all video seats): every DONE report lists each changed time as `<scene or beat>: <before> s -> <after> s`.

## Steps

1. Read `skills/video-storyboard/SKILL.md`, `references/fix-gates.md` and `scripts/check_timeline.py`.
2. Add `skills/video-storyboard/references/scene-fields.md`. Define `reframe`, `gauge_inset`, `plate_hold`, `hud_crop` and `static_hold` exactly as the shared contract says. Give 1 example per field. Say that a stat-change caption needs a `gauge_inset` from 0.3 s before the event to 1.2 s after it.
3. Extend `check_timeline.py`:
   - Caption hold: each caption shows for at least max(1.5 s, 1.0 s + 0.3 s per word). Fail with the scene id, the hold and the minimum.
   - Caption place: when the timeline gives a caption box, it is in the safe band (16:9: y 0.84-0.95; 9:16: not in the bottom 20 % or the right 15 %).
   - Sync: each cut is within 20 ms of a beat of the beat grid, and each locked action is within 0.07 s of a beat. Read the beat grid the way the script reads it today.
   - New fields: validate the shape of each of the 5 fields. A `gauge_inset` needs `lead_sec` >= 0.3 and `tail_sec` >= 1.2.
   - Menu time: the sum of scenes of kind `ui` or `menu` is at most 8 s per 80 s of film (scale by duration).
   Add a `--self-test` case for each new rule, 1 pass and 1 fail.
4. Add the report rule to `skills/video-storyboard/SKILL.md`: every DONE report lists each moved beat as `<beat>: <before> s -> <after> s`. Add 1-2 lines plus a link to `references/scene-fields.md`.
5. Keep SKILL.md under 12 KB. It is 11377 bytes now, so move existing detail to references/ if you need room.

## Constraints

- A `SKILL.md` must stay under 12 KB (`MAX_SKILL_MD_BYTES` in `crates/horch-core/tests/skills_catalog.rs`). Run `wc -c` on each SKILL.md you change. Put the detail in `references/`. Add 1-2 lines plus a link in SKILL.md.
- When a heuristic has a number, put it in a script check with a `--self-test` case. Prose alone is not enough.
- A new script is executable (`chmod +x`) and has a `--self-test` that runs in less than 10 s with no network. `crates/horch-core/tests/video_pipeline_wave.rs` lists the script entrypoints. If you add a script, add it to the lists there only when the ticket says so.
- Write in Simplified Technical English, as the existing skill files do.
- Edit only the paths in **Own**. Commit exact files, never a directory pathspec.

## Done when

- `check_timeline.py --self-test` passes with the new cases. The check passes.
- `references/scene-fields.md` matches the shared contract field for field.
- SKILL.md is under 12 KB.

## Report

Run `horch ledger done` with: the files changed, the size of each changed SKILL.md in bytes, each new script and its self-test result, and each lesson of the ticket you did not apply, with the reason.
