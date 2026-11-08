# 02: Replace designed payoff silence with an optional breakdown; end on the song's real ending; no stale audio report

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test skills_catalog --test video_pipeline_wave && skills/video-storyboard/scripts/check_timeline.py --self-test && skills/video-audio/scripts/check_audio.py --self-test && skills/video-audio/scripts/reconcile_timing.py --self-test`
**Own:** `skills/video-storyboard/`, `skills/video-audio/`

## Goal

No gate requires or exempts silence before the payoff; an optional `payoff_breakdown_sec` asks for a dip of at most 12 dB that is never silent (0.2). The audio skill ends on the song's real final phrase (0.7). check_audio.py never leaves a stale passing report (0.5, audio part).

## Context

- Source: the lawn-mower-man video fleet retro, round 4 (83 session transcripts, episodes 1-6). The operator asked for Tier 0 to land before PR #344 merges. Read the Tier 0 table in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/multi-herdr-video-changes.md` and the rows for your items in `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/video-fleet-retro.md` and `/Users/mascott/games/lmm-retro-wt/ai_docs/devblog/retro-2026-11/gaps-*.md` (lesson IDs in `candidates.md`). Those files are read-only evidence in another repo; never edit them.
- The line numbers in the change list are from a517288f. The run branch has moved since then; find the code by content.
- 3 other workers fix the rest of Tier 0 in parallel. Edit only your Own paths.
- Byte budget: each SKILL.md must stay at 12 KB or less (skills_catalog checks it). Put new detail in `references/` with a 1-line link.

## Steps

1. 0.2: In `check_timeline.py`, remove the rule that requires `payoff_silence_sec` when `bpm` is set, and remove the field. In `check_audio.py`, remove the 0.4 s quiet requirement and the automatic silence exemption. Add an optional `payoff_breakdown_sec` (timeline) that check_audio verifies as a level dip of 12 dB or less that is never silent (fail if the window goes below the silence floor the script already uses, or dips more than 12 dB). Update `reconcile_timing.py` and both SKILL.md field tables and any reference that names the old field. Self-tests: a timeline with bpm and no payoff field passes; a breakdown dip of 6 dB passes; a dip of 20 dB fails; a silent window fails; the old field is rejected or ignored with a warning (pick one and say which in DONE).
2. 0.7: In `skills/video-audio/SKILL.md`, replace "loop the bed and fade" with: use the song's real final phrase as the ending; a pad loop is a fallback only, when the song has no usable ending.
3. 0.5 (audio part): `check_audio.py` deletes the `--out` file before it starts, and writes `passed: false` with the error on an input error. Self-test: a run with a bad input over an existing passing report leaves `passed: false`.
4. Run the Check.

## Constraints

- These are user scripts in another project's fleet: keep the CLI flags that stay, and keep the JSON fields other than the removed one.
- Grep the whole repo for `payoff_silence` and report any hit outside your Own paths in DONE (do not edit it).

## Done when

- The Check passes, and `grep -rn payoff_silence skills/video-storyboard skills/video-audio` finds nothing, or only the rejection path.

## Report

Commit only your exact files (pathspec per file, never a directory). Run `horch ledger finish`, then `horch ledger done` with: each change ID (0.x), the file and the line it changed, and the self-test or test that proves it.
