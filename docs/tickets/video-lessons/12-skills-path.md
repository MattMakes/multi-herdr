# 12: horch skills path gives a stable path to a skill script

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch --test skills_path && cargo test -p horch-core --test skills_catalog --test video_pipeline_wave`
**Own:** `crates/horch/src/cmd/skillscmd.rs`, `crates/horch-core/src/skills.rs`, `crates/horch-core/src/skills/`, `crates/horch/tests/skills_path.rs`

## Goal

`horch skills path <id> [--script <rel>]` prints an absolute path that stays valid after the spawn's skill bundle is deleted, and exits 1 when the script is missing.

## Lesson

horch copies skills into a bundle per spawn (`~/.local/state/horch/skill-bundles/<spawn id>/skills/`) and deletes old bundles. A production tool stored a bundle path to check_audio.py. horch deleted that bundle, so the check never ran, and the reports said "passes". `horch skills show video-audio --json` gives `"install_path": null` for a bundled skill, so no stable path exists today.

## Context

- Run video-lessons (PR #341) landed tickets 01-07: the promo lessons in the video skills. This ticket adds 1 of 3 lessons from round 3 of the same production.
- Spec: `/Users/mascott/games/lawn-mower-man/ai_docs/devblog/video-pipeline-lessons.md`. Read lines 95-155 for the round 3 lessons. Read the ticket's own lesson below first.
- Landed names that this ticket must keep: `reframe` (`scale`), `gauge_inset` (`scale` 2.0), the capture sidecar `plates/<clip>.json` with `pixel_facts`, `raster-sizes.json`, `timeline-master.json`, `static_hold`.
- Keep every example generic. No game names or song names.

## Steps

1. Read `crates/horch/src/cmd/skillscmd.rs` and `crates/horch-core/src/skills.rs`. Find where bundles are written and deleted (search `skill-bundles`).
2. Add `horch skills path <id> [--script <rel>]`. For a bundled skill, materialize the skill files once into a content-addressed folder `<state_root>/skills/<id>/<digest>/`, then print the absolute folder path, or the script path with `--script`. For an installed skill, print its install path. Bundle cleanup must never delete this folder.
3. Exit 1 with a clear message when the skill id is unknown or the script file does not exist. Never print a path that does not exist.
4. Write tests in `crates/horch/tests/skills_path.rs`: a bundled skill path exists and holds SKILL.md; `--script scripts/check_audio.py` of video-audio exists and is executable; a missing script exits 1; a 2nd call returns the same path.
5. Run the check.

## Constraints

- A `SKILL.md` stays under 12288 bytes. Detail goes to `references/`; SKILL.md gets 1-2 lines plus a link.
- Each number goes into a script check with `--self-test` cases (1 pass, 1 fail). Self-tests use `tempfile`, never a fixed `/tmp` path, and run in less than 10 s.
- Keep functions small: `horch ledger finish` runs a complexity sweep on changed functions.
- Edit only the paths in **Own**. Commit exact files. Never commit `ai_docs/`.

## Done when

- The tests pass. The check passes.
- The DONE report shows 1 example output of `horch skills path video-audio --script scripts/check_audio.py`.

## Report

Run `horch ledger done` with the files changed, each SKILL.md size in bytes, each self-test result, and each part of the lesson you did not apply, with the reason.
