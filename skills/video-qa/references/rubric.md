# Blind 8-axis review rubric

Score each axis with an integer from `1` to `5`. Score what the render shows.
A plan that promises a good result does not raise a score.

Verdict rule: `VERDICT: PASS` needs all 3 conditions:

1. `qa/render-check.json` has `passed: true`.
2. The average of the 8 scores is `>= 4.0`.
3. No axis has a score below `3`.

Any other result is `VERDICT: FAIL`. A score of `3` or lower on an axis
needs at least 1 fix line for that axis in the report.

## General score anchors

| Score | Meaning |
| :--- | :--- |
| 5 | Ready to publish. A viewer finds no defect on this axis. |
| 4 | Good. Small defects that most viewers do not see. |
| 3 | Acceptable. A defect that an attentive viewer sees but that does not break the message. |
| 2 | Weak. A defect that most viewers see, or that hides part of the message. |
| 1 | Broken. The axis fails its purpose. |

The anchors for each axis below make these levels concrete. When the evidence
fits 2 levels, give the lower score and write the reason in the report.

## Axis 1: Hook & First-Frame Read

Evidence: `qa/hook-strip.png`, frame 0, checks 4 and 5, `BRIEF.md`.

- 5: Frame 0 already shows the subject and a readable claim. Clear motion
  starts before `1.0 s`. The core promise is clear by `3.0 s`.
- 4: Frame 0 is meaningful. The promise is clear by `3.0 s`, but the first
  motion is late or soft.
- 3: Frame 0 is a plain backdrop or a logo. The promise appears at
  `2.0-3.0 s`.
- 2: The promise appears after `3.0 s`, or the opening text is not readable
  at `360 px`.
- 1: Frame 0 is blank or black, or nothing moves in `0.0-3.0 s`.

Owners: weak claim or order of beats - `video-director`; late or weak motion -
`video-animator`; blank frame 0 from the render - `video-editor`.

## Axis 2: Narrative & Pacing

Evidence: `qa/contact-sheet-360p.png`, `STORYBOARD.md`, scene durations in
`timeline.json`, narration audio.

- 5: Each scene adds 1 new idea. Scene lengths follow the narration. The end
  has a clear call to action.
- 4: The story is clear. 1 scene is a little long or a little rushed.
- 3: 2 or more scenes repeat an idea, or 1 scene holds a static view for
  longer than the narration needs.
- 2: The order of ideas is confusing, or text leaves the screen before a
  viewer can read it.
- 1: No clear message, or no call to action.

Owners: story, order and scene length - `video-director`; cut points -
`video-editor`.

## Axis 3: Typography & Safe Zones

Evidence: contact sheet tiles (`360 px` high), check 11, `DIRECTION.md` type
scale, the `9x16.mp4` variant when present.

- 5: All text is readable on every tile. Hierarchy is clear. No text or key
  object touches the caption zone (bottom `18%`) or the vertical UI zone.
- 4: All text is readable. 1 label is tight to an edge.
- 3: 1 secondary label is hard to read at `360 px`, or 1 line breaks badly.
- 2: Primary text is hard to read, low contrast, or partly in a safe zone.
- 1: Text is cut off, covered, or not readable.

Owners: type scale and contrast tokens - `video-director`; text layout in a
scene - `video-animator`; reframing of a variant - `video-editor`.

## Axis 4: Motion Craft & Physics

Evidence: contact sheet sequences, extracted frames around transitions,
check 1, check 2, check 7.

- 5: Every move has a purpose. Easing is consistent. Objects have weight:
  anticipation, follow-through, settle. No pops or jitter.
- 4: Motion is smooth. 1 move is linear or ends without a settle.
- 3: Mixed easing styles, or 1 visible pop, jump or stall.
- 2: Several pops or jitter, or motion that distracts from the message.
- 1: Motion is broken: frames jump back, objects teleport, or the scene does
  not render the same way twice.

Owners: scene and character motion - `video-animator`; transition timing -
`video-editor`.

## Axis 5: Product/Demo Clarity

Evidence: capture plates in the contact sheet, extracted frames, `BRIEF.md`.

