# 08: Bring skills/video-audio/SKILL.md under the 12 KB skill budget

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `opus` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog && cargo test -p horch-core --test video_pipeline_wave`
**Own:** `skills/video-audio/SKILL.md`, `skills/video-audio/references/long-form.md`

## Goal

`skills/video-audio/SKILL.md` is 12288 bytes or less, and both check tests pass.

## Context

CI on PR #346 fails `skills_bundled_size_budget` in `crates/horch-core/tests/skills_catalog.rs` (limit `MAX_SKILL_MD_BYTES` = 12 * 1024). T-5 of this run added long-form rules, and the file is now 13465 bytes. The regression test `vpw_08_every_video_skill_keeps_the_short_and_long_split` in `crates/horch-core/tests/video_pipeline_wave.rs` requires that SKILL.md still contains `short`, `long` and `meta.format`.

## Steps

1. Run `wc -c skills/video-audio/SKILL.md`. Record the size.
2. Move the detailed long-form rules (chapter beds, crossfades, end-screen tail, 2-pass loudnorm note) to a new file `skills/video-audio/references/long-form.md`. Keep each rule unchanged.
3. In SKILL.md, keep 1 short paragraph: read `meta.format` first; a `long` film has no voice-over by default; for `long`, read `references/long-form.md`. Keep the words `short`, `long` and `meta.format`.
4. Run `wc -c skills/video-audio/SKILL.md`. Check: 12288 or less, with at least 300 bytes of margin.
5. Run the check command. Check: both tests pass.

## Constraints

- Write in Simplified Technical English.
- Do not delete a rule. Move it.
- Do not use `ANTHROPIC_API_KEY`.

## Done when

- SKILL.md is 11988 bytes or less, and the check command passes.

## Report

Send the DONE summary with `horch ledger done`. State the size before and after.
