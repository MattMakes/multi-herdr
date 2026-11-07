# hark listen: find

Part of the listen spec. Index: `docs/specs/hark/listen.md`. The calibration
is in `docs/specs/hark/listen-calibration.md`.

## Purpose

`find` returns the best moments of one recording for a description, as
sample trims an editor can cut directly: one key press out of a typing take,
one bird call out of a meadow bed, one hit out of a multi-take sample pack.
A long file is searched everywhere, not only at its start.

## Interface

The CLI flags, the signature and the result fields are in the index. Segment
fields:

| Field | Meaning |
|---|---|
| `start`, `end` | the trim in file seconds, 3 decimals |
| `duration` | end - start, 3 decimals |
| `score` | the ranking score: z minus the cut penalty, 2 decimals |
| `z` | calibrated z of the query for this segment, 2 decimals |
| `cosine` | raw cosine, 3 decimals |
| `verdict` | from z |
| `kind` | `stroke`, `event` or `window` |
| `peak_dbfs` | stereo-decode sample peak of the segment, 1 decimal |
| `head_db` | level of the first 5 ms frame against the segment's peak, 1 decimal |
| `tail_db` | level of the last 10 ms against the segment's peak, 1 decimal |
| `cut` | `"head cut"`, `"tail cut"`, both joined by ", ", or `""` |

Text output: the summary, a blank line, then one numbered line per segment:
start and end (3 decimals), the start as `m:ss.ss`, the duration (3
decimals), z text, verdict, kind, peak (1 decimal), head and tail dB
(integers), and, for a cut segment, its ranking score with the cut words.

## Behaviour

### Candidate kinds

| Kind | What it is |
|---|---|
| `stroke` | one onset, from just before its attack until it has decayed by 35 dB, never into the next onset |
| `event` | strokes that belong together: a pair (gap under 0.25 s, such as the down- and up-stroke of one key) or a longer chain, at most 2 s, never across an edit gap |
| `window` | sliding windows that cover the whole range with no gaps |

An **edit gap** is 10 ms or more of exact digital silence (at or below
-100 dBFS): the silence between separate takes of a sample pack.

### Query kind

`kind='auto'` reads the query text:

- `event`: a one-shot query (a click, a hit, a key press).
- `texture`: a clearly continuous query (rain, room tone, music).
- `either`: in between.

| Mode | Candidates scored | Window edges | Edge judging | Window filters |
|---|---|---|---|---|
| auto, read `event` | strokes, events, windows | snapped to the sound | all candidates | yes |
| auto, read `either` | strokes, events, windows | plain grid | strokes and events only | no |
| auto, read `texture` | windows only | plain grid | none | no |
| `--kind event` | strokes and events (windows only when the recording has no strokes or events) | snapped | all candidates | yes |
| `--kind texture` | windows only | plain grid | none | no |

"Window filters": a window that holds 2 or more separate sounds is never
returned, and a window is replaced by the best stroke or event inside it
when that one scores within 0.5 z (a tighter trim of the same moment).

"Edge judging": a candidate whose first 5 ms or last 10 ms are above -15 dB
against its own peak is cut into a sound. It is flagged `head cut` /
`tail cut` and ranked 0.5 z lower. Plain windows of `either` and `texture`
queries are not judged: they are cut anywhere by design and are meant to be
crossfaded.

### Trims

- A stroke or event starts 5 ms before its measured attack.
- A snapped window starts 5 ms before its sound and ends 20 ms after the
  sound has decayed.
- With `--start` / `--end`, only segments whose attack lies inside the range
  are returned. The audio is read from 0.1 s before `start`, so an attack at
  the boundary is seen whole.

### Speed

