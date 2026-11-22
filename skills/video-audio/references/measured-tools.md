# Measured beats and words with optional tools

The scripts of this skill need only `ffmpeg`. 2 optional tools measure more exactly when the host already has them: librosa for the beat grid of a music bed, and WhisperX for word times of a voice file. Neither is required, and neither is installed by you.

- Both are Python packages with large dependencies. Check with `python3 -c "import librosa"` and `whisperx --help`. Missing: use the built-in route and say so in `DONE:`. Do not install a package.
- WhisperX needs model files. It downloads them on first use. A model that is not on disk is a download: it needs the operator's word in the task. Without it, use the estimate or the whisper.cpp route.
- Your knowledge of these packages may not match the installed version. Check each call against `help()` or `--help` of that version.

## Beat grid from a music bed, with librosa

Use it when `audio/music.wav` has a real tempo that the built-in onset snippet reads wrongly: swing, tempo changes, a sparse intro.

```python
import json
import librosa

AUDIO = "audio/music.wav"
DURATION_SEC = 30.0          # meta.duration_sec
BEATS_PER_BAR = 4

y, sr = librosa.load(AUDIO, sr=None, mono=True)
tempo, frames = librosa.beat.beat_track(y=y, sr=sr, units="frames")
beats = [float(t) for t in librosa.frames_to_time(frames, sr=sr) if 0.0 <= t <= DURATION_SEC]

# Which beat of the bar is the downbeat: the phase with the most onset energy.
strength = librosa.onset.onset_strength(y=y, sr=sr)
phase = max(range(BEATS_PER_BAR),
            key=lambda p: sum(strength[f] for f in frames[p::BEATS_PER_BAR]))
downbeats = beats[phase::BEATS_PER_BAR]

sheet = {
    "bpm": round(float(tempo if not hasattr(tempo, "__len__") else tempo[0]), 2),
    "beats_sec": [round(t, 3) for t in beats],
    "downbeats_sec": [round(t, 3) for t in downbeats],
    "source": "librosa",
}
with open("audio/beats.json", "w", encoding="utf-8") as handle:
    json.dump(sheet, handle, indent=2)
```

Then check the result. A beat tracker is often wrong by a factor of 2.

1. Compare `bpm` with the track's stated tempo. Half or double means the tracker locked to the wrong level: pass a starting tempo to the tracker and run again.
2. The lists are strictly increasing and inside `[0, meta.duration_sec]`. The gaps between beats are even to within 10 %, except where the music changes tempo.
3. Render a click on every beat over the music and read the levels: `check_audio.py` with `--beats` reports whether the mix has energy on the grid.
4. The downbeat guess is a guess. Check that the first strong hit of the track is on a downbeat. If not, shift `phase`.

Other measurements that help the picture, from the same package:

| Need | Measurement | Use |
|---|---|---|
| Where hits are | onset detection, as times | place cuts and impacts on real transients, not only on the grid |
| Where the music changes | the loudness curve (RMS) over time, smoothed over 1 bar | find the drop and put `meta.hero_sec` on it |
| Sections | a novelty curve or a segment boundary method | plan acts on the music's own structure |
| Energy for an ambient motion | the spectral centroid or the RMS, sampled per video frame | write `audio/energy.json` with 1 value per frame; a scene reads it by frame index |

A scene never analyses audio while it renders. Every value it uses is in a file that you wrote before, indexed by frame or by time. That keeps the scene a function of the frame index.

## Word times with WhisperX

WhisperX transcribes and then aligns each word to the audio. Its word times are much closer than the syllable estimate, usually within a few hundredths of a second.

1. Run it on each voice file and ask for JSON output with alignment on. Use the language of the narration and a model that is already on disk. Speaker labels are not needed.
2. Put each result at `audio/words/vo-<scene-id>.words.json`. The file holds `segments[].words[]`, and often `word_segments[]`, with `word`, `start` and `end` in seconds from the start of that voice file.
3. Write the sheet:

```bash
python3 "<skill-dir>/scripts/sync_sheet.py" --timeline timeline.json \
  --audio-dir audio --beats audio/beats.json --words-dir audio/words \
  --out audio/sync-sheet.json
```

A segment becomes `"timing": "measured"` only when the aligned words match the narration words in count and at least `80 %` by text. Otherwise it keeps the estimate, and the sheet as a whole says `estimated`.

When a segment does not match:

- The transcript split or joined a word ("check out" and "checkout", "2026" as 2 words). Edit the words file so that it has 1 entry per narration word, with the outer start and end times.
- An entry has no `start` or `end`. The aligner could not place it: numbers and symbols are the usual cause. Give it the end of the word before and the start of the word after.
- The voice said something else. Listen to the levels and the length of that file; make the voice again.

Do not edit a time to make a picture fit. The words file is a measurement.

Any other aligner works the same way when it writes that JSON shape.

## What changes downstream

- `video-motion` and `video-character` lock word highlights and mouth shapes to the sheet. With measured times, a highlight may lead its word by `0-50 ms` and a hard cut may sit on a word.
- The editor's captions split long scenes into cues by the same word times.
- Say in `DONE:` which route each segment used, and the tool and model names.
