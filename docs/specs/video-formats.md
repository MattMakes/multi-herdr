# Video formats: short form and long form

Status: decided by the operator on 2026-11-08. Run: `video-formats`.

## Problem

Every fleet video skill treats each video as a muted social-feed clip of 90 s or less. The 2026-11-08 monuments devlog run asked for 6 YouTube episodes and 3 shorts. The fleet made 6 episodes of about 74 s with feed rules. The episodes had to be long form.

This spec adds 1 required field, `format`, with 2 values. Every video skill reads it. Every plan gate checks it.

## The 2 formats

| Rule | `short` | `long` |
|---|---|---|
| Where it plays | Social feeds (YouTube Shorts, TikTok, Reels), landing page hero, docs page, talk | A YouTube video page or a channel episode. The viewer clicked a title and a thumbnail. |
| Length | 15 s to 90 s. Targets: 15, 30, 60, 90 s. | 180 s to 900 s (3 to 15 min). No fixed targets. |
| Aspect | `16:9`, `9:16` or `1:1`. A feed cut uses `9:16`. | `16:9` only. |
| Sound | Assume muted. Burned-in captions carry the whole story. | Captions only by default: burned-in captions carry the story, with no voice-over. The game or product audio and music play under them. |
| Opening | Motion in the first frame. Hook before 1.5 s. No logo intro. | Motion in the first frame. The first 30 s prove the promise of the title and the thumbnail. A studio intro of 5 s or less may follow the cold open. |
| Pacing | Cut every 2 s to 3 s. | Chapters of 30 s to 180 s. Each chapter opens with its own hook caption. Shot length varies: fast montage, then room to breathe at each payoff. |
| Structure | 1 hero moment. | 3 or more chapters. Each chapter has 1 payoff. The final chapter holds the main hero moment. |
| Music | 1 bed. | 1 bed for each chapter or group of chapters. A single song never loops for the whole film. |
| Ending | 1 CTA on screen. The last frame works as a still. Optional loop. | 1 CTA, then a YouTube end screen of 5 s to 20 s with clear space for the end-screen elements. No loop. |
| Packaging | Title and cover frame. | 3 title options, 1 thumbnail concept, chapter timestamps (`chapters.txt`) for the description. |

A short cut of a long film is a separate production with `format: short`. It has its own script, timeline and review. It is not a reframe of the long master. A reframe to a 2nd aspect is allowed only inside 1 format.

## Choosing the format

The producer sets `format` from the operator's words:

- "short", "Shorts", "TikTok", "Reels", "vertical", "feed", "teaser", "landing page", "hero video": `short`.
- "long", "episode", "devlog episode", "YouTube video", "full video", any length of 3 min or more: `long`.
- A request for "N videos and M shorts": the N videos are `long` and the M shorts are `short`.
- Any other request: ask the operator. Do not guess.

A request for a length from 91 s to 179 s fits neither format. Ask the operator to choose 90 s (`short`) or 180 s or more (`long`).

## Fields (the shared contract)

All skills use these exact names.

`timeline.json`:

- `meta.format`: required. `"short"` or `"long"`.
- `meta.chapters`: required when `meta.format` is `"long"`. Absent when it is `"short"`. A list of `{"id": str, "title": str, "start_sec": number, "hook": str}`. The first `start_sec` is `0`. The list is strictly increasing. Each chapter runs to the next `start_sec`, or to `meta.duration_sec`. Each chapter is 30 s to 180 s. There are 3 chapters or more.
- `meta.end_screen_sec`: required when `meta.format` is `"long"`. A number from 5 to 20. The last `meta.end_screen_sec` seconds are the end screen.

`PRODUCTION.md` and `BRIEF.md`: a line `Format: short` or `Format: long`.

Batch manifest (`video-produce/scripts/validate_batch.py`): each film has `"format"`. A `short` film has `length_sec` in `15, 30, 60, 90`. A `long` film has `length_sec` from `180` to `900` and `aspects` equal to `["16:9"]`.

## Gates

`video-storyboard/scripts/check_timeline.py` fails a timeline with these errors:

- `meta.format` is absent or not `short` or `long`.
- `short`: `meta.duration_sec` is outside 15 s to 90 s (+-0.05 s), or `meta.chapters` is present.
- `long`: `meta.duration_sec` is outside 180 s to 900 s, `meta.aspect` is not `16:9`, `meta.chapters` breaks a rule in "Fields", or `meta.end_screen_sec` is absent or outside 5 s to 20 s.

`video-qa/scripts/qa_video.py` checks `meta.format` on the encoded master: the master length must fit the format range. The `loop_seam` gate runs only for `short`.

The critic reads `meta.format`. For `long`, axis 1 judges whether the first 30 s prove the title and the thumbnail, and axis 2 judges the chapter re-hooks and the pacing over the full runtime.

## Regression lock

`crates/horch-core/tests/video_pipeline_wave.rs` keeps a test that fails when a video skill loses the format split. It checks that each skill file in this list names both `short` and `long`, and names `meta.format` where it reads the timeline:

- `skills/video-produce/SKILL.md`
- `skills/video-produce/references/promo-brief.md`
- `skills/video-direction/SKILL.md`
- `skills/video-direction/references/archetypes.md`
- `skills/video-storyboard/SKILL.md`
- `skills/video-qa/SKILL.md`
- `skills/video-audio/SKILL.md`
- `skills/video-render/SKILL.md`
- `skills/orchestrate/references/video-pipeline.md`

The checker self-tests carry a case for each format: a 74 s `long` timeline fails, and a 300 s `long` timeline with valid chapters passes.
