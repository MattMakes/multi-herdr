---
name: video-audio
description: Use when a video project needs its sound - per-scene voice-over from local TTS (piper, kokoro or macOS say), scene timings reconciled to the spoken length in timeline.json, a beat grid in audio/beats.json, procedural ffmpeg SFX on clicks, keys and transitions, music ducked under the voice, and a -14 LUFS / -2 dBTP stereo master in audio/mix.wav proved by scripts/check_audio.py.
---

# Video audio

Phase 3 of the pipeline. Input: `timeline.json`, `DIRECTION.md`,
`captures/<scene-id>.json` and an optional music bed. Output: `audio/vo-*.wav`,
`audio/beats.json`, `audio/sync-sheet.json`, `audio/sfx-cues.json`,
`audio/bed.wav`, `audio/mix.wav`, and `qa/audio-check.json`. The editor in Phase 5 uses only `audio/mix.wav` and the
reconciled `timeline.json`.

`<skill-dir>` means the directory where this SKILL.md sits. Run every command from
the root of the video project.

Every step runs locally with no network: `ffmpeg`, `ffprobe`, `python3` (standard
library) and macOS `/usr/bin/say`. Do not download a model or a sample.

Do not commit generated `.wav`, `.aiff` and `.mp3` files.

## Steps

Read `meta.format` in `timeline.json` first. It is `short` or `long`.
A `long` film has no voice-over by default and has chapters in `meta.chapters`.
For `long`, read `<skill-dir>/references/long-form.md`.
Every rule below applies to both formats, unless it names a format.

1. Voice-over per scene -> `audio/vo-<scene-id>.wav`.
2. Reconcile timings in `timeline.json`, then run the plan checker again.
3. Beat grid -> `audio/beats.json`.
   3b. Sync sheet -> `audio/sync-sheet.json` (word times and beats).
4. SFX cues -> `audio/sfx/*.wav` and `audio/sfx-bus.wav`.
5. Duck and master -> `audio/mix.wav`.
6. Gate -> `<skill-dir>/scripts/check_audio.py`. Fix and repeat until it passes.

Recipes for steps 3 to 5: `<skill-dir>/references/lavfi-sfx-recipes.md`.
The sync sheet, effect placement, arc rules, series grid (measure 1
per song) and cut check against the final bed:
`<skill-dir>/references/sync-and-placement.md`.
Lock picture before the final continuous music bed, then run `scripts/check_music.py`.
The edit, continuity and SFX variation rules: `<skill-dir>/references/music-laid-last.md`.

## 1. Voice-over

A `long` film with no `narration` skips this step and step 2. See `references/long-form.md`.

Make 1 `48 kHz` mono PCM file for each scene with non-empty `narration`.

Use the first engine that is installed, in this order:

1. `piper` with a voice model already on disk:
   `echo "<text>" | piper --model <voice.onnx> --output_file /tmp/vo.wav`
2. `kokoro` with weights already on disk.
3. macOS `say`. It is always present and deterministic for a given voice:

```bash
say -v Samantha -o /tmp/vo-s01-hook.aiff "Stop debugging flaky tests by hand."
ffmpeg -hide_banner -v error -y -i /tmp/vo-s01-hook.aiff -ar 48000 -ac 1 \
  -c:a pcm_s16le audio/vo-s01-hook.wav
```

Run `say -v '?'` to list voices. Use the voice from `DIRECTION.md`, 1 voice
for the whole video. Set the rate with `say -r <wpm>`: `130-160` WPM.

Trim the engine silence at the start and the end. Keep `50 ms`:

```bash
ffmpeg -v error -y -i audio/vo-s01-hook.wav -af \
  "silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05,areverse,\
silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05,areverse" \
  audio/vo-s01-hook.trim.wav && mv audio/vo-s01-hook.trim.wav audio/vo-s01-hook.wav
```

Listen to each file once. Fix a wrong pronunciation by a respelling in the
text sent to TTS, for example `"horch"` -> `"horrch"`. Do not change the caption text.

## 2. Timing reconciliation

The spoken length decides the scene length. `scripts/reconcile_timing.py`
measures each file with `ffprobe` and extends any scene that is too short.
This rule is fixed:

`duration_sec = max(planned duration_sec, vo_duration_sec + 0.25)`