- 5: The viewer can follow every product action. The cursor, clicks and zooms
  lead the eye to the result. UI text is readable.
- 4: All actions are clear. 1 zoom is a little late or a little loose.
- 3: 1 action is hard to follow, or the UI is too small at `360 px`.
- 2: The main feature is not visible, or the demo shows dead time or loading.
- 1: The product cannot be identified, or the demo shows an error state.

Owners: recording, cursor and zoom - `video-capture`; demo choice -
`video-director`. Mark `N/A` only when the brief has no product or demo; then
score `5` and write `N/A - no demo in brief` in the axis row.

## Axis 6: Character/Illustration Cohesion

Evidence: contact sheet, `MOTION.md` when present, `DIRECTION.md`.

- 5: Characters and illustrations keep the same proportions, line weight,
  palette and style in every scene. Idle motion is alive.
- 4: Consistent. 1 small change in scale or line weight.
- 3: 1 scene uses a different style, or a character is static for `> 3 s`.
- 2: Several style breaks, or broken rig parts (detached limbs, wrong pivots).
- 1: Characters change identity between scenes.

Owners: rig and character motion - `video-animator`; style tokens -
`video-director`. Mark `N/A` only when the video has no characters or
illustrations; then score `5` and write `N/A - no characters` in the axis row.

## Axis 7: Audio-Visual Sync

Evidence: narration and on-screen events at the extracted frames, checks 8
and 9, `audio/sync-sheet.json` when present (word times in
`vo_segments[].words[]`, beats in `beats_sec`, and `timing`: `estimated`
or `measured`).

- 5: Each visual reveal lands with its word or beat (within 1 frame by eye).
  Music, voice and effects are clear and balanced.
- 4: In sync. 1 reveal is a little early or late, or 1 effect is a little loud.
- 3: 1 reveal is clearly out of sync, or music covers 1 phrase of voice.
- 2: Several reveals are out of sync, or a click or pop at a cut.
- 1: Voice and picture do not match, or audio is clipped or missing.

Owners: mix, levels, pops, voice timing - `video-audio`; the position of a
reveal - `video-animator`; the position of a cut - `video-editor`.

## Axis 8: Brand & Aesthetic Cohesion

Evidence: contact sheet as a whole, `DIRECTION.md` palette and type tokens.

- 5: Every scene uses the declared palette, fonts and grid. The video looks
  like 1 piece.
- 4: Cohesive. 1 color or spacing value is off token.
- 3: 1 scene looks like it comes from a different video.
- 2: Several scenes break the palette or font rules.
- 1: No consistent look.

Owners: tokens and rules - `video-director`; a scene off token -
`video-animator`; color shift in the render - `video-editor`.

## Owner table

| Defect type | Owner |
| :--- | :--- |
| Story, script, scene order, scene length, tokens, type scale | `video-director` |
| Browser recording, cursor path, zoom, dead time in a plate | `video-capture` |
| Voice, music, effects, loudness, true peak, pops, sync of words | `video-audio` |
| Scene motion, character rig, text layout in a scene, determinism | `video-animator` |
| Cuts, transitions, compositing, encode, variants, captions files | `video-editor` |

Numeric check owners: checks 1 and 2 - `video-animator`; checks 3, 6 and 10 -
`video-editor`; check 4 - `video-animator` when the scene starts blank,
otherwise `video-editor`; check 5 - `video-animator`; check 7 -
`video-animator` for a scene, `video-capture` for a plate; checks 8 and 9 -
`video-audio`; check 11 safe zone - `video-director`, flashes -
`video-animator`.

## Report template: qa/critique-round-<N>.md

```markdown
# Critique round <N>

VERDICT: <PASS|FAIL>

Numeric gate: <passed|failed> (<k>/11 checks passed, qa/render-check.json)
Average score: <x.xx> / 5. Lowest axis: <axis name> (<score>).

| # | Axis | Score | Reason |
| :--- | :--- | :--- | :--- |
| 1 | Hook & First-Frame Read | <1-5> | <1 sentence> |
| 2 | Narrative & Pacing | <1-5> | <1 sentence> |
| 3 | Typography & Safe Zones | <1-5> | <1 sentence> |
| 4 | Motion Craft & Physics | <1-5> | <1 sentence> |
| 5 | Product/Demo Clarity | <1-5> | <1 sentence> |
| 6 | Character/Illustration Cohesion | <1-5> | <1 sentence> |
| 7 | Audio-Visual Sync | <1-5> | <1 sentence> |
| 8 | Brand & Aesthetic Cohesion | <1-5> | <1 sentence> |

## Fixes

[MM:SS.mmm] Scene <id>: <issue> -> <owner>: <fix>
```

