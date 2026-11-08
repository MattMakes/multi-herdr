---
name: video-direction
description: "Use when a video project starts - a launch film, product demo, CLI walkthrough, concept explainer or short social cut - and you must fix the creative direction before anyone storyboards, captures, animates or records audio. Writes BRIEF.md (audience, 1 takeaway, format, loaded taste notes) and DIRECTION.md (3 directions with rendered key frames and a chosen line, then the palette with WCAG AA contrast, type scale, motion grammar, audio mood at -14 LUFS), reusing MOTION.md or BRAND.md tokens when the project has them. Picks a narrative archetype from references/archetypes.md."
---

# Video Direction

Direction is the set of decisions every later phase obeys. Make each one
once, write it down with a number, and do not let a later phase guess.

Output: `BRIEF.md` and `DIRECTION.md` at the root of the video project
pack. The storyboard (`video-storyboard`) reads both. Nothing is captured,
animated or recorded until both exist.

Apply the phone framing, proof, hero, and brand intro rules in
[`references/phone-framing.md`](references/phone-framing.md) before hand-off.

## Order of work

1. Read `.video/NOTES.md` in the video project folder when it exists.
   It holds taste notes from earlier films. Follow them unless the brief
   says otherwise.
2. Read the request and the product. Run the product or read its README
   when you can. A direction for a product you did not see is a guess.
3. Look for existing tokens: `MOTION.md`, `BRAND.md`, a design-system
   token file, or brand colors in the product's CSS. Reuse them (see
   "Reuse existing tokens").
4. Read `Format` from `PRODUCTION.md`: `short` or `long`. Write `BRIEF.md`.
5. Pick 1 archetype from `references/archetypes.md`. Its format label must
   match `Format`. Its duration range must contain the target duration.
