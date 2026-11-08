# 05: Organic motion, plate scale and encoded-file gates

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave`
**Own:** `skills/video-motion/`, `skills/video-render/`

## Goal

video-motion carries the organic limb, impact and plate-scale rules, and video-render measures the real encoded files.

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

1. Read `skills/video-motion/SKILL.md`, `skills/video-render/SKILL.md` and their references.
2. Add `skills/video-motion/references/organic-motion.md`: organic limbs (tentacles, tails, rope, cloth) use FABRIK inverse kinematics with 40 or more joints and several iterations, plus layered sine waves along the chain; a low joint count gives notches at the root; each impact changes the scene (for example, each blow completes a part of a logo); no plate is drawn at more than 1.2x its natural size, so export plate frames at 2x when a zoom needs it.
3. If `skills/video-motion/scripts/scan_linear_motion.py` or the existing scripts can check the 1.2x upscale rule from `raster-sizes.json`, add that check with a self-test case. If not, say why in the report.
4. Add 2 lines plus a link to `skills/video-motion/SKILL.md`.
5. Add `skills/video-render/references/encoded-gates.md`:
   - Measure loudness (-14 LUFS, from -15 to -13) and true peak (-1.0 dBTP or lower) on the ENCODED master.mp4 with `ffmpeg -af ebur128=peak=true`. Keep the wav at -2.0 dBTP, because AAC adds about +1 dB.
   - Each derived file (9x16, preview) has the same frame count as master.mp4 (ffprobe `-count_frames`). A packaging step must never cut the film.
   - Do not use the ffmpeg `trim` filter on clips with odd timestamps. Use `select='between(n,A,B)'` with `-fps_mode passthrough`.
   - Write `timeline-master.json` as the shared contract says: intro scenes first, then the film scenes, so QA checks the real master.
   - Extract fresh probe frames every round. Never reuse probe frames of an earlier round.
6. If `skills/video-render/scripts/assemble_master.mjs` writes the master, make it write `timeline-master.json` and print the frame count of each derived file. Run `node --check` on the script.
7. Add 1-2 lines plus a link to `skills/video-render/SKILL.md`. It is 12002 bytes now and the limit is 12288. Move existing detail to a reference first, so that SKILL.md ends at 11500 bytes or less.

## Constraints

- A `SKILL.md` must stay under 12 KB (`MAX_SKILL_MD_BYTES` in `crates/horch-core/tests/skills_catalog.rs`). Run `wc -c` on each SKILL.md you change. Put the detail in `references/`. Add 1-2 lines plus a link in SKILL.md.
- When a heuristic has a number, put it in a script check with a `--self-test` case. Prose alone is not enough.
- A new script is executable (`chmod +x`) and has a `--self-test` that runs in less than 10 s with no network. `crates/horch-core/tests/video_pipeline_wave.rs` lists the script entrypoints. If you add a script, add it to the lists there only when the ticket says so.
- Write in Simplified Technical English, as the existing skill files do.
- Edit only the paths in **Own**. Commit exact files, never a directory pathspec.

## Done when

- The 2 references exist and cover steps 2 and 5.
- The check passes. `node --check` passes on each changed .mjs file.
- Both SKILL.md files are under 12 KB; video-render is at 11500 bytes or less.

## Report

Run `horch ledger done` with: the files changed, the size of each changed SKILL.md in bytes, each new script and its self-test result, and each lesson of the ticket you did not apply, with the reason.
