---
name: audio-review
description: Use when a sound effect, a variation pool, a game SFX pack or a film mix needs a blind review by ear - listens to each WAV through agy (Gemini hears audio natively), takes every number from hark measure, hark timeline and hark sfx check, scores 7 axes from 1 to 5 (small-speaker clarity, timing, loudness and peaks, masking, repetition fatigue, tonal fit, realness) and writes 1 review file with fix lines ranked BLOCKER or POLISH, each naming the file to change.
---

# Audio review: blind ears plus hark numbers

You judge sound. You do not fix it. You write exactly 1 file: the review file
that your task names. When the task names none, write
`qa/audio-review-round-<N>.md` in the project folder.

The ear and the meter do different jobs:

- Your ear (Gemini through agy) is good at the class of a sound, its rough
  character, its fit with the brief, and gross errors: a silent file, a wrong
  sound, a harsh click, a cheap synthetic tone.
- Your ear is not good at numbers. The probe
  (`docs/specs/hark/agy-audio-probe.md`) found pitch wrong by up to 260 Hz,
  wrong hit counts, and onsets 0.2-0.5 s late. So every number in the review
  (time, pitch, count, duration, level) comes from hark. Never write a number
  that you heard.

## Blind rules

- Read only: the brief or the event list that the task names, the audio files,
  the picture or the cue file, and your own hark output.
- Do not read builder notes, chat history, commit messages, recipe code or
  earlier review rounds. Each round is a fresh session.
- Listen before you read any hark output for that file. Write your ear notes
  first, then measure, then score. Where ear and meter disagree, the meter
  wins for numbers and the ear wins for character.
- Listen to WAV only. The probe heard an MP3 as silence. When the input is not
  WAV, report it as a BLOCKER for the owner and score only from the meters.

## Step 1: preflight

Run each command once:

- `HARK="$(horch skills path hark --script scripts/hark)"` - resolves the hark
  launcher. Never use a skill-bundle path, and never store the path in a file.
- `"$HARK" doctor` - exit 3 means no hark venv; report `BLOCKED:` with the
  output. Do not install anything.
- `ffprobe -version` - needed to check the format of each file.

## Step 2: listen

Open each WAV with your file-reading tool, by its absolute path. agy attaches
the file as a native audio part. Describe what you hear in plain words: the
class, the attack, the body, the tail, the character, any defect. Do not run a
shell command to "listen". When your session cannot attach the audio, use the
standalone invocation in [listening](references/listening.md).

## Step 3: measure

| need | command |
|---|---|
| levels, peaks, onset, attack, decay, bands, pitch | `"$HARK" measure <files> --json` |
| defects in effect files (clicks, cut tails, silence, pitch drift, 2 attacks) | `"$HARK" sfx check <files> --json` |
| a mix or film: events, sync per cue and per cut, loudness curve | `"$HARK" timeline <film or wav> --cues <cues.json> --json` |
| real recording or code-synthesized (needs the ML extra) | `"$HARK" realness <files> --json` |
| a film scorecard | `"$HARK" critique <film> --fast --json` |

On a shared machine, prefix each call with `HARK_THREADS=2`. When a command
exits 3 because the ML extra is absent, score that axis from the ear only and
say so in the axis line.

## Step 4: score

Score each axis from 1 to 5 with [the rubric](references/rubric.md). Each
axis line gives the score, 1 ear sentence and the hark readings that back it.
An axis that does not apply (timing for a pack with no picture or event list)
is `n/a` with 1 reason.

| axis | ear | meter |
|---|---|---|
| A1 clarity on phone and laptop speakers | does it read without bass? | `bands_db`, `centroid_hz`, `brightness_hz` |
| A2 timing to picture or game event | does it feel early or late? | timeline `offset_ms`, `attack_at`, `attack_ms` |
| A3 loudness and peaks | jumps, harshness, pumping | `lufs`, `true_peak_dbtp`, `clipped_samples`, `lra_lu` |
| A4 masking | does a sound bury the voice or another cue? | timeline overlaps, `bands_db` per stem |
| A5 repetition fatigue | does the pool sound like 1 file? | spread of `duration`, `centroid_hz`, `f0_hz` in the pool |
| A6 tonal fit | does it match the brief and the other sounds? | `f0_hz`, `centroid_hz`, `key` (music) |
| A7 realness | real or cheap synthetic, as the brief asks | `realness` p_real |

## Step 5: write the review file

Use the template in [the rubric](references/rubric.md). Fix lines:

- `BLOCKER` - the sound must not ship: silent or wrong sound, clipping,
  true peak over the ceiling, a cue with no sound, onset off by more than
  1 frame (video) or 10 ms (game), a click at an edge, an axis at 1.
- `POLISH` - ships, but better with the fix: any axis at 2 or 3.
- Each fix line names the file to change, the reading that proves the
  problem, and the target reading. Rank BLOCKER lines first, then POLISH.
- End with exactly 1 line: `VERDICT: PASS` when there is no BLOCKER and no
  axis under 3, else `VERDICT: FAIL`.

Read the review file back once before you report it.
