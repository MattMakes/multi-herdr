# 03: QA reports pass, fail or skip; an applicable skip, an empty scan and a stale report fail

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-qa/scripts/qa_video.py --self-test && skills/video-motion/scripts/scan_linear_motion.py --self-test`
**Own:** `skills/video-qa/`, `skills/video-motion/scripts/scan_linear_motion.py`, `skills/video-render/references/raster-sizes.md`

## Goal

A QA result is never a pass by default: checks have 3 states, and 0 samples, 0 files scanned or a stale report fail (0.3, 0.4, 0.5 qa part).

## Context

- Source: the lawn-mower-man video fleet retro, round 4 (83 session transcripts, episodes 1-6). The operator asked for Tier 0 to land before PR #344 merges. Read the Tier 0 table in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/multi-herdr-video-changes.md` and the rows for your items in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/video-fleet-retro.md` and `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/retro-2026-11/gaps-*.md` (lesson IDs in `candidates.md`). Those files are read-only evidence in another repo; never edit them.
- The line numbers in the change list are from a517288f. The run branch has moved since then; find the code by content.
- 3 other workers fix the rest of Tier 0 in parallel. Edit only your Own paths.
- Byte budget: each SKILL.md must stay at 12 KB or less (skills_catalog checks it). Put new detail in `references/` with a 1-line link.

## Steps

1. 0.3: In `qa_video.py`, replace `passed = not found` style results with a state of pass, fail or skip. A skip of a check that applies to the input is a FAIL; a check that does not apply is skip. `files_scanned: 0` is a FAIL. The headline prints "N pass / M skip / K fail". Fix `skills/video-qa/SKILL.md` text that says skip means pass. Self-tests for each case.
2. 0.4: An empty raster or size report (0 samples) is a FAIL in `qa_video.py` and in `scan_linear_motion.py`. Count canvas `drawImage` sources of a video as samples. Allow integer nearest-neighbour scales for pixel art. Update `skills/video-render/references/raster-sizes.md` (it says canvas draws of a video are not counted). Self-tests: 0 samples fails; a 3x nearest-neighbour pixel-art sample passes.
3. 0.5 (qa part): `qa_video.py` deletes the `--out` file first, and an exit 2 (input error) writes `passed: false` with the error. Self-test with an old passing report in place.
4. Run the Check.

## Constraints

- Keep the report fields other workers' scripts read; add the state field next to `passed` and keep `passed` a boolean that is true only for no fails and no applicable skips.

## Done when

- The Check passes with the new self-tests.

## Report

Commit only your exact files (pathspec per file, never a directory). Run `horch ledger finish`, then `horch ledger done` with: each change ID (0.x), the file and the line it changed, and the self-test or test that proves it.
