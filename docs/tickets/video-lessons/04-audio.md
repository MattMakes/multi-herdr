# 04: Music laid last, continuity check and SFX bus rules

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-audio/scripts/check_audio.py --self-test && skills/video-audio/scripts/check_music.py --self-test`
**Own:** `skills/video-audio/`, `crates/horch-core/tests/video_pipeline_wave.rs`

## Goal

video-audio lays the music last as 1 continuous bed, and check_music.py and check_audio.py enforce the music and peak numbers.

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

1. Read `skills/video-audio/SKILL.md`, its references and `scripts/check_audio.py`.
2. Add `skills/video-audio/references/music-laid-last.md` with every rule of lessons section 3: measure the tempo from the song file (never trust metadata); the beat grid only plans cuts; the bed comes after picture lock; edit at downbeats on 4- or 8-bar phrases with 20-60 ms equal-power crossfades; repeat or skip whole bars; at most 3 % time-stretch; no pitch-shift; write `audio/music-edit.md` first, then build from 1 script; a breakdown, not silence (low-pass, drop the drums, dip at most 12 dB, hero hit on the downbeat); intro and film are 1 audio file; transient control on the SFX bus only; a signature end song for a series, shorts blend into it over 1-2 bars; SFX round-robin, small pitch steps on a scale, varied timbre; "main hit + offset tap" rhythm.
3. Add `skills/video-audio/scripts/check_music.py`. Input: the music bed wav and the beat grid. In 50 ms windows, fail when a window is more than 12 dB under the median of its 4-bar neighbourhood, or when the bed has digital silence. Write `audio/music-continuity.json` as the shared contract says. Add a `--self-test` with synthetic signals: 1 continuous bed (pass), 1 bed with a gap (fail), 1 breakdown with a 10 dB dip (pass).
4. Extend `check_music.py` with musical continuity at each edit point. Read the edit points from the `film in` column of `audio/music-edit.md`. Lesson from the operator's ear: a loudness check passed, but a 40 ms splice between 2 songs a half step apart (for example F major into E minor) sounded broken. Splices between compatible keys (D major or B minor into E minor) sounded good.
   - At each edit point, measure the tempo on both sides, the beat-phase offset (fail above 15 ms), the key or chroma compatibility across the join, and the spectral-flux spike ratio. Count the short loop joins in the bed.
   - Fail a short splice (a crossfade under 1 bar) between keys that clash: a half step or a tritone apart. The fix is a blend of at least 1 bar inside a filtered breakdown, or a bridge bar with no pitched notes.
   - Add a per-edit list to `audio/music-continuity.json`: `edits` (list of `{"film_sec", "tempo_left", "tempo_right", "phase_ms", "key_left", "key_right", "key_clash", "flux_ratio", "crossfade_ms", "pass"}`) and `loop_joins` (count).
   - Write a 6 s excerpt wav around each edit point to `audio/edit-excerpts/edit-<n>-<film_sec>.wav`, so a human listens to each risky join. Print the paths.
   - Add `--self-test` cases with synthetic tones: a short splice between keys a half step apart (fail), the same splice between compatible keys (pass), a phase offset of 30 ms (fail).
   - Add these rules to `references/music-laid-last.md`. Say that the numbers do not replace a listen: the operator listens to the excerpts before the mix locks.
5. Extend `check_audio.py`:
   - Cut peaks: the peak within 20 ms of each cut is -3.0 dBFS or lower.
   - True peak: the wav is at -2.0 dBTP or lower. Say in the output that AAC adds about +1 dB, and that video-render measures -1.0 dBTP on the encoded MP4.
   Add `--self-test` cases for both. Skip a rule that already exists, and say so in the report.
6. Add `check_music.py` to the script entrypoint lists in `crates/horch-core/tests/video_pipeline_wave.rs` (the lists near lines 100-120 name `check_audio.py`). Change nothing else in that file.
7. Add 2 lines plus links to `skills/video-audio/SKILL.md`. Keep it under 12 KB (it is 11321 bytes now; move detail to references/ if needed).

## Constraints

- A `SKILL.md` must stay under 12 KB (`MAX_SKILL_MD_BYTES` in `crates/horch-core/tests/skills_catalog.rs`). Run `wc -c` on each SKILL.md you change. Put the detail in `references/`. Add 1-2 lines plus a link in SKILL.md.
- When a heuristic has a number, put it in a script check with a `--self-test` case. Prose alone is not enough.
- A new script is executable (`chmod +x`) and has a `--self-test` that runs in less than 10 s with no network. `crates/horch-core/tests/video_pipeline_wave.rs` lists the script entrypoints. If you add a script, add it to the lists there only when the ticket says so.
- Write in Simplified Technical English, as the existing skill files do.
- Edit only the paths in **Own**. Commit exact files, never a directory pathspec.

## Done when

- Both self-tests pass. The check passes.
- `check_music.py` is executable and is in the entrypoint lists of `video_pipeline_wave.rs`.
- SKILL.md is under 12 KB.

## Report

Run `horch ledger done` with: the files changed, the size of each changed SKILL.md in bytes, each new script and its self-test result, and each lesson of the ticket you did not apply, with the reason.