Every candidate costs one CLAP window (0.3 to 1 s on a CPU in the source's
measurements). Candidates do not depend on the query and their embeddings are
cached, so a second query on the same file is fast (1.2 s against 29.9 s
cold for an 18 s file in the source's timing run).

## Algorithms

All envelope work uses 5 ms frames. `env` = 20 log10(max(frame RMS, 1e-12))
of the mono decode, with non-overlapping frames of 240 samples. `floor` =
the 10th percentile of `env` over the decoded audio. Frame index of time t =
floor(t / 0.005) unless the step says "round".

### F1. Setup

1. If the query is empty or blank: value error "no query given".
2. Check the range (calibration part). s0 = start or 0. s1 = min(end,
   duration) when end is given, else the duration.
3. m0 = max(0, s0 - 0.1). Decode mono audio of `[m0, s1)`. All candidate
   times are local to m0. off = s0 - m0. d = decoded length in seconds.
4. Query embedding (templates, A1). Query kind (F2).
5. use_events = kind is `event`, or kind is `auto` and the read kind is not
   `texture`. prefer_events = kind is `event`, or kind is `auto` and the read
   kind is `event`.
6. Compute `env` and `floor` of the local audio.

### F2. Query kind

1. q = the query's text embedding (with templates).
2. e = mean of the text embeddings of the 6 event anchors; t = mean of the
   6 texture anchors (both with templates, means not renormalised).
3. score = q · e - q · t.
4. Kind: `event` when score >= -0.08; `texture` when score <= -0.27; else
   `either`. Report the score rounded to 3 decimals.

### F3. Window candidates

Inputs: local end d, window `win` and hop `hop` (null for defaults), local
start t0 = off. span = d - t0.

1. Default window: win = clamp(span / 6, 1, 5) s. While win < 10 and
   (span - win) / (win / 2) + 1 > 40: win = min(10, 1.25 × win).
2. If span <= win: one window `[t0, d)`.
3. Default hop: hop = win / 2. If (span - win) / hop + 1 > 40:
   hop = min(win, (span - win) / 39).
4. Starts: t0, t0 + hop, ... while start <= d - win (tolerance 1e-6). If the
   last window ends more than 1 ms before d, add a start at d - win.
5. Windows have priority 0.

So the default keeps about 40 windows at most: the window grows to 10 s
first, then the hop grows up to the window length, before the count grows.

### F4. Window edge snapping (prefer_events only)

Inputs: a window `[a, b)`, search bounds lo = off, hi = d. Frame indices by
rounding. step = 200 frames (1 s). w = 6 frames.

1. i0 = round(a / 0.005); i1 = max(i0 + 1, round(b / 0.005)), at most the
   frame count. If i0 >= i1 - 1, return the window unchanged.
2. P = max(env[i0:i1]). Quiet level q = max(floor + 6, P - 40). Decay level
   qe = max(floor + 4, P - 35). Both stay fixed for this window.
3. Bounds in frames: nlo = max(0, round(lo / 0.005)), nhi = min(frame count,
   round(hi / 0.005)).
4. **Start.** When env[i0] > q (the window starts inside sound):
   - The start is "inside a falling tail" when i0 - w >= nlo and the mean of
     the 6 frames before i0 exceeds the mean of the 6 frames from i0 by more
     than 1 dB.
   - Falling tail: search forward from i0 (at most to i1 - 2 and at most 1 s)
     for the first frame at or below q. If found and at least 0.1 s of window
     remain after it, move i0 there (start after the earlier sound's tail).
   - Otherwise (rising or sounding): search backward from i0 (at most 1 s,
     not before nlo) for the first frame at or below q. If found, move i0
     there (go back to where it is quiet).
5. **End.** last = i1 - 1. When env[last] > qe (the window ends inside
   sound):
   - The end is "at a new attack" when last + 1 + w <= nhi and the mean of
     the 6 frames after last exceeds the mean of frames max(i0, last - 6) ..
     last by more than 1 dB.
   - New attack: search backward from last (down to i0 + 1) for the first
     frame at or below q. If found and the window keeps at least 0.1 s, end
     the window just after that frame (stop before the new attack).
   - Otherwise (still decaying or sounding): walk forward from last while
     the next frame is inside nhi and within 1 s of last, the current frame
     is above qe, and the next frame is at most 6 dB above the lowest level
     seen so far in this walk (so the walk never climbs into a new attack).
     End the window just after the frame where the walk stops.
6. **Tighten.** In the frames `[i0, i1)`: if any frame is above q, move i0 to
   one frame before the first such frame (never before i0). Then, if any
   frame of the new `[i0, i1)` is above qe, set i1 to just after the last
   such frame.
7. Return a' = max(lo, i0 × 0.005), b' = min(hi, i1 × 0.005 + 0.02) (the
   20 ms is added only when i1 × 0.005 < hi).

After snapping, remove near-duplicate windows (F7).

### F5. Stroke and event candidates

Input: the local mono audio. Output: `(start, end, kind, priority)`.

1. Onsets: the core-io onset detector (seconds). No onsets: no candidates.
2. env1 = the same envelope with 1 ms frames.
3. **Strokes.** For each onset o with next onset nx (the local end d for the
   last onset):
   1. Frames f0 = floor(o / 0.005), f1 = max(f0 + 1, floor(nx / 0.005)).
      Skip the onset if this frame range is empty.
   2. Peak frame pk = the loudest frame within the first 60 ms (13 frames)
      of the range.
   3. Decay end: the first frame from pk to f1 whose level is below
      max(env[pk] - 35, floor + 4). end = that frame's time + 0.02 s. If
      there is none, end = nx.
   4. Attack start: in env1, look at frames within ±30 ms of o. The attack is
      the first 1 ms frame whose level reaches max(floor + 10, local max -
      30) (floor is the 5 ms floor). start = max(0, attack - 0.005).
   5. Never start before the end of the previous kept stroke.
   6. end = min(end, nx - 0.002 (or d for the last onset), start + 2.0).
   7. Keep the stroke when end - start >= 0.02 s. Prominence =
      env[pk] - floor.
4. Every stroke is a candidate of kind `stroke` with priority = prominence.
5. **Joins.** Two consecutive strokes a, b are joined when b.start - a.end
   < 0.25 s and there is no edit gap between a.start and b.start (F8).
6. **Pairs.** For each joined consecutive pair with b.end - a.start <= 2 s and
   a.prominence >= b.prominence - 15 dB (a pair is never led by a stroke
   15 dB weaker): an `event` from a.start to b.end with priority =
   max of the 2 prominences.
7. **Chains.** Walk the strokes from the first. From stroke i, extend while
   the next stroke is joined and the chain would still end within 2 s of
   stroke i's start. Drop leading strokes whose prominence is more than 15 dB
   below the chain's top prominence. If more than 2 strokes remain: an
   `event` from the first remaining start to the last end, priority = top
   prominence. Continue after the chain.
8. **Isolation bonus.** For every candidate: it is isolated when the mean of
   the 6 frames (30 ms) before its start is at most floor + 10 dB (or there
   are no frames before it) and its tail level (F8) is at most -25 dB. An
   isolated candidate gets +10 priority.
9. **Selection** (when a cap n is given; F6).

### F6. Round-robin selection

Cap n = 48. Bucket of a candidate = floor(start / 2 s). Within each bucket,
sort by priority descending. Then take one candidate from each bucket in
bucket order, repeat the round, until n are taken or all are used. So the
picks cover the whole time range: isolated, prominent moments first in each
stretch.

### F7. Near-duplicate removal

Walk the candidates in order. Keep a candidate unless an already kept one
overlaps it by at least 0.9 × the extent of the two together (extent = later
end - earlier start).

### F8. Edge levels and edit gaps

- Segment frames of `[a, b)`: from floor(a / 0.005) to max(that + 2,
  floor(b / 0.005)).
- tail level = max of the last 2 frames - max of all frames (dB, <= 0).
- head level = the first frame - max of all frames.
- Empty frame range: 0.
- Edit gap in `[a, b)`: 2 or more consecutive frames at or below -100 dB.

### F9. Multiple separate sounds in a window

A window `[a, b)` (frames by rounding) holds 2 or more separate sounds when:

1. It has at least 3 frames, and
2. at least 2 frames are above its quiet level q (F4 step 2), and
3. between the first and the last frame above q there is either an edit gap,
   or 50 or more consecutive frames (0.25 s) at or below q.

### F10. Scoring and selection

1. Windows (F3), snapped and de-duplicated when prefer_events. Remember the
   window count and the first window's length for the summary.
2. Score every window: embed `[m0 + a, m0 + b)` as one span (A3), then
   calibrate against the query (A6): z and cosine.
3. When use_events:
   - All stroke and event candidates (F5, no cap) whose start + 5 ms lies at
     or after off (with 1 ms tolerance): their attack is inside the range.
   - Searched span s1 - s0 up to 60 s: one stage. Select 48 (F6).
   - Longer: two stages. Take the 6 best windows by z. Keep only candidates
     that overlap one of them. Select 48 (F6).
   - De-duplicate (F7). Score each like a window.
4. With `--kind event` and at least 1 stroke or event: drop all windows.
5. Candidates = strokes and events, then windows. Round z to 2 decimals.
6. Head and tail levels (F8) for every candidate. A candidate is judged when
   use_events and (prefer_events or it is not a window). cut = judged and
   (tail > -15 dB or head > -15 dB). score = z - 0.5 × cut.
7. When prefer_events, a window is excluded from the answer when
   - some stroke or event lies inside it (start >= window start - 50 ms and
     end <= window end + 50 ms) and the best such z >= the window's z - 0.5,
     or
   - there are strokes or events at all and the window holds multiple sounds
     (F9).
8. File loudness: the stereo-decode peak of `[m0, s1)`. The file is "loud"
   when the peak >= -70 dBFS.
9. Walk the candidates by score descending (excluded ones never). Take a
   candidate when it overlaps no taken candidate (1 ms tolerance) and, for a
   loud file, its own stereo peak is at least -70 dBFS (a near-silent
   stretch of a sounding file is never an answer). Stop at `top`.
10. Output times = m0 + local time.

### F11. Summary

Facts, in order:

1. How the search ran: the query-kind reading (auto) or `kind=K`; the range
   ("all N s" or "m:ss.ss-m:ss.ss (N s)"); the number of windows and their
   approximate length; "edges snapped to the sound" when prefer_events; when
   the windows were dropped, that they only located events; with
   use_events, the number of onset-based candidates and "one stage" or "two
   stages: events inside the 6 best windows".
2. With segments: each segment's time span, duration, kind, z text,
   verdict and cut words. Then the trim rule: strokes and events start 5 ms
   before the attack; windows are snapped (prefer_events) or are plain
   stretches to crossfade; a head or tail cut trim cuts into a sound.
3. When the best segment is `weak` or `no match`: the recording may not
   contain this sound.
4. Without segments: no candidate segments.
5. When the searched audio's peak is below -70 dBFS: a near-silence warning
   first.

## Constants

| Name | Value | Meaning |
|---|---|---|
| envelope frame | 5 ms | |
| attack frame | 1 ms | attack refinement |
| pre-roll | 5 ms | trims start this far before the attack |
| margin | 0.1 s | audio read before `start` |
| event cap length | 2.0 s | max stroke, pair and chain length |
| join gap | 0.25 s | strokes closer than this can form an event; also the quiet gap of F9 |
| decay depth | 35 dB | stroke end, window decay level |
| stroke floor margin | 4 dB | stroke end is at least floor + 4 |
| stroke end pad | 20 ms | added after the decay frame |
| next-onset guard | 2 ms | stroke ends this far before the next onset |
| minimum stroke | 20 ms | |
| peak search | 60 ms | after the onset |
| attack search | ±30 ms | around the onset |
| attack level | max(floor + 10, local max - 30) dB | |
| weak lead | 15 dB | a pair or chain is never led by a stroke this much weaker |
| isolation | pre-level <= floor + 10 dB over 30 ms and tail <= -25 dB | +10 priority |
| candidate cap | 48 | strokes and events |
| bucket | 2 s | round-robin selection |
| window count target | 40 | |
| default window | span / 6, clamped 1 to 5 s, grows by 1.25 up to 10 s | |
| default hop | window / 2, grows up to the window | |
| one-stage limit | 60 s | longer searches use 2 stages |
| best windows (stage 2) | 6 | |
| refine margin | 0.5 z | window replaced by a stroke / event inside it |
| inside tolerance | 50 ms | |
| cut level | -15 dB | head or tail above it = cut |
| cut penalty | 0.5 z | |
| edit gap | >= 10 ms at <= -100 dBFS | |
| snap reach | 1 s | max edge move |
| quiet level | max(floor + 6, window peak - 40) dB | |
| decay level | max(floor + 4, window peak - 35) dB | |
| slope test | 6 frames, 1 dB | falling / rising tests |
| climb guard | 6 dB | decay walk stops when the level climbs |
| minimum snapped length | 0.1 s | |
| snapped end pad | 20 ms | |
| de-duplicate overlap | 0.9 | |
| near-silence | -70 dBFS | |
| query kind: event | score >= -0.08 | |
| query kind: texture | score <= -0.27 | |

Anchor phrases of the query-kind reader (see index Q2):

| Event anchors | Texture anchors |
|---|---|
| a single short sound effect | a continuous sound |
| one short hit | a long background atmosphere |
| a click | a steady drone |
| a single impact | a piece of music |
| one short pop | ambient room noise |
| a quick one-shot sound | a soundscape |

Threshold rule (re-run it when the anchors, the templates or the CLAP model
change): on labelled "tune" query texts (clear one-shot and clear continuous
class phrasings), the event threshold = the lowest score of the one-shot
texts, rounded down to 2 decimals; the texture threshold = the median score
of the continuous texts. So every tune one-shot reads as `event`, and only
clearly continuous texts skip the onset candidates.

## Errors and edge cases

- Empty or blank query: value error.
- Bad range: the range errors of the calibration part.
- A recording without onsets: no strokes or events; windows only, even with
  `--kind event`.
- A recording shorter than the default window: one window over the whole
  range.
- With `--start`, a stroke that begins in the 0.1 s margin is dropped (its
  attack is outside the range); a window never starts before `start`.
- `--win` given without `--hop`: the hop default (and its growth rule)
  still applies.
- Near-silent file: candidates are scored and returned; the summary warns.
- A sounding file: a candidate whose own peak is below -70 dBFS is never
  returned.
- All candidates excluded: "no candidate segments".

## Acceptance tests

Fixtures are built in the test from code (no recordings needed unless marked
**[model]**). `clicks.wav`: 6 s of room-tone-like noise at -60 dBFS RMS with
a 3 ms click (peak -6 dBFS, exponential decay 40 ms) at 0.5, 1.5, 2.5, 3.5,
4.5 s.

- **FND-1** Strokes land on clicks. `clicks.wav`, F5. Properties: 5 strokes;
  each starts within 0 to 15 ms before its click and ends before the next
  click; all are isolated (priority >= prominence + 10).
- **FND-2** Down- and up-strokes pair. Clicks at 1.00 s and 1.12 s (same
  level), and at 3.00 s and 3.12 s. Properties: 2 `event` candidates, each
  spanning its 2 strokes; no event spans 1.00 s to 3.12 s.
- **FND-3** Weak lead. A click at 1.00 s at -40 dBFS then one at 1.10 s at
  -6 dBFS. Property: no pair event starts at the weak click.
- **FND-4** Edit gap. Two clicks 0.1 s apart with 20 ms of exact zeros
  between them. Property: no event joins them.
- **FND-5** Round-robin. 200 candidates in 20 s with priorities rising with
  time; select 48. Properties: every 2 s bucket that has candidates is
  represented; within each bucket the highest-priority ones are taken first.
- **FND-6** Windows cover everything. For spans 3, 7.5, 30, 61, 300 and 900 s
  with defaults: the first window starts at t0, the last ends at the span
  end, each window starts at or before the previous end (no gaps), every
  window has the same length, length <= 10 s. The count is at most 41 for
  spans up to 400 s. At 900 s the window is 10 s, the hop is 10 s and the
  count is 90 (the hop never exceeds the window). With span 3 s: 1 window.
  With win 2 and hop 2 on 7.5 s: starts 0, 2, 4, 5.5.
- **FND-7** De-duplication. Candidates (0, 1), (0.02, 1.01), (2, 3) ->
  (0, 1) and (2, 3) remain.
- **FND-8** Edge levels. A 1 s segment that decays to -40 dB at its end ->
  tail <= -30; a segment ending at the peak of a sine -> tail > -15 (cut);
  an empty range -> 0.
- **FND-9** Snapping. A window [1.48, 2.0) over `clicks.wav` (it starts 20 ms
  before the click at 1.5 s) -> snapped start in [1.48, 1.50], end within 0.3 s
  of the click's decay plus 20 ms. A window that starts 30 ms after a click
  (inside its tail) -> the start moves forward past the tail.
- **FND-10** Multiple sounds. A window spanning 2 clicks 1 s apart -> F9 true;
  a window around one click -> false.
- **FND-11** **[model]** End to end. `clicks.wav`, query "a click".
  Properties: `query_kind` is `event`; 5 segments at most; no 2 segments
  overlap; each segment contains one click; no segment has kind `window`
  holding 2 clicks.
- **FND-12** **[model]** Query kind. "a single mechanical keyboard key press"
  -> `event`; "heavy rain on a roof" -> `texture` or `either`, never
  `event`; "a piece of music" -> `texture`.
- **FND-13** **[model]** Range. `find(clicks.wav, "a click", start=2.0,
  end=4.0)` -> every segment starts at or after 1.995 s and before 4.0 s;
  the clicks at 2.5 and 3.5 s are found; `duration` == 2.0.
- **FND-14** **[model]** Long file, two stages. A 300 s file of room tone
  with one glass tap at 150 s and one at 275 s. Query "a glass ting", top 3:
  both taps are inside returned segments, as `stroke` or `event` trims; the
  summary says two stages.
