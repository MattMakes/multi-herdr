# Procedural SFX, beat grid and bus recipes

Every recipe here renders with `ffmpeg -f lavfi` and needs no download. Each
command outputs a stereo PCM file at `48 kHz` and 16 bits. Render the SFX into
`audio/sfx/` at build time. Do not commit the `.wav` output to git.

Common tail for every render command:

```bash
-ar 48000 -ac 2 -c:a pcm_s16le audio/sfx/<name>.wav
```

## 1. Design rules

1. Keep each SFX short: UI ticks `20-60 ms`, whooshes `250-450 ms`, chimes
   `400-800 ms`, risers `1.0-2.0 s`.
2. Put a fade in of at least `2 ms` and a fade out on every sound. A hard edge
   makes a click.
3. Place SFX `12-20 dB` below the voice-over. A UI tick sits near `-24 dBFS`
   peak. A transition whoosh sits near `-18 dBFS` peak.
4. Do not repeat one identical sample more than 3 times in `2 s`. Vary pitch by
   `+-3%` or gain by `+-2 dB` with the cue index.
5. Use the band `600 Hz - 6 kHz` for UI sounds. Keep energy below `120 Hz`
   for impacts and risers only.
6. A project-local CC0 sample in `audio/sfx/` replaces a recipe when it sounds
   better. Record its origin in `audio/sfx/SOURCES.txt`.

## 2. Recipes

Each block is the `-i` value for `ffmpeg -hide_banner -v error -y -f lavfi -i "<recipe>"`.

### click (cursor press, `30 ms`)

```text
sine=f=1800:d=0.03,afade=t=in:d=0.002,afade=t=out:st=0.005:d=0.025,volume=-12dB
```

### soft-click (hover or toggle, `40 ms`)

```text
anoisesrc=d=0.04:c=white:a=0.3,bandpass=f=3500:width_type=q:w=4,afade=t=in:d=0.002,afade=t=out:st=0.004:d=0.036
```

### key (keystroke, `25 ms`)

Vary `f` per key from `2400` to `2800` Hz to avoid a machine-gun sound.

```text
anoisesrc=d=0.025:c=pink:a=0.4,highpass=f=1500,biquad=b0=0.3:b1=0:b2=-0.3:a0=1:a1=-1.2:a2=0.5,afade=t=out:st=0.003:d=0.022
```

### whoosh (scene transition, `350 ms`)

A band-passed pink noise with a symmetric swell.

```text
anoisesrc=d=0.35:c=pink:a=0.5,highpass=f=600,lowpass=f=4000,afade=t=in:d=0.15,afade=t=out:st=0.15:d=0.2
```

### whip (`whip-pan` transition, `250 ms`)

A noise sweep through a resonant filter. The `biquad` coefficients give a
narrow resonance near `2 kHz` at `48 kHz`.

```text
anoisesrc=d=0.25:c=pink:a=0.6,biquad=b0=0.2:b1=0:b2=-0.2:a0=1:a1=-1.6:a2=0.8,afade=t=in:d=0.08,afade=t=out:st=0.08:d=0.17
```

### chime (callout reveal, `600 ms`)

```text
sine=f=1320:d=0.6,afade=t=in:d=0.003,afade=t=out:st=0.02:d=0.58,aecho=0.8:0.6:120:0.35,volume=-6dB
```

### success (two-tone confirmation, `500 ms`)

```text
aevalsrc=exprs='0.25*sin(2*PI*(if(lt(t\,0.12)\,880\,1320))*t)*exp(-6*t)':s=48000:d=0.5,afade=t=in:d=0.003
```

### pop (badge or chip appears, `80 ms`)

A fast downward pitch drop.

```text
aevalsrc=exprs='0.5*sin(2*PI*(900-6000*t)*t)*exp(-40*t)':s=48000:d=0.08
```

### impact-sub (hero drop or title slam, `700 ms`)

```text
aevalsrc=exprs='0.8*sin(2*PI*(70-40*t)*t)*exp(-5*t)':s=48000:d=0.7,afade=t=in:d=0.004
```

### riser (`1.5 s` build before a hero drop)

Rising sine sweep plus a swelling noise bed. End the riser exactly on the
downbeat of the drop.

```text
aevalsrc=exprs='0.3*sin(2*PI*(200*t+300*t*t))*(t/1.5)':s=48000:d=1.5,afade=t=out:st=1.45:d=0.05
```

