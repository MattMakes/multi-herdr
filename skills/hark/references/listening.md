# Listening: choose an asset, read the numbers, write queries

The `listen` unit owns these commands. Each needs the ML extras (section "Setup" in `setup.md`).

## Loop 1: choose an asset

| step | command | read |
|---|---|---|
| 1 | `hark rank "<description>" <folder>/ --top 5` | a shortlist |
| 2 | `hark find <winner> "<single-event description>" --top 3` | the best spans as trims (start, end in seconds) |
| 3 | `hark describe <winner> --start S --end E` | the strongest tags, plus attack and decay of the span |
| 4 | `hark realness <winner> --start S --end E` (or a folder) | real or pro-produced versus code-synthesized, for the exact span you ship |
| 5 | `hark ab a.wav b.wav --query "<description>" [--a-start --a-end --b-start --b-end]` | the closer candidate, and how B differs in measured terms |
| opt | `hark match <file> "<claim>" --alts "<confusion>" "<confusion>"` | a test of 1 claim against plausible confusions |
| opt | `hark compare <file> --refs <approved>/ [--start --end]` | how well the file sits with an approved palette |

A and B can be 2 trims of 1 multi-take file.

### `find`

- 3 segment kinds. A `stroke` runs from 1 onset through its decay. An `event` groups strokes of 1 action (the press
  and release of a key). Both start 5 ms before the attack and end after the decay. A `window` is a plain span: for a
  one-shot query its edges snap to the sound; for a bed it is meant to be crossfaded.
- `--kind event` limits results to onset trims (hits, clicks, steps). `--kind texture` limits them to windows (beds).
  Without the flag find infers the kind from the query.
- `head` is the level of the first 5 ms and `tail` of the last 10 ms, relative to the trim's peak. At or under -30 dB
  the edge is clean. A `head cut` or `tail cut` flag means the trim starts or ends inside a sound: never use it
  unchanged.

### The tagger on short sounds

On one-shots under about 0.5 s the AudioSet tagger is unreliable. "Tagger unsure", or a neighbouring class (zipper
for a swish, finger snapping for a click), counts neither for nor against. A clearly unrelated class (speech, music,
engine, vehicle) is a warning sign.

### Realness

- Run it on the span that ships. On a file of many takes, p_real averages every take and the gaps between them.
- When the user asked for real or professionally produced audio, a "likely code-synthesized" trim is out. An
  "uncertain" trim is usable only after you read its evidence; the report gives the reason you kept it.
- When the goal is synthetic audio (procedural music, UI tones, forge output), realness is a reading to report, not a
  filter.
- Thresholds depend on length, and each summary prints the ones it used. Clips of 6 s or less: lo 0.17, hi 0.77.
  Longer files average 4 s windows: lo 0.40, hi 0.77 (loosely calibrated). Across lengths compare verdicts, not raw
  p_real.
- Realness judges the sound, not its origin. Polished professional synthetic tones (UI sounds, chimes, synth stabs)
  score as synthetic too. When realness could not run, say that real versus synthetic was not checked. Tagger classes
  like sine wave, beep or synthesizer are weak evidence only.
- Runs of exact digital zero prove nothing alone: real recordings often carry edit or fade padding.

## Reading the numbers

| number | meaning | trust |
|---|---|---|
| LUFS, dBTP, LRA, dBFS, ms, Hz | meter readings (BS.1770 loudness, spectrum, envelope) | exact; set levels from these |
| z (match, rank, find, describe tags) | how far 1 description stands out from a few hundred vocabulary descriptions scored on the same sound | >= 3 strong, 2.5-3 plausible, 2-2.5 weak, < 2 no match |
| p_vs_alts | a sharp, uncalibrated forced choice among the query and its alternatives | only says which wording wins; judge with z |
| cosine | raw CLAP similarity | not a probability; compare only within 1 sound and 1 query |
| tagger labels (0..1) | independent per-class AudioSet probabilities | fine for broad classes; deaf above 8 kHz |
| p_real, verdict | a small fitted classifier: real or pro-produced versus code-synthesized | strong, not conclusive; read the evidence; clip and long-file thresholds differ |
| digital_silence_frac | share of samples that are exactly 0 | weak alone |
| typicality z (compare) | distance to the reference set relative to its own spread | >= -1 fits; < -2 is an outlier |
| sync offset_ms, chance | sound minus picture per cut or cue; the score random edits would get | meaningful only well above chance |
| critique status | the value against the range of 7 professional launch films | a range, not a target; ok = inside what pros do |

Trust order: measurements, then verdicts, z values and probabilities. Never report a cosine as a confidence. For
critique, loudness follows web delivery practice: true peak <= -1 dBTP for a master, <= 0 dBTP for decoded AAC or MP3
(which already includes codec overshoot).

## Writing queries

- Name the sound, not its job. Use source, material, action and character: "a metal locker door banging shut in a
  tiled corridor", not "a sound for the title card".
- Give alternatives a listener could really mistake for it (thock vs click vs typewriter).
- When a z sits near a boundary, try 2 or 3 wordings. Believe a reading only when it survives all of them.
- hark cannot hear a purpose. A purpose phrase ("something appearing") can match none of a set of real pops that their
  own titles match. To test a sound for a purpose, describe the sound that purpose needs in 2 or 3 ways and keep the
  best reading.

## Limits to state when they apply

- hark does not measure taste. It identifies a sound and meters it. Beauty and fit with the story are the user's call:
  give evidence and a shortlist.
- No speech recognition: hark detects speech, not its words. No judgement of lyrics or language.
- CLAP covers 50 Hz-14 kHz; the tagger stops at 8 kHz. Air and low-pass problems need `hark measure` (bandwidth,
  bands).
- Very short sounds are padded to a 10 s window. Isolated hits read best through `find` or `describe` on a trimmed
  span.
- `attack_ms` and `decay_ms` use smoothed envelopes (5 ms and 10 ms RMS). An instant attack reads about 4 ms. Very
  short clicks cannot show a decay under about 10-20 ms.
- Picture cuts come from frame differences. Hard cuts and whips are reliable. Slow crossfades are missed or found late.
  Motion inside a shot is not an event: animation needs cue times (`--cues`).
- Dense mixes smear the tags. Describe 1 stem or 1 segment, not the full mix.
- Rights, provenance and attribution are outside what audio can answer. The `sound-sourcing` skill carries the
  usage-rights audit.