The `0.25 s` is the breath pad before the next scene. With `meta.bpm`, the
end of a scene that grew moves up to the next beat (`k * 60 / bpm`), so every
cut stays on the step 3 grid. Every `beat_in` and `beat_out` is recomputed. The script applies the pad, recomputes every `start_sec` and `meta.duration_sec`,
and moves the other project times with the scenes: `rhythm[]` `start_sec`, `end_sec` and
`events_sec`, `meta.hero_sec`, `meta.payoff_breakdown_sec` and
`meta.allowed_silences`. A time inside a scene that grew keeps its fraction of
that scene. A time after it moves by the same change. It records
`vo_duration_sec` on each narrated scene and prints each changed scene.

```bash
python3 "<skill-dir>/scripts/reconcile_timing.py" timeline.json --audio-dir audio
```

Then run the `video-storyboard` plan checker. It must pass before step 3:

```bash
python3 "<skill-dir>/../video-storyboard/scripts/check_timeline.py" timeline.json \
  --out qa/plan-check.json
```

When that folder does not exist, send the reconciled `timeline.json` to the
director. When a scene now breaks the scene length limit, shorten the narration
text and go back to step 1. Do not speed up the voice. Report each changed scene
to the director in 1 line: scene id, old and new `duration_sec`.

## 3. Beat grid

Write `audio/beats.json`:
```json
{ "bpm": 120.0, "beats_sec": [0.0, 0.5, 1.0], "downbeats_sec": [0.0, 2.0], "source": "pulse" }
```

- No music bed: render the procedural pulse at the `DIRECTION.md` `bpm`. The
  grid is exact: `beats_sec[k] = k * 60 / bpm` up to `meta.duration_sec`.
  `downbeats_sec` is every 4th beat.
- A music bed in `audio/music.wav`: run the onset and autocorrelation snippet
  in the reference. Check the `bpm` against the track metadata.

Both lists are strictly increasing and inside `[0, meta.duration_sec]`. Snap
cuts and hero reveals to a beat within `40 ms` (`2` frames at `60 fps`). Tell the
director which cuts moved.

End on the song's real final phrase: edit whole bars out of the middle so the
song's own ending lands on `meta.duration_sec`. A pad loop with a
`1.0 s` `afade=t=out` is a fallback only, when the song has no usable ending.
This is a `short` rule: a `short` film has 1 bed.

`long` beat grid rules (`meta.format: long`) are in `<skill-dir>/references/long-form.md`.

## 3b. Sync sheet

`video-motion`, `video-character` and the critic read word times from `audio/sync-sheet.json`. Write it after steps 2 and 3. It needs the final
scene `start_sec` and the beats:

```bash
python3 "<skill-dir>/scripts/sync_sheet.py" --timeline timeline.json \
  --audio-dir audio --beats audio/beats.json --out audio/sync-sheet.json
```

It splits each voice file into phrases with `ffmpeg silencedetect` and gives
each word a time in proportion to its syllables (`"timing": "estimated"`).
Add `--whisper-model <file>` only when a whisper.cpp binary and model are
already on disk (`"timing": "measured"`). Run it again after a voice file or scene start changes.

## 4. SFX

Sources, in this order: (1) procedural `lavfi` recipes (`sine`, `anoisesrc`,
`aevalsrc`, `afade`, `aecho`, `biquad`) rendered into `audio/sfx/`; (2) CC0
samples that the project already has in `audio/sfx/*.wav`.

Cue sources:

- `captures/<scene-id>.json` `clicks[].t_sec` -> `click`, and `keys[].t_sec` ->
  `key`. Absolute time is `scene.start_sec + t_sec`.
- `timeline.json` `scenes[].transition_out` -> `whoosh` or `whip`, centred on the cut.
- Callout reveals -> `chime` or `pop`. The CTA or hero drop -> `riser` + `impact-sub`.

Write every cue to `audio/sfx-cues.json` as
`{ "time_sec", "kind", "scene_id", "gain_db", "sync", "start_sec" }`. Place
each effect by its measured sync point, not by its first sample: run
`check_audio.py --measure-sfx audio/sfx`, then set
`start_sec = time_sec - <onset, peak or end of the file>`. A riser ends on
the drop (`sync: end`). Build the SFX bus with 1 `adelay` of `start_sec` per cue.
Keep SFX `12-20 dB` under the voice, fade every edge, and use 1 key sound per `60 ms` at most.

## 5. Ducking and master

1. Build `audio/vo-bus.wav`: each voice file at
   `scene.start_sec + narration.lead_in_sec` (default `0.15 s`).
