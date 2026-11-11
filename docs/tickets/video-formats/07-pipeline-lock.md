# 07: Pipeline recipe, teammates, the contract file and the regression lock

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `sonnet-feature` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo test -p horch-core --test video_pipeline_wave`
**Own:** `skills/orchestrate/references/video-pipeline.md`, `teammates/video-director.md`, `teammates/video-producer.md`, `teammates/video-critic.md`, `crates/horch-core/tests/video_pipeline_wave.rs`, `docs/specs/video-formats.md`, `CHANGELOG.md`

## Goal

The orchestrator recipe and the video teammates name both formats. A Rust test fails when a video skill loses the format split. The contract file is committed.

## Context

The fleet video skills treat every video as a muted feed clip of 90 s or less. The monuments devlog run made 6 YouTube episodes of about 74 s. They had to be long form. The operator decided the fix on 2026-11-08: 2 formats, `short` and `long`. Long form is 3 to 15 min, 16:9, captions only, no voice-over.

Read the contract first: `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md`. That file is in the main checkout, not in your worktree. Use its field names and numbers exactly. Do not invent other names. Ticket 07 commits the contract file.

Other tickets of this run edit the other video skills at the same time. Edit only the paths in **Own**.

## Prior work

Tickets 01 to 06 have landed on the run branch before you start. Read their diffs with `git log -p` on the run branch.

## Steps

1. Copy `/Users/matthewscott/repo/multi-herdr/docs/specs/video-formats.md` to `docs/specs/video-formats.md` in your worktree. Commit it unchanged.
2. In `skills/orchestrate/references/video-pipeline.md`, near lines 76 and 89: the orchestrator records `Format` with the length. A request for "N videos and M shorts" gives N `long` productions and M `short` productions. A short is never a reframe of a long master.
3. In `teammates/video-director.md`, `teammates/video-producer.md` and `teammates/video-critic.md`: add 1 line each that tells the worker to read `meta.format` and follow its rules. Keep every other line.
4. In `crates/horch-core/tests/video_pipeline_wave.rs`, add the test of the contract section "Regression lock". It reads each listed skill file. It asserts the file contains `short` and `long`, and `meta.format` for every file except `promo-brief.md` and `archetypes.md`. The failure message names the file and the missing word.
5. Run `HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`. It must pass.
6. Add 1 line under the unreleased section of `CHANGELOG.md`: "Video skills now separate short form (15-90 s) from long form (3-15 min YouTube episodes)."
7. Run the check command. It must pass.

## Constraints

- Write skill text in Simplified Technical English: 1 instruction per sentence, active voice, at most 20 words per procedural sentence.
- Keep every existing short-form rule. Label it as a `short` rule. Do not delete it.
- Do not use `ANTHROPIC_API_KEY`.
- Out of scope: the other skill folders, Rust code, and the monuments project.

## Done when

- The check command passes and includes the new test.
- `teammates --check` passes.
- `docs/specs/video-formats.md` is committed.

## Report

Send the DONE summary with `horch ledger done`. Name the new test.