### glitch (error state, `150 ms`)

```text
anoisesrc=d=0.15:c=white:a=0.4,acrusher=bits=4:mode=log:aa=1,afade=t=out:st=0.1:d=0.05
```

### pulse (procedural music bed when no track exists)

`<bpm>` from `DIRECTION.md`, `<dur>` from `timeline.json` `meta.duration_sec`.
`P = 60 / <bpm>`. Example at `120 bpm` (`P = 0.5 s`):

```text
aevalsrc=exprs='0.5*sin(2*PI*55*t)*exp(-12*mod(t\,0.5))+0.08*sin(2*PI*220*t)*(0.5+0.5*sin(2*PI*t/2))':s=48000:d=<dur>
```

The kick sits on every beat. The pad swells once per bar of 4 beats. The beat
grid is exact: `beats_sec[k] = k * P`.

## 3. Cue map to SFX bus

Read cues from these sources. Write each placed event into
`audio/sfx-cues.json` as `{ "time_sec", "kind", "scene_id", "gain_db" }`.

| Source | Field | SFX | Offset |
| --- | --- | --- | --- |
| `captures/<scene-id>.json` | `clicks[].t_sec` | `click` | `scene.start_sec + t_sec` |
| `captures/<scene-id>.json` | `keys[].t_sec` | `key` | `scene.start_sec + t_sec`. Thin to 1 per `60 ms`. |
| `timeline.json` | `scenes[].transition_out` | `whoosh` or `whip` | Centre on the cut: `cut - duration / 2` |
| `timeline.json` | callout reveal | `chime` or `pop` | Reveal start, `-10 ms` lead |
| `timeline.json` | `kind == "cta"` or hero | `riser` + `impact-sub` | Riser ends on the cut. Impact at the cut. |

Snap transition and hero cues to the nearest `beats_sec` value when it is
within `40 ms`. Do not move click and key cues: they follow the picture.

Build the SFX bus with one `adelay` per cue. `adelay` takes milliseconds.

```bash
ffmpeg -hide_banner -v error -y \
  -i audio/sfx/whoosh.wav -i audio/sfx/click.wav -i audio/sfx/chime.wav \
  -filter_complex "\
[0:a]adelay=2675:all=1,volume=-2dB[c0];\
[1:a]adelay=4120:all=1[c1];\
[2:a]adelay=6010:all=1,volume=-4dB[c2];\
[c0][c1][c2]amix=inputs=3:duration=longest:normalize=0,apad=whole_dur=<dur>,atrim=0:<dur>[sfx]" \
  -map "[sfx]" -ar 48000 -ac 2 -c:a pcm_s16le audio/sfx-bus.wav
```

Generate this command from `audio/sfx-cues.json` with a short script. Use
`normalize=0` on every `amix`, or `amix` divides each input by the input count.

## 4. Voice-over bus

Place each `audio/vo-<scene-id>.wav` at `scene.start_sec + narration.lead_in_sec`
(default `0.15 s`):

```bash
ffmpeg -hide_banner -v error -y -i audio/vo-s01-hook.wav -i audio/vo-s02-demo.wav \
  -filter_complex "\
[0:a]adelay=150:all=1[v0];[1:a]adelay=3150:all=1[v1];\
[v0][v1]amix=inputs=2:duration=longest:normalize=0,apad=whole_dur=<dur>,atrim=0:<dur>,aformat=channel_layouts=stereo[vo]" \
  -map "[vo]" -ar 48000 -c:a pcm_s16le audio/vo-bus.wav
```

## 5. Ducking and premix

The voice bus is the sidechain key. With a macOS `say` voice at its default
level, `threshold=0.02` and `ratio=8` gave `10.6 dB` of reduction. The depth
depends on the voice level, so measure it. `attack=30` and `release=250` are
in milliseconds.

```bash
ffmpeg -hide_banner -v error -y -i audio/music.wav -i audio/vo-bus.wav -i audio/sfx-bus.wav \
  -filter_complex "\
[1:a]asplit=2[vo][key];\
[0:a]volume=-6dB[bed];\
[bed][key]sidechaincompress=threshold=0.02:ratio=8:attack=30:release=250[duck];\
[duck][vo][2:a]amix=inputs=3:duration=first:normalize=0,aformat=channel_layouts=stereo[m]" \
  -map "[m]" -ar 48000 -c:a pcm_s24le audio/premix.wav
```