2. Duck the music under the voice by `8-12 dB` with
   `sidechaincompress=threshold=0.02:ratio=8:attack=30:release=250` (attack
   `30 ms`, release `250 ms`). Volume automation with `volume` and `enable` is
   the alternative. Also write the ducked bed alone to `audio/bed.wav` for the
   effect margin check.
3. Mix voice, ducked music and SFX with `amix ... normalize=0` into
   `audio/premix.wav`.
4. Master in 2 passes with `loudnorm=I=-14:TP=-2.0:LRA=7`: measure first, then
   apply the measured values with `linear=true`. Write `audio/mix.wav` as
   `48 kHz` stereo 24-bit PCM (`pcm_s24le`). 16-bit is also accepted.

Single-pass `loudnorm` missed by `1.5 LU` on a `6 s` test mix (a `short` clip).
Always run 2 passes. The exact commands are in the reference, sections 4 to 6.
`long` ducking and mastering rules are in `<skill-dir>/references/long-form.md`.

Pad or trim the master to `meta.duration_sec` (gate: `+-0.05 s`).

## 6. Gate

```bash
python3 "<skill-dir>/scripts/check_audio.py" audio/mix.wav \
  --timeline timeline.json --beats audio/beats.json --sync-sheet audio/sync-sheet.json \
  --sfx-cues audio/sfx-cues.json --sfx-bus audio/sfx-bus.wav --bed audio/bed.wav \
  --out qa/audio-check.json
```

Add `--music audio/music.wav` when the project has a supplied music bed.

Exit `0`: every gate passes. Exit `1`: a gate fails; `errors[]` names the rule.
Exit `2`: an input or a tool is missing.

| Rule | Pass condition | Usual fix |
| --- | --- | --- |
| `format.sample_rate` | `48000` Hz | Add `-ar 48000` to the master command |
| `format.channels` | `2` | Add `-ac 2` |
| `format.duration` | within `+-0.05 s` of `meta.duration_sec` | `apad=whole_dur=<d>,atrim=0:<d>` |
| `loudness.integrated` | `-15.0` to `-13.0` LUFS | Run both loudnorm passes again |
| `loudness.true_peak` | WAV `<= -2.0` dBTP; AAC adds about `+1 dB` | Master again with `TP=-2.0`; video-render checks MP4 `<= -1.0` dBTP |
| `mix.cut_peak` | peak within `20 ms` of each cut `<= -3.0` dBFS | Limit transients on the SFX bus only |
| `silence.leading` | `<= 0.20 s` | Start the bed or the voice at `0.0 s` |
| `silence.trailing` | `<= 0.20 s` | Let the bed fade to the last frame |
| `silence.dropout` | no internal gap `> 0.80 s` below `-50 dBFS` | Extend the bed, or list the gap |
| `beats.*` | `bpm > 0`, both lists strictly increasing in `[0, duration]` | Make the grid again |
| `sync.*` | Words in order inside their segment, segment inside its scene, every narrated scene present | Run `sync_sheet.py` again |
| `sfx.sync` | Each cue's sync point lands within `40 ms` of `time_sec` | Set `start_sec` from `--measure-sfx` |
| `sfx.margin` | Effect level minus bed level in the effect's band is `>= 3 dB` | Raise `gain_db` or duck the bed |
| `mix.hero_loudest` | Loudest moment inside `meta.hero_sec` | Lift the hero |
| `mix.payoff_breakdown` | Optional `meta.payoff_breakdown_sec` dips `<= 12 dB` and is never under `-50 dBFS` | Low-pass or thin the bed, do not cut it |
| `music.drop` | The bed's drop within 1 beat of the hero start | Offset the bed |

When `meta.bpm` is set, the film has music, and `meta.hero_sec` is required.
No gate requires or exempts silence before the payoff. The old
`meta.payoff_silence_sec` fails `plan.removed_field`.

A planned silence (a dramatic pause) needs a `timeline.json` entry: `"meta": { "allowed_silences": [[12.4, 13.6]] }`, in seconds.

`metrics` gives `integrated_lufs`, `true_peak_dbtp` and every silence range.
Copy these numbers into your `DONE:` line.

Run `python3 "<skill-dir>/scripts/check_audio.py" --self-test` after you
change the script.

## Done means

- `qa/audio-check.json` has `"passed": true`.
- `audio/sync-sheet.json` exists and the gate ran with `--sync-sheet`.
- `qa/plan-check.json` passes on the reconciled `timeline.json`.

- `DONE:` states the LUFS, the true peak, the final duration, the TTS engine
  and voice, the sync sheet `timing`, and each changed scene.
