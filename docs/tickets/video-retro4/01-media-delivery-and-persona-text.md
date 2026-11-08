# 01: Deliver finished videos through LFS; fix critic-owner and usage-limit persona text

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave`
**Own:** `teammates/video-animator.md`, `teammates/video-audio.md`, `teammates/video-capture.md`, `teammates/video-critic.md`, `teammates/video-director.md`, `teammates/video-editor.md`, `teammates/video-producer.md`, `skills/orchestrate/references/video-pipeline.md`, `teammates/_base/fleet-orchestrator.md`, `docs/recipes/video-pipeline.md`

## Goal

The video personas and the pipeline reference carry changes 0.1, 0.8 and 0.9, plus the pipeline-reference line of 0.3.

## Context

- Source: the lawn-mower-man video fleet retro, round 4 (83 session transcripts, episodes 1-6). The operator asked for Tier 0 to land before PR #344 merges. Read the Tier 0 table in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/multi-herdr-video-changes.md` and the rows for your items in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/video-fleet-retro.md` and `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/retro-2026-11/gaps-*.md` (lesson IDs in `candidates.md`). Those files are read-only evidence in another repo; never edit them.
- The line numbers in the change list are from a517288f. The run branch has moved since then; find the code by content.
- 3 other workers fix the rest of Tier 0 in parallel. Edit only your Own paths.
- Byte budget: each SKILL.md must stay at 12 KB or less (skills_catalog checks it). Put new detail in `references/` with a 1-line link.

## Steps

1. 0.1: In the 7 `teammates/video-*.md` files, keep "workers do not commit working media" but say finished deliverables are delivered by the orchestrator. In `skills/orchestrate/references/video-pipeline.md`, replace "Large media stays outside git" with a delivery step for the orchestrator: commit the finished deliverables through git LFS in 1 PR, check `git lfs ls-files` against the deliverable list, check the PR state before each push, and hand the operator 1 link. Operator quote for the reason: "The mp4's are missing ... THATS THE MOST IMPORTANT PART". Mirror it in `docs/recipes/video-pipeline.md` if that file states the old rule. Check: `grep -rn 'outside git\|Do not commit generated media' teammates skills/orchestrate docs/recipes` shows no line that forbids delivering the finished video.
2. 0.8: Delete the line in `teammates/video-critic.md` that has the critic name an owner or worker. The orchestrator routes each fix by file ownership. Check: the persona agrees with video-qa ("never name a worker").
3. 0.9: In `teammates/_base/fleet-orchestrator.md`, replace "usage limit ... tell the operator and go on" with: hold all spawns when the pool pace is over 1.5x, and never reap or replace a worker that shows the usage-limit message. Check: grep finds the new rule and not the old one.
4. 0.3 (pipeline line only): in `skills/orchestrate/references/video-pipeline.md`, replace any text that says a skipped check passes with: a check reports pass, fail or skip; a skip of a check that applies is a FAIL. (The qa script is another worker's.)
5. Run the Check.

## Constraints

- Keep the persona frontmatter unchanged. Keep each edit small; do not rewrite sections.
- `teammates/_base/fleet-orchestrator.md` is shared by every orchestrator: change only the usage-limit lines.

## Done when

- The Check passes, and each grep in the Steps gives the expected result.

## Report

Commit only your exact files (pathspec per file, never a directory). Run `horch ledger finish`, then `horch ledger done` with: each change ID (0.x), the file and the line it changed, and the self-test or test that proves it.