6. Write 3 directions and render 1 key frame for each (see "Three
   directions"). Send them to the orchestrator and stop. The orchestrator
   or the operator picks 1. Write the pick as a `chosen:` line.
7. Write the rest of `DIRECTION.md` for the chosen direction.
8. Check every number in the "Direction checklist" below. Fix a value
   that fails before you hand off.

## BRIEF.md

The brief answers "who watches, and what must they remember". Keep it
under 1 page. Required sections:

| Section | Rule |
|---|---|
| Audience | 1 named group with 1 sentence on what they know already. "Developers" is too wide. "Backend developers who run CI on every pull request" is correct. |
| Core takeaway | Exactly 1 sentence of 15 words or fewer. Every scene supports it or is cut. |
| Proof | 1 to 3 concrete facts the video shows (a number, a before/after, a live run). |
| Call to action | 1 action with 1 destination (a URL, a command, a store link). |
| Format | Write the line `Format: short` or `Format: long`, from `PRODUCTION.md`. Then give aspect, resolution, frame rate and target duration, from the table below. |
| Channel | Where the video plays (landing page hero, social feed, docs page, talk, YouTube video page). For `short`, it sets sound-off defaults and the safe zones: assume muted. For `long`, the film plays on a YouTube page with captions only and no voice-over. |
| Constraints | Brand rules, banned claims, legal text, deadlines. Write "none" when there are none. |
| Loaded notes | 1 line: `Loaded notes: .video/NOTES.md` when you read it, or `Loaded notes: none` when it does not exist. |

Format values. Use only these:

| Aspect | Resolution | Use |
|---|---|---|
| `16:9` | `1920x1080` | Web hero, docs, talks, video sites |
| `9:16` | `1080x1920` | Vertical social feeds and stories |
| `1:1` | `1080x1080` | Square feed posts |

- Frame rate: `30` fps for talking, UI capture and slides. `60` fps for
  fast motion, scrolling capture and cursor-heavy demos.
- Target duration, `short`: `15s`, `30s`, `60s` or `90s`. Pick the
  shortest one that holds the proof.
- Target duration, `long`: 180 s to 900 s, with aspect `16:9` only. Pick
  the shortest length that holds the proof and 3 chapters or more.
- A length from 91 s to 179 s fits neither format. Ask the operator to
  choose 90 s (`short`) or 180 s or more (`long`).
- A 2nd aspect is a later reframe of the master inside 1 format, not a 2nd
  direction. A short cut of a long film is a separate production with
  `Format: short`.

## DIRECTION.md

`DIRECTION.md` is a token sheet plus short reasons. Each section holds
values a checker or a teammate can use without asking.

### Three directions

Before the build, show 3 different directions as real frames. The plan
gate (`check_timeline.py` of `video-storyboard`) reads this section and
fails when it is wrong. Write it in this exact form:

```markdown
## Directions

### D1: Calm drift
- Motion language: slow springs, long holds
- Material: paper cut-outs on a warm ground
- Depth: no
- Key frame: qa/directions/d1.png
- Palette: #F4F1EA #1D1F24 #E4572E
- Palette source: BRAND.md color tokens

### D2: ...
### D3: ...

chosen: D2
```

- 3 directions or more, headed `### D<n>: <name>`.
- No 2 directions share both motion language and material. Change at
  least 1 of the 2.
- Exactly 1 direction has `Depth: yes` (parallax, layered planes, a
  camera move in space). The others have `Depth: no`.
- Each direction has its own key frame: a PNG under `qa/directions/`.
- The palette lists hex colors. The source names where they come from:
  a brand file and token, a page of the product site, or `new`.
- `chosen: D<n>` names 1 listed direction. Write it only after the
  orchestrator or the operator picks.

Render a key frame: write 1 static HTML page per direction at the film
size, for example `directions/d1/index.html`, with the hook headline in
that direction's palette, type and material. Then screenshot it with
headless Chrome:

```bash
"$CHROME" --headless=new --disable-gpu --hide-scrollbars \
  --window-size=1920,1080 --screenshot=qa/directions/d1.png \
  "file://$PWD/directions/d1/index.html"
```

`$CHROME` is `CHROME_PATH`, or on macOS
`/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`. Open each
PNG and look at it before you send it. The sections below describe the
chosen direction only.

### Archetype and beats

Name the archetype and copy its beat list with times scaled to the
target duration. For `short`: state the hook pattern you picked and the
first 3 s in 1 sentence: what moves, and what headline text lands. For
`long`: state the cold open and the first 30 s in 1 sentence each: what
moves, and how the first 30 s prove the promise of the title.

### Palette

- 5 colors or fewer, each as a hex value with a role: `background`,
  `surface`, `text`, `accent`, `signal` (success or alert). Fewer is
  better.
- Every text-on-background pair the video uses must reach a WCAG AA
  contrast ratio of 4.5:1 or more. Write the ratio next to each pair, for
  example `text #F5F5F2 on background #111318 = 17.0:1`.
- Compute the ratio with WCAG relative luminance:
  `L = 0.2126 R + 0.7152 G + 0.0722 B` on linearized channels, and
  `ratio = (L_light + 0.05) / (L_dark + 0.05)`.
- Captions sit on a scrim (a dark band at 60% to 80% opacity) when the
  plate behind them is busy. Give the scrim color and opacity.
- A dark first frame looks like a broken video in a feed. Start on a lit
  frame unless the archetype needs a dark open. Then the storyboard sets
  `meta.allow_dark_intro: true`.

### Typography

- 2 font families or fewer: 1 display family for headlines, 1 text family
  for captions and labels. A monospace family for code counts as 1 of the
  2 unless it is the product's own UI in a capture.
- Sizes at `1080p` (the short side of the frame is 1080 px):
  headline 64 px or more, body and captions 36 px or more, labels 36 px
  or more. Scale linearly for other heights.
- Give weight, line height and letter spacing for each level.
- Fonts must be local files or system fonts. Give the file path or the
  system name. No runtime font download.
- All on-screen text (captions, labels, titles, cards) renders in Chrome
  as HTML and is composited as frames. The local ffmpeg has no text
  filter, so name fonts Chrome can load.

### Motion grammar

- 2 easing curves or fewer for the whole video. Name each with its
  cubic-bezier values and its use, for example
  `enter: cubic-bezier(0.16, 1, 0.3, 1)` for things that arrive and
  `move: cubic-bezier(0.65, 0, 0.35, 1)` for things that travel.
- Scene transitions last 250 ms to 450 ms. Pick 1 default and at most 2
  other transition types.
- Element entrances: 200 ms to 600 ms. Stagger related elements by 40 ms
  to 80 ms.
- Hold every text element long enough to read, after it settles: 1.0 s plus
  0.3 s for each word, and never under 1.5 s. The plan gate checks it.
- Cursor motion in captures follows the `move` curve. No teleporting
  cursor.
- State what never moves (logo position, caption band) so the eye has an
  anchor.

### Audio mood

| Token | Rule |
|---|---|
| `bpm` | 1 tempo, for example `112`. Scene cuts snap to its beat grid. |
| `key` | 1 musical key, for example `D minor`, for any generated bed or stingers. |
| `voice_persona` | `short` only. `long` has no voice-over: write `none`. Age range, pace, tone and 1 sentence of what it is not ("not a radio announcer"). |
| `voice_wpm` | `short` only. 130 to 160 WPM. The plan checker rejects more than 2.7 words/s. |
| `lufs_target` | `-14` LUFS integrated for the final mix. Narration sits 8 dB to 12 dB above the music bed. |
| `sfx` | The sound for clicks, key presses, reveals and transitions. Quiet, short, consistent. |
| `sound_off` | What carries the story with no sound (captions, on-screen text). `short`: assume a feed plays muted. `long`: captions carry the story, and game or product audio and music play under them. |

## Reuse existing tokens

When the project has `MOTION.md`, `BRAND.md` or a design-token file, the
video uses those tokens first:

- Copy each reused value into `DIRECTION.md` with its source file and
  token name, for example `accent #4F7CFF (BRAND.md: color.primary)`.
- Keep brand easing curves and durations when they fit the 2-curve and
  250 ms to 450 ms rules. When a brand value breaks a rule, keep the rule,
  and write the brand value and the reason for the change.
- Add new tokens only where the brand has none. Mark them `new`.
- A brand palette over 5 colors is cut down for the video. List which
  colors you keep and why.
- Brand contrast pairs still need 4.5:1. A brand pair that fails is not
  used for text.

## Direction checklist

Check each line before hand-off. Write the result in `DIRECTION.md`
under "Checks".

- [ ] Audience is 1 named group. Takeaway is 1 sentence of 15 words or fewer.
- [ ] `BRIEF.md` has a `Format: short` or `Format: long` line. The archetype label matches it.
- [ ] Aspect and resolution are 1 row of the format table. Frame rate is 30 or 60 fps. A `long` film is `16:9`.
- [ ] `short`: target duration is 15 s, 30 s, 60 s or 90 s and inside the archetype range.
- [ ] `long`: target duration is 180 s to 900 s and inside the archetype range.
- [ ] Palette has 5 colors or fewer. Every text pair is 4.5:1 or more, with the ratio written.
- [ ] Typography has 2 families or fewer. Headline 64 px or more, body 36 px or more at 1080p.
- [ ] Motion grammar has 2 easing curves or fewer. Transitions are 250 ms to 450 ms.
- [ ] Audio mood names bpm, key, voice persona, voice WPM and -14 LUFS.
- [ ] Every reused token names its source file.
- [ ] `short`: the first 3 s are described: what moves and what headline lands.
- [ ] `long`: the cold open and the first 30 s are described: what moves and how they prove the title's promise.
- [ ] 3 directions, exactly 1 with depth, each with a key frame PNG and a palette source. 1 `chosen:` line.
- [ ] `BRIEF.md` has a `Loaded notes:` line.

## Hand-off

Send the storyboard phase the paths of `BRIEF.md` and `DIRECTION.md`, the
archetype name, and any open risk (a product feature that is not ready
to capture, a missing brand file). Do not write `timeline.json`. That
belongs to `video-storyboard`.