Fix line rules:

- 1 defect for each line. Sort the lines by time.
- Put numeric gate errors first in time order with the other lines. Use the
  `timestamp` from `render-check.json`. For an error without a time, use
  `[00:00.000]` and the first scene ID.
- `<issue>` states what a viewer sees or what the check measured, with the
  number: "headline is 22 px high at 360 px", "freeze 2.10 s".
- `<fix>` states 1 action with a target value: "raise headline to 96 px",
  "move the reveal 0.20 s later to the downbeat at 04.000".
- For `VERDICT: PASS`, the Fixes section may hold polish lines. Mark each
  with `(optional)` after the fix.

Example lines:

```text
[00:00.000] Scene s01-hook: frame 0 is a flat navy background, claim appears at 01.200 -> video-animator: show the headline at frame 0 and start the logo move at 00.100
[00:07.500] Scene s03-demo: cursor jumps 600 px in 1 frame -> video-capture: re-record with the smooth cursor path between the 2 clicks
[00:12.033] Scene s04-arch: audio peak -1.8 dBFS within 20 ms of the cut -> video-audio: add a 10 ms fade at the cut and lower the whoosh by 4 dB
```

## Ranking mode: qa/ab-rank.md

Use this mode when the plan gives you 2 masters, `render/ab/A.mp4` and
`render/ab/B.mp4`, and asks for a rank. You do not know which direction made
which master. Do not try to find out: do not open `DIRECTION.md`, a fork
folder or `.video/`.

1. Watch both masters in full. Run the numeric gate on each, with its own
   output files, for `A` and then `B`:
   `python3 "<skill-dir>/scripts/qa_video.py" --video render/ab/A.mp4 --timeline timeline.json --out qa/ab-A-check.json --contact-sheet qa/ab-A-sheet.png --hook-strip qa/ab-A-hook.png`.
   Exit 1 does not stop the rank. Use the errors as evidence for both
   masters in the same way.
2. Score each master on the 8 axes with the anchors above.
3. Pick 1 master. A tie is not allowed: when the scores are equal, prefer
   the master whose core takeaway in `BRIEF.md` a viewer restates sooner.
4. Name the weakest shot of each master with its time.

Margin per axis: `A +<n>` when A scores `n` more, `B +<n>` when B scores
`n` more, `0` when equal.

```markdown
# A/B rank

PREFERENCE: <A|B>
Reason: <1 sentence, about what a viewer sees>

| # | Axis | A | B | Margin |
| :--- | :--- | :--- | :--- | :--- |
| 1 | Hook & First-Frame Read | <1-5> | <1-5> | <A +n, B +n or 0> |
| 2 | Narrative & Pacing | <1-5> | <1-5> | <margin> |
| 3 | Typography & Safe Zones | <1-5> | <1-5> | <margin> |
| 4 | Motion Craft & Physics | <1-5> | <1-5> | <margin> |
| 5 | Product/Demo Clarity | <1-5> | <1-5> | <margin> |
| 6 | Character/Illustration Cohesion | <1-5> | <1-5> | <margin> |
| 7 | Audio-Visual Sync | <1-5> | <1-5> | <margin> |
| 8 | Brand & Aesthetic Cohesion | <1-5> | <1-5> | <margin> |

Weakest shot A: [MM:SS.mmm] Scene <id>: <what a viewer sees>
Weakest shot B: [MM:SS.mmm] Scene <id>: <what a viewer sees>
```

Use only the labels A and B in the report. The producer checks it with
`ab_blind.py check-rank` of the `video-produce` skill. A rank gives no
`VERDICT:` line: the preferred master still goes through the critic loop.
