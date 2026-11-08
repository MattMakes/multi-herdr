# Audio review rubric

Score each axis from 1 to 5. A 5 needs both the ear and the meter to agree.
A meter reading outside a BLOCKER limit caps the axis at 1, whatever the ear
says. `hark measure` bands: sub 20-60 Hz, low 60-250, lowmid 250-1k,
mid 1k-4k, presence 4k-8k, air 8k-16k, top 16k-24k (`bands_db` is the power
share of each band in dB).

## A1 Clarity on phone and laptop speakers

Phone speakers give almost nothing under 300 Hz. Laptop speakers give little
under 150 Hz. A sound that lives in `sub` and `low` disappears on them.

| score | anchor |
|---|---|
| 5 | The sound reads at once without bass. `lowmid` + `mid` + `presence` hold most of the power, and the attack sits in `mid` or `presence`. |
| 4 | Reads without bass. Some weight is lost, but the identity stays. |
| 3 | Reads, but soft or dull: `brightness_hz` low for its class, or the identity leans on `low`. |
| 2 | Mostly in `sub` and `low`. On a phone only a faint tick is left. |
| 1 | Nothing in `lowmid` and above: inaudible on a phone. |

## A2 Timing to picture or game event

The reference is the cue file (video) or the event list (game). Video: 1 frame
is 33 ms at 30 fps, 42 ms at 24 fps. Game: input feedback must start within
10 ms of the file start.

| score | anchor |
|---|---|
| 5 | Every cue within 1 frame (`offset_ms`); game takes: leading silence at most 1 ms, 20 dB under the peak within 5 ms. |
| 4 | Every cue within 2 frames; sync clearly beats `chance`. |
| 3 | Most cues within 100 ms; 1 or 2 drift. |
| 2 | Several cues late or early by more than 100 ms, or sync near `chance`. |
| 1 | A cue with no sound, or a game take with more than 10 ms of leading silence. |

Place rule: a hit, click or key by its onset on the cue; a whoosh by its peak
on the apex; a riser by its end on the hit.

## A3 Loudness and peaks

Video master: -14 LUFS integrated, true peak at most -1 dBTP. Game assets:
1 peak ceiling per asset class, not -14 LUFS; the engine mixes at run time.

| score | anchor |
|---|---|
| 5 | On target within 1 LU; `true_peak_dbtp` at most -1; `clipped_samples` 0; no jump that the ear notices. |
| 4 | Within 2 LU; peaks safe. |
| 3 | Off by 2-4 LU, or 1 element jumps out by ear. |
| 2 | Off by more than 4 LU, or `lra_lu` too wide for the medium (over 8 LU for a short film). |
| 1 | `clipped_samples` over 0, or `true_peak_dbtp` over the ceiling. |

## A4 Masking

Masking is 1 sound hiding another in the same band at the same time: SFX over
voice in `lowmid`-`mid`, 2 hits in 1 frame, a bed that fills `presence` under
UI clicks.

| score | anchor |
|---|---|
| 5 | Every voice line and every designed effect is clear by ear; timeline shows no 2 events in 1 frame unless planned. |
| 4 | 1 minor overlap, clear by ear. |
| 3 | 1 effect is hard to hear, or the voice dips under an effect. |
| 2 | Several effects or words are lost. |
| 1 | A key line or the key hit cannot be heard. |

## A5 Repetition fatigue

Applies to a pool of takes, a loop, or an effect that fires many times. A
single one-shot is `n/a`.

| score | anchor |
|---|---|
| 5 | Takes differ by ear; spread in `duration`, `centroid_hz` or `f0_hz` across the pool; loop seam clean in `sfx check`. |
| 4 | Takes differ slightly; no take stands out. |
| 3 | 2 takes sound the same, or 1 take stands out and gets noticed on repeat. |
| 2 | The pool sounds like 1 file. |
| 1 | 1 file for a frequent event, or a loop with a click at the seam. |

## A6 Tonal fit

Fit with the brief and with the other sounds of the film or game.

| score | anchor |
|---|---|
| 5 | Matches the brief's mood and material; pitched sounds share a key or sit a fourth or fifth apart. |
| 4 | Fits; 1 sound slightly off in brightness or weight. |
| 3 | 1 sound belongs to another palette. |
| 2 | Several sounds clash, or the palette fights the picture. |
| 1 | The wrong sound class for the event (a laser for a door). |

## A7 Realness

Judge against the brief: a brief can ask for a stylised synthetic sound. Then
"real" means believable in that style.

| score | anchor |
|---|---|
| 5 | Believable for the brief by ear; `realness` agrees with the brief's ask. |
| 4 | Believable; 1 tell (too clean tail, static pitch). |
| 3 | Clearly made in code where the brief asks for real, or the reverse. |
| 2 | Cheap: buzzy oscillator, fake noise burst, no body. |
| 1 | Broken: artefacts, aliasing, digital silence in the tail. |

## Review file template

```markdown
# Audio review round <N>

Scope: <files or film reviewed>. Brief: <path>. Cues or events: <path or none>.
Meter commands: <each hark command that ran, with its exit code>.

## Ear notes
- <file>: <class, attack, body, tail, character, defects - no numbers>

## Scores
| axis | score | ear | readings |
|---|---|---|---|
| A1 clarity | 4 | reads without bass | bands_db mid -3.1, low -9.0; brightness_hz 2810 |
| A2 timing | ... | ... | ... |
| A3 loudness | ... | ... | ... |
| A4 masking | ... | ... | ... |
| A5 repetition | ... | ... | ... |
| A6 tonal fit | ... | ... | ... |
| A7 realness | ... | ... | ... |

## Fixes
1. BLOCKER <file to change>: <problem>. Reading: <value>. Target: <value>.
2. POLISH <file to change>: <problem>. Reading: <value>. Target: <value>.

VERDICT: PASS | FAIL
```
