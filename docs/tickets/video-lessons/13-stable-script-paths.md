# 13: Production tools resolve skill scripts at run time

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave`
**Own:** `skills/orchestrate/references/video-pipeline.md`, `skills/video-produce/`, `skills/video-audio/references/music-laid-last.md`

## Goal

The video recipe and video-produce tell every production tool to resolve skill scripts with horch skills path at run time and to fail loudly when a script is missing.

## Lesson

A production tool must not point to a temporary skill-bundle path. horch copies skills per spawn and deletes old copies. The music builder called check_audio.py from a deleted bundle, so that check silently never ran while reports said "passes".

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.

## Steps

1. Read the DONE summary of ticket 12 with `horch ledger show` on its task. Use the exact command it built: `horch skills path <id> --script <rel>`.
2. In `skills/video-produce/references/promo-brief.md` or a new `skills/video-produce/references/project-tools.md`, add the rule: a project script that calls a skill script resolves it with `"$(horch skills path <id> --script <rel>)"` at run time; it never stores a `skill-bundles` path; it exits non-zero when the resolve fails or the script exits non-zero; a gate that did not run is a failure, not a pass.
3. Add 1 line plus a link to `skills/video-produce/SKILL.md`.
4. In `skills/orchestrate/references/video-pipeline.md`, add the rule to the ownership or avoid section, with 1 example line. Say that qa_video.py fails a missing gate result (ticket 11).
5. In `skills/video-audio/references/music-laid-last.md`, change the music build script rule so that it calls check_audio.py and check_music.py through `horch skills path`.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.

## Done when

- The 3 files hold the rule with the exact command of ticket 12. The check passes.

## Report

Run `horch ledger done` with the files changed, each SKILL.md size in bytes, each self-test result, and each part of the lesson you did not apply, with the reason.
