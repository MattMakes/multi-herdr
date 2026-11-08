# Troubleshooting

## Mixer and master

| symptom | fix |
|---|---|
| `design not found` | the path resolves against `--root`; check both |
| `fragment X: samples name 'y' is already defined` | 2 fragments use 1 name; rename with the fragment prefix |
| `pool P: unknown sample S` | the pool names a sample nobody defined; a slice group needs `group#*` |
| an empty pool, or a slice group with no onset | widen the window, lower `delta` (0.05) or `gate_db` (-24) |
| `reverse` refused | the design keeps recordings in their real direction; find a recording with that shape |
| a comment key inside `buses` refused | comment keys must start with `_`; other bus keys must be `sfx`, `amb` or `music` |
| `N recording(s) missing; nothing written` | every missing file is listed with its kind and name; download it or fix the path |
| no cue file with rules | pass `--cues`, or set `cues` in the design |
| no duration and no cue total | set `duration` in the design or `total` in the cues |
| `rubberband failed` | ffmpeg lacks the Rubber Band filter; use `stretch: none` (tempo within about 0.2 %) or another ffmpeg build |
| the mix is all silent when mastering | exit 1 and the old `--out` stays; check trims (`--dump`), gains and cue matches |
| `master gate FAILED` | read `out/qa/master.json`; lower `master.lufs` by 1 for a dynamic track, or ride the loudest peak down |
| a solo render plays other takes than the full mix | name every rule: an unnamed rule is keyed by its index, which changes when fragments are left out |
| a solo render has no premaster | `--only` writes the unmastered sum to `--out` itself |
| an ambience bed is refused by name | its window is shorter than the 1.5 s loop crossfade; widen `src_start..src_end` |
| a placement starts before 0 or ends after the film | the outside part is dropped; QA lists it under `cut_by_film_ends` |
| the first run is slow | every recording decodes once into `out/audio/cache`; later runs read the cache |

## QA

| symptom | fix |
|---|---|
| `qa.py: ... not found ... (--stems)` (exit 2) | pass the flag it names; solo QA needs `--only` and the solo paths |
| stems of different lengths, or a master of another length | exit 2: render again so stems and master come from 1 run |
| trace sections skipped: "the trace and the report disagree" | the trace is from another run or another `--only`; mix again with `--trace` |
| "N placement(s) could not be rebuilt" | the design changed after the mix; mix again |
| audibility `skipped` on a solo render | expected: no bed to judge against; silent effects still fail |
| harshness "not judged" | there is no music stem to compare with |
| a click "inside a recording" | dump the sample and look at its spectrogram; trim around a defect or switch takes; if it is the sound's own transient, add `qa_ok` |
| many "bright" notes | fine when the palette is bright (plastic, glass, bells); else a high-shelf cut or a darker take |
| `qa.py --listen` says hark cannot be imported | run QA without `--listen` and call `{{HARK}} timeline` and the rest directly, or repair the venv |

## Rights and credits

| symptom | fix |
|---|---|
| every recording shows "no page reading" | `rights_seen` and `rights_confirmed` are empty; run the rights auditor brief |
| `rights_seen` reads as another licence than the manifest | reread the page, then correct the manifest's `rights` or reject the item |
| used in the film but in no manifest | add an entry and audit it, or remove the sound |
| the entry id and the Freesound page disagree | entry, page and download refer to different sounds; download again from the right page |
| credits list PROBLEMS | a recording in use is forbidden, unknown or unconfirmed, or has no author, title or page_url |
| a Freesound preview returns 404 | the folder is the sound id divided by 1000 (integer); after the `_` comes the uploader's numeric id from the page |
| Pixabay answers 403 or a challenge page | it blocks scripted downloads; ask the user to download |
| a Mixkit item has no page | record the category page and the title listed there |
| a file decodes with errors | download it again, preferring the lossless or HQ version |

## Delivery

| symptom | fix |
|---|---|
| true peak above -1 dBTP after AAC | lower `master.ceiling` from 0.77 to 0.72 and render again; measure the MP4 with `{{HARK}} measure film.mp4` (`master.py --measure` reads WAV and FLAC only) |
| `master.py --measure` fails on `last_480_zero` | the file was not mastered by the chain; use `{{HARK}} measure` for a delivered file |
| the audio stops before the picture | the soundtrack is shorter than the video; set `duration` to the picture length (the mux prints the difference) |
| sync looks wrong after the mux | the mux copies the picture and cuts the audio to it; check the cue frames and cross-correlate the MP4 audio with the WAV (lag 0); never shift the audio to fix a picture offset |
| `mux: the keep-old file exists` | move that file or pass another `--keep-old`; the video is untouched |
| paths with spaces | quote them: `--root "/path/to/my film"` |
