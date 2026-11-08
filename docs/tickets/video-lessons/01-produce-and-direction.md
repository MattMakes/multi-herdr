# 01: Promo brief, claims ledger and phone framing rules

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-produce/scripts/check_claims.py --self-test`
**Own:** `skills/video-produce/`, `skills/video-direction/`

## Goal

video-produce gates a promo script and its claims before production, and video-direction carries the phone framing, hook and hero rules.

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

1. Read `skills/video-produce/SKILL.md` and `skills/video-direction/SKILL.md` and their references.
2. Add `skills/video-produce/references/promo-brief.md`. Cover lesson rules 1 and 3: the audience is the buyer or player; no jargon, commit hashes or tool names; 1 hero moment per episode; a hook in the first 2 s; a CTA at the end; the operator approves the script before any production unit starts.
3. In the same reference, define the claims ledger `claims.md`: 1 row per caption claim, with verdict `true`, `partly true` or `false`, and the evidence (code path, play test, or measurement). A `false` or unverified claim blocks the script.
4. Add `skills/video-produce/scripts/check_claims.py`. It reads `claims.md` and exits 1 when a row has verdict `false`, an empty evidence cell, or the text `[inferred]`. Add a `--self-test`.
5. Add 2 lines plus links to `skills/video-produce/SKILL.md`.
6. Add `skills/video-direction/references/phone-framing.md`. Cover these rows of lessons section 2: subject size (100-120 px wide at 1080p, push-in with `reframe`), find time (1 s), UI shots (labels 10 px at 360 px high, or a 1.5-1.6x push-in), menu time (8 s or less per 80 s), hook (the first shot proves the hook line; each short has its own hook), hero (last in the content, on the biggest music lift, holds 2-2.5 s after the payoff). Add rule 5 (show the claim on screen; use `gauge_inset`) and rule 7 (stage a new capture take; do not rescue a weak take in the edit).
7. Add the brand intro rules of lessons section 5 to the same reference: 5-6 s, the same in every long video; shorts open on content and show logos at the end only; iterate the intro with the operator as its own small unit first.
8. Add 2 lines plus a link to `skills/video-direction/SKILL.md`. Keep it under 12 KB (it is 10625 bytes now).

## Constraints

- A `SKILL.md` must stay under 12 KB (`MAX_SKILL_MD_BYTES` in `crates/horch-core/tests/skills_catalog.rs`). Run `wc -c` on each SKILL.md you change. Put the detail in `references/`. Add 1-2 lines plus a link in SKILL.md.
- When a heuristic has a number, put it in a script check with a `--self-test` case. Prose alone is not enough.
- A new script is executable (`chmod +x`) and has a `--self-test` that runs in less than 10 s with no network. `crates/horch-core/tests/video_pipeline_wave.rs` lists the script entrypoints. If you add a script, add it to the lists there only when the ticket says so.
- Write in Simplified Technical English, as the existing skill files do.
- Edit only the paths in **Own**. Commit exact files, never a directory pathspec.

## Done when

- The 2 references exist and cover every item of steps 2, 3, 6 and 7.
- `check_claims.py --self-test` passes. The check passes.
- Both SKILL.md files are under 12 KB.

## Report

Run `horch ledger done` with: the files changed, the size of each changed SKILL.md in bytes, each new script and its self-test result, and each lesson of the ticket you did not apply, with the reason.