Measure the duck depth: run `ebur128` on the music bus alone and on the
ducked bus over one narration range (`-ss <start> -t <len>`). The momentary
loudness drop must be `8-12 dB`. When the drop is less than `8 dB`, lower
`threshold` to `0.01`. When the drop is more than `12 dB`, raise `threshold`
to `0.03` or lower `ratio` to `4`. Measure again after each change.

Volume automation is the alternative when the voice bus is sparse. Duck the bed
with `volume` and `enable`, one range per narration segment:

```text
volume=volume=-10dB:enable='between(t,0.15,2.90)+between(t,3.15,6.40)'
```

This makes a hard step. Prefer `sidechaincompress` when the step is audible.

## 6. Two-pass loudness master

Single-pass `loudnorm` misses the target on clips shorter than about `30 s`.
A measured `6 s` premix landed at `-15.5 LUFS`. Always run 2 passes.

Pass 1, measure:

```bash
ffmpeg -hide_banner -nostats -i audio/premix.wav \
  -af loudnorm=I=-14:TP=-1.0:LRA=7:print_format=json -f null - 2>&1 \
  | sed -n '/^{/,/^}/p' > audio/loudnorm-pass1.json
```

Pass 2, apply with the measured values in linear mode:

```bash
python3 - <<'PY' > audio/loudnorm-args.txt
import json
d = json.load(open("audio/loudnorm-pass1.json"))
print(f"measured_I={d['input_i']}:measured_TP={d['input_tp']}:"
      f"measured_LRA={d['input_lra']}:measured_thresh={d['input_thresh']}:"
      f"offset={d['target_offset']}")
PY
ffmpeg -hide_banner -v error -y -i audio/premix.wav \
  -af "loudnorm=I=-14:TP=-1.0:LRA=7:$(cat audio/loudnorm-args.txt):linear=true,aresample=48000" \
  -ac 2 -c:a pcm_s24le audio/mix.wav
```

Warning: in zsh, `$VAR:l`, `$VAR:a` and `$VAR:h` are modifiers, and
`$VAR[x]` is a subscript. Write `${VAR}` before a `:` or `[` in a filter
string.

`loudnorm` resamples to `192 kHz` internally. Keep `aresample=48000` at the end.
When the second pass reports `normalization_type: dynamic`, the true-peak
target was not reachable in linear mode. Add `alimiter=limit=0.8:level=false`
before pass 1 and run both passes again. When `check_audio.py` reports a true
peak above `-1.0 dBTP`, run both passes again with `TP=-1.5`.

## 7. Beat grid from a music bed

Python standard library only. It reads PCM from `ffmpeg`, builds an onset
curve from positive energy steps on a `10 ms` hop, finds the tempo by
autocorrelation between `60` and `180 bpm`, then finds the beat phase.

```python
import array, json, subprocess, sys
path, out = sys.argv[1], sys.argv[2]
RATE, HOP = 12000, 120  # 120 samples at 12 kHz = exactly 10 ms
raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-ac", "1", "-ar", str(RATE),
                      "-f", "s16le", "-"], capture_output=True, check=True).stdout
pcm = array.array("h", raw)
energy = [sum(s * s for s in pcm[i:i + HOP]) for i in range(0, len(pcm) - HOP, HOP)]
flux = [max(0.0, b - a) for a, b in zip(energy, energy[1:])]
def score(lag):
    return sum(flux[i] * flux[i + lag] for i in range(len(flux) - lag))
lag = max(range(6000 // 180, 6000 // 60 + 1), key=score)  # lag in 10 ms hops
phase = max(range(lag), key=lambda p: sum(flux[p::lag]))
duration = len(pcm) / RATE
beats = [round((phase + k * lag) * HOP / RATE, 3) for k in range(len(flux) // lag + 1)]
beats = [b for b in beats if b <= duration]
json.dump({"bpm": round(6000 / lag, 2), "beats_sec": beats,
           "downbeats_sec": beats[::4], "source": "onset"}, open(out, "w"), indent=2)
```

The tempo resolution is the `10 ms` hop: `+-0.5 bpm` near `60 bpm` and
`+-3 bpm` near `180 bpm`. When `DIRECTION.md` gives a `bpm` and the bed is
procedural, write the grid from the formula instead. Downbeats assume `4/4`
with the first detected beat on beat 1. Listen once and shift
`downbeats_sec` by 1 to 3 beats when the bar starts elsewhere.
