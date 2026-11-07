# Music laid last

Lock the picture before you build the final music bed. The early beat grid only
plans cuts. Measure tempo from each source audio file. Never trust its metadata.

## Edit plan

Write `audio/music-edit.md` before you render audio. Use 1 table row for each
edit: source, section, source in, source out, film in, film out, edit type and
crossfade ms. Build the complete bed from 1 script. The intro and the film use
1 continuous audio file.

Edit on downbeats at 4-bar or 8-bar phrase boundaries. Use an equal-power
crossfade from `20 ms` to `60 ms`. Repeat or skip whole bars. Use stems when
they are available. Time-stretch by at most `3 %`. Do not pitch-shift.

The final bed must not stop for a picture cut. Use a musical breakdown instead
of silence: low-pass the music, remove the drums and dip by at most `12 dB`.
Put the hero hit on the downbeat. Apply transient control to the SFX bus only.

For a series, select 1 signature end song. Blend each short into that song over
`1` or `2` bars before the end card.

## Join checks

The music build script resolves and runs both audio gates at run time:

```sh
python3 "$(horch skills path video-audio --script scripts/check_audio.py)" <arguments>
python3 "$(horch skills path video-audio --script scripts/check_music.py)" <arguments>
```

The build script never stores a `skill-bundles` path. It exits non-zero when
resolution fails or either checker exits non-zero. A gate that did not run is
a failure, not a pass.

`check_music.py` checks `50 ms` windows against the median of the local 4-bar
phrase and rejects a drop above `12 dB` or digital silence. It also checks each
`film in` value in the edit plan.

A short splice must have no beat-phase error above `15 ms`. A crossfade shorter
than 1 bar must not join keys that are a half step or a tritone apart. Blend
clashing keys for at least 1 bar inside a filtered breakdown. An unpitched
bridge bar is also valid. Review the measured tempo, key or chroma, spectral
flux and loop-join count.

The numbers do not replace a listen. The checker writes a `6 s` excerpt for
each edit. The operator listens to every excerpt before the mix locks.

## SFX rhythm and variation

Use round-robin samples for repeated effects. Move variants by small pitch
steps on a musical scale, and vary their timbre. Use a fast "main hit + offset
tap" group for action. Do not use evenly spaced copies of 1 fixed sound.
