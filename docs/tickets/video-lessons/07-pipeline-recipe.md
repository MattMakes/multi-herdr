# 07: Video recipe: round loop, ownership, media paths, ledger tasks

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave`
**Own:** `skills/orchestrate/references/video-pipeline.md`, `docs/recipes/video-pipeline.md`

## Goal

The orchestrate video recipe carries the round loop, the ownership rules and the horch 0.4.6 task route, so a new fleet applies them from the start.

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

1. Read `skills/orchestrate/references/video-pipeline.md` and `docs/recipes/video-pipeline.md`. Run `horch ledger task --help` and `horch spawn --help`.
2. Add the keep rules of lessons section 6: a shared CONTEXT.md and decisions.md per production that every worker reads; 1 plan file per unit; the wave order (producer -> director with 3 directions, pick 1 -> capture -> audio grid -> scenes -> editor -> critic); the round loop (director and capture in parallel -> audio and scenes in parallel -> editor -> fresh critic); at most 3 critic rounds, then the 3 critiques go to the operator.
3. Add the triage rules: route each critic fix by file ownership, not by the seat the critic names; reject a fix that breaks the brief or asks for content that does not exist; write each rejection with its reason to `qa/accepted-exceptions.md` (shared contract), and the next critic reads it.
4. Add the ownership rules: 1 owner per output folder per round; tell the old owner before a new owner starts; rename or delete a superseded build script, and name it in the plan of every later unit.
5. Add the horch facts: horch 0.4.6 refuses `horch spawn <persona> "task"`. Queue each one-off with `horch ledger task "<title>" --persona <seat> --file <plan> --check none`, then run the spawn line it prints. A worker runs in a git worktree that does not hold untracked media. Large media stays out of git; commit the text sources. Give every plan absolute paths to the shared media root.
6. Add the gate step: the script and claims gate (video-produce `check_claims.py`) runs with the operator before any production unit. Add the brand intro as its own small unit before the episodes (1 animator + 1 audio unit per round of notes).
7. Add the avoid list of lessons section 6 that no other unit covers. Do not repeat audio, capture or render rules: link to the seat skill instead.
8. Remove or fix every place in both files that tells the orchestrator to type a task into `horch spawn`.
9. Keep `docs/recipes/video-pipeline.md` a short pointer; update it only if it names something that changed.

## Constraints

- A `SKILL.md` must stay under 12 KB (`MAX_SKILL_MD_BYTES` in `crates/horch-core/tests/skills_catalog.rs`). Run `wc -c` on each SKILL.md you change. Put the detail in `references/`. Add 1-2 lines plus a link in SKILL.md.
- When a heuristic has a number, put it in a script check with a `--self-test` case. Prose alone is not enough.
- A new script is executable (`chmod +x`) and has a `--self-test` that runs in less than 10 s with no network. `crates/horch-core/tests/video_pipeline_wave.rs` lists the script entrypoints. If you add a script, add it to the lists there only when the ticket says so.
- Write in Simplified Technical English, as the existing skill files do.
- Edit only the paths in **Own**. Commit exact files, never a directory pathspec.

## Done when

- `skills/orchestrate/references/video-pipeline.md` covers steps 2-8.
- `grep -n 'horch spawn [a-z-]* "' skills/orchestrate/references/video-pipeline.md` prints nothing.
- The check passes. `vpw_03_the_recipe_is_reachable_from_the_orchestrate_skill` still passes.

## Report

Run `horch ledger done` with: the files changed, the size of each changed SKILL.md in bytes, each new script and its self-test result, and each lesson of the ticket you did not apply, with the reason.
