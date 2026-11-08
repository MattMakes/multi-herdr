# 04: Encoder facts: no -shortest, prefer aac_at, +4 dB margin, CFR select recipe

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && node --check skills/video-render/scripts/render_frames.mjs && node --check skills/video-render/scripts/assemble_master.mjs`
**Own:** `skills/video-render/scripts/render_frames.mjs`, `skills/video-render/scripts/assemble_master.mjs`, `skills/video-render/references/encoded-gates.md`, `skills/video-render/references/ffmpeg-compositing.md`, `skills/video-render/SKILL.md`

## Goal

The render scripts and references carry change 0.6.

## Context

- Source: the lawn-mower-man video fleet retro, round 4 (83 session transcripts, episodes 1-6). The operator asked for Tier 0 to land before PR #344 merges. Read the Tier 0 table in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/multi-herdr-video-changes.md` and the rows for your items in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/video-fleet-retro.md` and `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/retro-2026-11/gaps-*.md` (lesson IDs in `candidates.md`). Those files are read-only evidence in another repo; never edit them.
- The line numbers in the change list are from a517288f. The run branch has moved since then; find the code by content.
- 3 other workers fix the rest of Tier 0 in parallel. Edit only your Own paths.
- Byte budget: each SKILL.md must stay at 12 KB or less (skills_catalog checks it). Put new detail in `references/` with a 1-line link.

## Steps

1. Remove `-shortest` from the mux in `render_frames.mjs`; pad the audio to the video length instead (`apad` with `whole_dur`, or equivalent), so the last frames are never cut.
2. In `render_frames.mjs` and `assemble_master.mjs`, prefer the `aac_at` encoder on macOS when `ffmpeg -encoders` lists it, and fall back to native `aac`. The loudness gate measures the final MP4, not the pre-encode mix.
3. In `references/encoded-gates.md`, change the encode margin text from "+1 dB" to "up to +4 dB" (native aac raised true peak by up to 4 dB in the retro).
4. In the `select between` recipe (find it with grep; likely `ffmpeg-compositing.md` or encoded-gates.md), add `-fps_mode cfr -r 60`.
5. If either script has a self-test or a dry-run mode, run it; if ffmpeg is installed, render a 2 s smoke clip to /tmp and confirm with ffprobe that the audio and video durations match within 1 frame. Report the result.

## Done when

- The Check passes, and `grep -n shortest skills/video-render/scripts/*.mjs` finds nothing.

## Report

Commit only your exact files (pathspec per file, never a directory). Run `horch ledger finish`, then `horch ledger done` with: each change ID (0.x), the file and the line it changed, and the self-test or test that proves it.
