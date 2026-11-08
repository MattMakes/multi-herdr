# Sync sheet, effect placement and the audio arc

This reference holds the detail of step 3b (the sync sheet), the effect
placement in step 4, the bed file of step 5 and the arc rules of the gate.

## Series song grids and final-bed cut checks

Keep exactly 1 measured grid for each song at
`<series root>/music/grids/<song id>.json`. Create it from the source song:

```bash
python3 "<skill-dir>/scripts/check_music.py" path/to/song.wav \
  --write-grid "<series root>/music/grids/<song id>.json"
```

The file has this schema:

```json
{
  "song": "song-id",
  "source_sha256": "64 lowercase hexadecimal characters",
  "tempo_bpm": 120.0,
  "bar_sec": 2.0,
  "downbeats_sec": [0.498, 2.498, 4.498],
  "measured_by": "check_music.py 1"
}
```

`song` identifies the source, and `source_sha256` proves which source file was
measured. `tempo_bpm`, `bar_sec`, and `downbeats_sec` are measurements from that
file. `measured_by` records the script and measurement version. An episode reads
this series file. It never copies a tempo, bar length, or downbeat number by hand.

The final bed can change the source timing through a warp, trim, or loop. At
render time, measure the final bed again and compare every scene cut from
`timeline-master.json` (or `timeline.json`) with its nearest measured beat:

```bash
python3 "<skill-dir>/scripts/check_audio.py" audio/mix.wav \
  --timeline timeline-master.json --bed-beats audio/bed.wav
```

Every cut must be within `20 ms` of a measured final-bed beat. The report records
the worst drift in seconds and its cut time. Rebuild the timing when this check
fails; do not replace the series grid with a copied bar length.

## 1. The sync sheet: `audio/sync-sheet.json`

`video-motion` locks word highlights to it. `video-character` builds visemes
from it. The critic reads it for the sync axis. Write it after the timing
reconciliation and the beat grid, because it needs the final `start_sec` of
each scene and the beats.

```bash
python3 "<skill-dir>/scripts/sync_sheet.py" --timeline timeline.json \
  --audio-dir audio --beats audio/beats.json --out audio/sync-sheet.json
```

Every time is in project time: seconds counted from the first frame of the film. To get
scene time, subtract the scene `start_sec`.

```json
{
  "version": 1,
  "timing": "estimated",
  "bpm": 120.0,
  "beats_sec": [0.0, 0.5, 1.0],
  "downbeats_sec": [0.0, 2.0],
  "vo_segments": [
    {
      "scene_id": "s01-hook",
      "file": "audio/vo-s01-hook.wav",
      "text": "Ship every frame.",
      "start_sec": 0.15,
      "end_sec": 1.42,
      "timing": "estimated",
      "phrases": [[0.2, 1.37]],
      "words": [
        { "word": "Ship", "start_sec": 0.2, "end_sec": 0.43 },
        { "word": "every", "start_sec": 0.43, "end_sec": 1.14 },
        { "word": "frame.", "start_sec": 1.14, "end_sec": 1.37 }
      ]
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `timing` | `measured` when every segment is measured, else `estimated` |
| `bpm`, `beats_sec`, `downbeats_sec` | Copied from `audio/beats.json` |
| `vo_segments[].start_sec` | `scene.start_sec + lead_in_sec` (default `0.15 s`) |
| `vo_segments[].end_sec` | `start_sec` + the voice file length |
| `vo_segments[].phrases` | Speech ranges between pauses |
| `vo_segments[].words[]` | The narration words as written, with `start_sec` and `end_sec` |

A scene with `narration` as an object uses `narration.text` and
`narration.lead_in_sec`. A string `narration` uses the default lead-in.

### Estimated route (always)

1. `ffmpeg silencedetect` (`-40 dB`, `0.15 s`) splits each voice file into
   phrases.
2. The script splits the narration into text phrases at `, . ; : ! ?` and at a
   free-standing dash.
3. When the counts match, text phrase `k` goes to speech phrase `k`. When they
   do not match, each word goes to the speech phrase that holds its syllable
   midpoint.
4. Inside a phrase, each word gets time in proportion to its syllable count.

Phrase starts are within about `0.1 s`. Word times inside a long phrase are
less exact. Lead a highlight by `0-50 ms`, and do not lock a hard cut to 1
estimated word inside a phrase.

### Measured route (optional)

Use it only when a whisper.cpp binary (`whisper-cli` or `whisper-cpp`) is on
`PATH` and a model file is already on disk. Do not download a model.

```bash
python3 "<skill-dir>/scripts/sync_sheet.py" --timeline timeline.json \
  --whisper-model ~/models/ggml-base.en.bin
