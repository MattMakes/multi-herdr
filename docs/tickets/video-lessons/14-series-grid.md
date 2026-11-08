# 14: 1 measured beat grid per song, checked against the bed at render time

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-audio/scripts/check_music.py --self-test && skills/video-audio/scripts/check_audio.py --self-test`
**Own:** `skills/video-audio/scripts/`, `skills/video-audio/SKILL.md`, `skills/video-audio/references/sync-and-placement.md`

## Goal

A series stores 1 measured beat grid per song, and the audio gate checks every cut against the beats measured from the final bed, not against a copied bar length.

## Lesson

Episodes of a series copied a song bar length (2.3668 s) that was correct only with the source warp of episode 1. Their cuts landed 0.13-0.21 s early by the end of the film. Store 1 measured grid file per song (bar length, downbeats) at the series level. Check cuts against the bed's own measured beats at render time.

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.


## Steps

1. Read `check_music.py`, `check_audio.py` and `references/sync-and-placement.md`.
2. Define the series grid file `<series root>/music/grids/<song id>.json`: `song`, `source_sha256`, `tempo_bpm`, `bar_sec`, `downbeats_sec` (list), `measured_by` (script and version). Document it in `references/sync-and-placement.md`. Say that an episode reads it and never copies a number by hand.
3. Add a `--write-grid <out.json>` option to `check_music.py`. It measures the tempo and downbeats of a song file and writes the grid file.
4. Add a `--bed-beats` check to `check_audio.py`: it measures the beats of the final bed wav, and fails when a cut of `timeline-master.json` (or `timeline.json`) is more than 20 ms from a measured bed beat. Report the worst drift in seconds and where it is. A copied grid that drifts by 0.13 s must fail.
5. Add `--self-test` cases: a grid written from a synthetic click track matches its tempo within 0.1 BPM; a cut list that drifts 0.15 s by the end fails; an aligned cut list passes.
6. Add 1 line plus a link to SKILL.md. Do not edit `references/music-laid-last.md`: ticket 13 owns it.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.


## Done when

- Both self-tests pass with the new cases. The check passes.
- `sync-and-placement.md` defines the series grid file.

## Report

Run `horch ledger done` with the files changed, the SKILL.md size, and each self-test result.
