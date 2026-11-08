# Sound brief: {{FILM}}

This brief is the contract for every agent on the soundtrack. Sub-agents get it unedited.
Delete the example rows (marked `e.g.`) when you fill it in.

## The picture

- Video: `{{VIDEO}}`, {{DURATION}} s, {{FPS}} fps.
- Grid: {{BPM}} BPM in {{BEATS_PER_BAR}}/4. One beat is {{BEAT_S}} s. One bar is {{BAR}} s.
- Cue file: `{{CUES}}` (frames come from the picture code, never typed by hand).

The soundtrack uses recorded or produced audio only:

- 1 licensed music track, edited at bar lines;
- recorded foley and effects on the cues;
- recorded ambiences wherever the picture shows a place.

Code only selects, trims, places, sets levels, pans, equalises, crossfades, stretches the music slightly,
resamples for pitch and masters. Code never makes a tone, a noise or a reverb tail.

Quality target: {{BAR_REFERENCE}}

## Sound concept

- Palette: {{PALETTE}} (one family of close-miked sources). e.g. felt, wood and soft plastic.
- Music: {{MUSIC_CHARACTER}}. e.g. quiet intro, groove from 4 s, breakdown at 12 s, the DROP at {{DROP_T}} s on the
  hero reveal, resolve under the logo at {{LOGO_T}} s, a tail to the end; instrumental, 110-130 BPM.
- Brand personality row: {{BRAND_ROW}}. e.g. precise or expensive: sparse piano, low strings, air.
- Restraint: about 5 designed effects per 10 s, most of them where the music is sparse.
- Places: {{PLACES}}. e.g. a quiet studio under the UI section.

## Picture map

| t (s) | on screen | cue (kind / id) | sound need |
|---|---|---|---|
| e.g. 0.0-1.0 | cursor glides to the button | none | silence: continuous motion is left to the music |
| e.g. 1.0 | button press | click / ui:cursor:0 | the film's single click voice (press + release 85 ms later) |
| e.g. 2.0-2.7 | typing in the search field | key / ui:type:key:* | one sliced recorded keyboard; the music dips 6 dB |
| e.g. 5.0 | camera zoom | whoosh / cards:zoom:0 | 1 recorded swish, apex on the velocity peak |
| e.g. 6.0-6.4 | 3 cards pop in | pop / cards:card:* | soft recorded pops rising 2 semitones |
| e.g. 6.5-8.0 | build | riser / reveal:build:0 | the music's build, or a recorded cymbal roll ending 30 ms before the hit |
| e.g. 8.0 | hero reveal | hit / reveal:hero:0 | the DROP, plus 1 low recorded impact |
| e.g. 10.0 | logo lockup | bell / logo:lockup:0 | the music stops on a bar line; 1 soft note in key |

## Ambience zones

| from (s) | to (s) | place | bed |
|---|---|---|---|
| e.g. 0.0 | 7.0 | studio | quiet room tone, ducked under key actions |

## Usage rights

- Allowed: CC0, Public Domain Mark, CC BY (2.0-4.0), Mixkit free licences, Pixabay Content License.
- Forbidden: any NC, ND or SA term, Sampling+, personal or editorial use, custom terms, disputed terms.
- Not used: unclear "royalty free", platform rips, anything behind a login, AI-generated or code-made audio.
- Read the terms on the item's own page. Never invent a licence. When in doubt, drop the recording.

## Quality bar

- Effects: recorded, close and clean; lossless or HQ previews; no clipping; the floor between events stays under
  -55 dBFS; the 95 % rolloff is above 12 kHz; no untreated room on foley; no background events, voices or handling.
- hark: realness "real or pro-produced" on every file in use (an "uncertain" result needs its evidence in the
  manifest); match z >= 2.5 for the need it covers.
- Ambiences: clean, no single loud events, at least 40 s usable, true stereo.
- Music: professionally produced, full bandwidth, instrumental, a steady 4/4 inside the tempo range.
- Mix: -14 LUFS integrated, LRA 5-9 LU, true peak <= -1 dBTP after AAC. The peak moment is the drop. The logo sits
  8-20 dB under it. The end decays to silence.

## Files and tools

- Downloads: `audio/assets/<category>/<source>-<id>_<slug>.<ext>`, for example `fs-123456_desk-typing.ogg` or
  `mk-2001_trackpad-tap.wav`. Audio files only. Never an archive that holds programs. Never run a downloaded file.
- Manifests: `audio/assets/manifests/<category>.json` (fields in `templates/manifest_entry.json`).
- Rights audit: `audio/assets/manifests/rights.json`, written by `rights_audit.py` and the rights auditor.
- Design: `audio/sounddesign.json` (lead) and `audio/design/<category>.json` (one designer each).
- Review and QA: `out/audio-review/<category>/`, `out/qa/`.
- Measure: `{{PY}} -I {{SKILL}}/scripts/analyze.py <files> --png <dir>`; listen: `{{HARK}} rank`, `match`, `find`,
  `describe`, `realness`, `ab`, `compare`.
- Download etiquette: one file at a time, a pause between, no more than about 40 files per category.