```

A segment is measured only when the transcribed words match the narration
words in count and at least `80 %` by text. Otherwise that segment keeps the
estimate. A mismatch often means a wrong pronunciation: listen to that file.

## 2. Effects placed by measured sync point

Each effect has 3 sync points, measured on its rendered file: `onset` (the
first sound), `peak` (the loudest `5 ms`) and `end` (the last sound). The
floor is `40 dB` under the peak. Measure every file in `audio/sfx/`:

```bash
python3 "<skill-dir>/scripts/check_audio.py" --measure-sfx audio/sfx
```

Each cue in `audio/sfx-cues.json` names the moment and the sync point that
must land on it:

```json
[
  { "time_sec": 6.0, "kind": "riser", "scene_id": "s03-hero", "gain_db": -8,
    "sync": "end", "start_sec": 4.52 },
  { "time_sec": 6.0, "kind": "impact-sub", "scene_id": "s03-hero", "gain_db": -6,
    "sync": "peak", "start_sec": 5.98 }
]
```

- `time_sec`: the moment (a cut, a click, the hero).
- `sync`: `onset`, `peak` or `end`. Default: `end` for `riser`, `peak` for
  every other kind.
- `start_sec`: where the file starts in the bus, the `adelay` value. Compute it
  as `time_sec - <sync point of the file>`.
- `file`: optional. The default is `audio/sfx/<kind>.wav`, relative to the
  project root.

The gate measures each file again and fails `sfx.sync` when
`start_sec + sync point` is more than `40 ms` from `time_sec`. The usual
defect is a riser that starts on the drop instead of ending on it.

## 3. Effect over the bed

In step 5, also write the ducked bed alone as `audio/bed.wav`. Split the
`[duck]` output with `asplit=2[duck][bedout]` and map `[bedout]` to the
second output file.

For each cue, the gate finds the band of the effect (low under `150 Hz`, mid
`150 Hz - 2 kHz`, high over `2 kHz`). It then compares the SFX bus and the bed
in that band over the effect span, from onset to end. The effect level minus
the bed level must be `3 dB` or more (`sfx.margin`). Fix: raise the cue `gain_db`, move
the effect to a band where the bed is quiet, or duck the bed for the cue.

## 4. The arc: hero, planned silence, drop

The director writes these `timeline.json` fields:

| Field | Meaning |
| --- | --- |
| `meta.bpm` | Optional. A number `> 0` says that the film has music. Then `meta.hero_sec` and `meta.payoff_silence_sec` are required. |
| `meta.hero_sec` | `[start_sec, end_sec]` of the hero moment |
| `meta.payoff_silence_sec` | `[start_sec, end_sec]` of the planned silence before the payoff |

Rules:

- `mix.hero_loudest`: the loudest momentary loudness (`400 ms`) of the mix is
  inside the hero range, within `0.5 LU`. Fix: lift the hero (an impact, the
  full bed) or lower the louder place.
- `mix.payoff_silence`: inside the planned range there is a run of at least
  `0.4 s` that is `15 dB` or more under the 90th percentile of the `100 ms`
  levels and under `-30 dBFS`. Fix: cut the bed and the effects in that range,
  and end the voice before it. A planned silence also counts as an allowed
  silence for `silence.dropout`.
- `music.drop` (with `--music audio/music.wav`): the drop is the downbeat whose
  bar has the largest rise in energy under `150 Hz`. When that rise is at least
  `6 dB` and the plan has a hero, the drop must be within 1 beat of the hero
  start. The report gives `drop_sec` and the energy of each bar. Fix: move or
  cut the bed so its biggest rise coincides with the hero start.
