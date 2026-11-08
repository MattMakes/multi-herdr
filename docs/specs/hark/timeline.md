# hark timeline unit: spec index

Unit `timeline` of the hark clean-room rebuild. It covers 4 commands and their helpers:

- `hark timeline`: listen through a whole soundtrack or film: events, tags, sections, loudness, beat grid, sync of
  sound to picture cuts and cues, a text transcript.
- `hark sheet`: one annotated PNG of the same analysis for agents that can look at images.
- `hark critique`: a launch-video sound scorecard against a profile of professional launch films, with fixes.
- the video helpers: picture cuts (visual events), frames, stream info.

The spec is split into 5 files because it is longer than 60 KB:

| file | content |
|---|---|
| `docs/specs/hark/timeline.md` (this file) | overview, the exact interface, data assets, dependencies, open questions, coverage |
| `docs/specs/hark/timeline-analysis.md` | timeline behaviour, algorithms (events, swells, loudness, beat grid, cues, sync, tags, sections, transcript, cache), constants, errors, acceptance tests |
| `docs/specs/hark/timeline-analysis-tests.md` | timeline acceptance tests T1-T19 and evaluation targets |
| `docs/specs/hark/timeline-sheet-video.md` | video helpers and the sheet layout (every panel and marker), constants, errors, acceptance tests |
| `docs/specs/hark/timeline-critique.md` | critique scorecard (every metric, weight, tolerance, range rule, verdict, fix text rules, tips), the launch profile values, playbook facts, errors, acceptance tests |

Where each required section lives:

| section | location |
|---|---|
| Interface | this file, section 1 |
| Behaviour | analysis 1; sheet-video 1.1 and 2.1; critique 1 |
| Algorithms | analysis 2; sheet-video 1.2 and 2.2; critique 2 |
| Constants | analysis 3; sheet-video 1.3 and 2.3; critique 3 |
| Data assets | this file, section 2 |
| Errors and edge cases | analysis 4; sheet-video 1.4 and 2.4; critique 4 |
| Acceptance tests | analysis-tests (T1-T19 and evaluation targets); sheet-video 1.5 and 2.5 (V1-V6, S1-S7); critique 5 (C1-C14 and evaluation targets) |
| Dependencies on other units | this file, section 3 |
| Open questions | this file, section 4 |

"The source" is the upstream project. Names in this spec are ours. The `hark` package lives at
`skills/hark/engine/hark/` (decisions file). Module names below are proposals for the architect.

## 1. Interface

### 1.1 CLI

All three commands accept the shared flags of core-io: `--json` (print the full result as JSON, ASCII-escaped),
`--out FILE` (also write the JSON result to FILE, creating folders), `--threads N` (CPU threads for the model
runtime and BLAS). Without `--json`, the command prints the result's `text` field.

```
hark timeline FILE [--video VIDEO] [--cues CUES] [--from S | --start S] [--to E | --end E]
              [--detail low|normal|high] [--fresh] [--json] [--out O] [--threads N]
hark sheet FILE [-o PNG] [--video VIDEO] [--cues CUES] [--from S | --start S] [--to E | --end E]
              [--detail low|normal|high] [--width PX] [--json] [--out O] [--threads N]
hark critique FILE [--video VIDEO] [--profile PROFILE.json] [--fast] [--json] [--out O] [--threads N]
```

| flag | meaning |
|---|---|
| `FILE` | audio file or video. For a video, its audio is analysed and its own picture cuts are used |
| `--video` | the picture for cuts and frames (default: FILE itself when it is a video) |
| `--cues` | a JSON file `{fps, events: [{f, kind, id}]}` (or `t` seconds), JSON text, or `"1.5,3.2,..."` seconds |
| `--from`/`--start`, `--to`/`--end` | zoom window in seconds (floats) |
| `--detail` | model budget, default `normal`: low = 12 CLAP-tagged events, no AST; normal = 20 events, AST on the top event and 3 section crops; high = up to 96 events, 16 AST labels, finer sections |
| `--fresh` | recompute everything (no cached result, no cached model outputs) and store the new result |
| `-o` | output PNG; default `<file stem>_sheet.png` in the current folder |
| `--width` | image width in px, default 2000, clamped to 1000-2400 |
| `--profile` | a profile JSON made by the listen unit's profile command (default: the bundled 7 launch films) |
| `--fast` | skip the AudioSet (music and speech presence) and realness passes |

Exit codes: 0 on success. `critique` prints `hark critique: error: <message>` on stderr and exits 1 on an input
error or a decode error. `timeline` and `sheet` raise input errors that the core-io CLI turns into its standard
error line and exit code (core-io decides the format; Q4).

### 1.2 Python API (proposed module names)

```
hark.timeline.timeline(path, video=None, cues=None, start=None, end=None, detail='normal',
                       cache=True, fresh=False) -> dict
hark.timeline.detect_events(x, zthr=3.0, min_contrast_db=3.0, min_gap=0.03, lead=True) -> list[dict]
hark.timeline.level_envs(x) -> {'full': array, 'high': array}
hark.timeline.level_peak(envs, t, radius=0.15, min_rise_db=3.0, min_fall_db=2.0, local=0.3) -> dict | None
hark.timeline.loudness(x, at_start=True, at_end=True) -> dict
hark.timeline.music_grid(m, min_bpm=60, max_bpm=200) -> dict
hark.timeline.read_cues(cues) -> (list[dict], fps | None)
hark.timeline.sync(times, events, beats=None, frame_s=None, levels=None) -> {'items': [...], 'stats': {...}}
hark.timeline.top_accents(events, n=3) -> list[dict]
hark.timeline.novelty_bounds(m, kernel_s=4.0, zthr=3.0) -> list[(t, z)]
hark.sheet.sheet(path, out_png, video=None, cues=None, start=None, end=None, detail='normal', width=2000,
                 tl=None) -> dict
hark.video.info(path), has_video(path), frame_diff(path, cache=True), diff_events(d, fps, ratio=4.0,
                 floor=0.012, big=0.15, gap=0.12), cuts(path, method='diff', threshold=0.3, min_gap=0.2,
                 cache=True), frame(path, t, width=320), frames(path, times, width=320)
hark.critique.critique(path, profile=None, video=None, use_ast=True, use_realness=True) -> dict
hark.critique.scorecard(values, profile, chance=None, lossy=False) -> dict
hark.critique.real_thresholds(file=True) -> {'lo', 'hi'}
```

`x` is a float32 array at 48 kHz, shape `(n,)` or `(n, 2)`; `m` is mono. `sheet(..., tl=r)` reuses a timeline
result. The modules are callable as their main function when core-io provides that convenience. The surfaces unit
exposes `timeline`, `sheet` and `critique` as MCP tools with the same parameters.

### 1.3 Timeline result (JSON)

Top level:

| field | type | meaning |
|---|---|---|
| `file` | str | the path as given |
| `video` | str or null | the picture used |
| `start`, `end`, `duration` | float | the analysed window in file time (3 decimals) |
| `file_duration` | float | the whole file |
| `context` | [float, float] | the analysed context in file time (equals `[start, end]` unless zoomed) |
| `has_audio` | bool | false for a picture-only video |
| `lossy` | bool | the audio was decoded from a lossy codec |
| `loudness` | object | below |
| `music` | object | beat grid, below |
| `events` | list | window events, below |
| `sections` | list | below |
| `sync` | object | `{cuts?: {items, stats}, cues?: {items, stats}}` |
| `cuts` | list[float] | picture cuts inside the window |
| `cut_method` | str or null | `frame difference` when there is a picture |
| `fps` | float or null | video frame rate |
| `model_calls` | object | `clap_event_windows`, `ast_event_windows`, `clap_section_windows`, `ast_section_windows` |
| `timing_s` | object | `decode_s`, `loudness_s`, `events_s`, `beats_s`, `cuts_s`, `tag_events_s`, `sections_s`, `total_s` |
| `detail` | str | low, normal or high |
| `summary` | str | one ASCII paragraph |
| `text` | str | the transcript (starts with the summary) |
| `cached` | bool | true when served from the result cache |

`loudness`: `integrated_lufs` (null when all gated out), `true_peak_dbtp` (null for digital silence),
`digital_silence`, `max_momentary {t, lufs}`, `momentary {t0, dt, lufs[]}`, `max_short_term {start, end, lufs}`,
`short_term {t0, dt, lufs[]}`, `lra_lu`, `silences [{start, end, dur, kind, level_lufs, where?}]`, `clipping
{samples, runs, times[]}`. `kind` is `digital silence`, `near silence` or `drop`; `where` is `lead-in`, `tail` or
`whole file`.

`music`: `tempo`, `tempo_alt`, `beats[]`, `downbeats[]`, `pulse_clarity`, `beat_regularity`, `confidence`
(`clear`, `weak`, `none`), `note`. The empty grid has only `tempo` null, `beats` [], `downbeats` [], `confidence`
`none` and `note`.

Event:

| field | meaning |
|---|---|
| `t` | attack time (hit) or peak time (swell), file time, 3 decimals |
| `shape` | `hit` or `swell` |
| `band` | `full`, `low` or `high` |
| `contrast_db` | rise in the band where the event stands out most |
| `prominence_db` | rise of the K-weighted level over the bed just before (broadband) |
| `bed_db` | that bed, about LUFS, floored at -80 |
| `z` | onset novelty in robust standard deviations over about 2 s |
| `length_ms` | attack to decay |
| `rise_ms` | swells only: build-up length |
| `starts_file` | only on the event at the very start of a file |
| `peak_dbfs`, `local_lufs`, `centroid_hz` | levels and the excess spectral centroid |
| `beat_offset_ms` | event minus the nearest beat, when the grid is clear or weak |
| `tags` | tagged events: `[{tag, kind, z}]` top 3 |
| `kind`, `label`, `exposed` | tagged events: kind word, short phrase, at least 12 dB over the bed |
| `ast` | AST-labelled events: `[[label, p], ...]` |

Section: `start`, `end`, `windows`, `coherence`, `music_genre`, `music_mood`, `instruments`, `ambience`, `voice`
(each `[{tag, cat, z, cos}]`, top 2), `lufs`, `digital_silence`, `ast {Music, Speech, Silence}` and `ast_top`
(when AST ran), `label`.

Sync item: `t`, `kind`?, `id`?, `offset_ms`, `status` (`OK`, `near`, `swell`, `peak`, `off`, `no sound`),
`event_t`?, `event_prominence_db`?, `event`?, `accent_offset_ms`, `beat_offset_ms`?.

Sync stats: `n`, `hit_within_1_frame` or `hit_within_40ms`, `hit_within_100ms`, `median_abs_offset_ms`,
`answered_by_swell`, `answered_by_level_peak`, `no_sound[]`, `frame_ms` or `tolerance_ms`, `chance_within_1_frame`
or `chance_within_40ms`, `chance_within_100ms`, `accent_min_db`, `accent_within_1_frame` or `accent_within_40ms`,
`accent_chance_within_1_frame` or `accent_chance_within_40ms`, `accent_within_100ms`,
`accent_chance_within_100ms`, `on_beat`, `on_beat_chance`. Absent keys follow the rules of analysis 2.7.

### 1.4 Sheet result and critique result

Sheet result fields: sheet-video part, section 2.2 "Output". Critique result fields: critique part, sections 2.10
and 2.11; a metric row is described in critique 2.6.

### 1.5 Cue file format

```json
{"fps": 60, "events": [{"f": 120, "kind": "whoosh", "id": "logo-in"}, {"t": 3.25, "kind": "click"}]}
```

Also accepted: a bare list of such objects or of numbers; `cues` in place of `events`; `time`, `sec`, `seconds`
for `t`; `frame` for `f`. Analysis part, section 2.6.

## 2. Data assets

| id | asset | format | origin | mark | fallback |
|---|---|---|---|---|---|
| A1 | bundled launch profile (`hark/data/launch_profile.json`) | JSON: `version` (2), `n`, `metrics {key: {n, median, p10, p90, min, max, values[]}}`, `chance` (same stats for the 4 sync shares), `arc {median, p10, p90}` (11 points each), `films [{id, style, duration, has_video, metrics, chance, arc}]`, `warnings[]`, `summary` | the listen unit's profile command run over 7 third-party professional launch films (web MP4, 128 kbps AAC), ids `ref_1..ref_7`, neutral style lines, no paths or file names | NEEDS-DATA (the films are not available) | ship a seed profile with `n` 7, the summary statistics of critique table 3.4 per metric, the chance medians and the median arc, and `films: []`. Scoring then works. Leave-one-out and the reference match need per-film values, so they do nothing with the seed (a reference film is then scored in-sample). Users can build their own profile from their own films. Q1 |
| A2 | tag background means | 49 numbers, one per event phrase (analysis table 3.4) | the mean CLAP cosine of each phrase over 536 real background events: the events the timeline would tag in 8 synthetic tuning mixes plus 4 launch films, and every placed one-shot of the mixes | REGENERATE when we have a CLAP checkpoint and background audio; the shipped numbers are facts | ship the table as constants. With a different CLAP checkpoint, regenerate: collect at least about 500 events from mixes and films, embed their excerpts as analysis 2.8 does, and take the per-phrase mean cosine |
| A3 | listening vocabulary (`vocab.json`) | categories of terms plus prompt templates | listen unit | owned by listen | none needed here; the timeline reads the categories `music_genre`, `music_mood`, `instruments`, `ambience`, `voice` and the templates |
| A4 | synthetic tuning mixes | 8 mixes of 30 s, each with a bed-only file and a JSON of ground truth | built from real recordings (music and ambience beds, one-shot hits, whooshes) with a fixed random seed; procedure below | REGENERATE from any licensed recording library; NEEDS-DATA for the exact source library | the procedure below with our own licensed recordings (the sound-sourcing skill) |
| A5 | timeline evaluation film | a 60 s motion-graphics film at 60 fps with its soundtrack, music, ambience and SFX stems, the mixer's placement report (every sample with time, alignment, sample name) and a 153-cue list | the source author's own project | NEEDS-DATA | use the analysis acceptance tests, plus any project of ours where a script placed the sounds (the forge and bank units can render a soundtrack with a placement list); report recall within 30 ms and offset errors as in the analysis tests |
| A6 | launch evaluation set | per-film metrics of the 7 references and 2 test films, the known-bad mix list, results | measured on the same 7 films and the test film with a real and a synthesized soundtrack | NEEDS-DATA (films), REGENERATE (known-bad mixes from any soundtrack with ffmpeg) | rebuild the known-bad set (critique 5) from a licensed soundtrack of ours: quiet -12 dB, hot +12 dB, mono fold, low-pass 3 kHz twice, 2.5 s leading silence, 4 s trailing silence, 12 s fade-out, 2.5 s muted hole, 20:1 compression from -36 dB plus a limiter, stems alone, a synthesized bed, a 30 s 440 Hz sine, 30 s pink noise |

**Tuning mix procedure (A4)**, for the clean implementer:

1. Pools: music files, ambience files, one-shot sources (foley, garden, keyboard, magic, toy categories) and motion
   (whoosh) files, none of which appear in the evaluation film.
2. One-shots: in each source file (first 60 s, mono), find up to 3 isolated hits: 5 ms RMS frames in dB; visit
   frames loudest first and stop at frames more than 25 dB under the file maximum; skip frames within 0.3 s of a
   used one; require 0.2-0.02 s before to stay at least 20 dB under the peak and 0.06-0.5 s after to stay
   at least 12 dB under it; cut 0.6 s from 0.2 s before the peak with a 50 ms fade-out. The attack is the first
   millisecond (1 ms max-abs envelope) at or above 10 % of the envelope maximum.
3. Whooshes: the first 4 s with a 30 ms fade-out; the alignment time is the envelope peak.
4. Beds: a random excerpt (tiled when short), normalised to unit RMS.
5. Each mix (seed 20261014, 30 s): 2-4 beds with hard switches at random times between 4 s and `dur - 4` s, at
   least 4 s apart; beds alternate music and ambience with a 30 % chance to flip; music at -24..-16 dB RMS,
   ambience at -40..-28 dB. Events from a random start 0.3-0.8 s, every 0.35-1.6 s (plus 1 s after a whoosh) until
   `dur - 0.8` s; 12 % are whooshes (peak-aligned), the rest one-shots (attack-aligned); each at a random SNR of
   -14..+14 dB against the bed RMS over the 0.3 s from 50 ms before (peak measured as the maximum 10 ms RMS).
   Normalise the mix (and the bed file by the same gain) so the peak is at most 0.89.
6. Ground truth per event: time, alignment (`attack` or `peak`), SNR, category, file. Mix JSON: beds (start, end,
   kind, file, RMS), boundaries, events, duration.

## 3. Dependencies on other units

| unit | what this unit needs from it |
|---|---|
| core-io | 48 kHz decode (`load` with start/end and mono options), probe (codec, lossy flag, channels, duration, video stream with fps, width, height, `has_video`, probe errors), file id for cache keys, atomic replace, the decode error type, mono fold, dB helper, audio and video extension sets, BS.1770 K-weighting, loudness curves, integrated loudness, LRA, true peak, frame RMS, the cache root and data folder, `fmt_t`, JSON writer, `emit` (text or `--json`, `--out`), the error-line format, the shared CLI flags, the CLAP model (`embed_audio` with repeat padding, `embed_text` with a shared text cache, a model cache folder) and the AST model (`probs`, `top(p, k, min_p)`, `prob_of(p, label)`, labels), on-demand model install |
| listen | the listening vocabulary asset (A3); the profile module: metric table (group, label, unit, decimals), `film_metrics`, the audio pass (onsets, loudness curves), `aggregate`, `load_profile`, `media_problem`, `unscorable`, `fmt_value`, the shape-metric set and the 10 s and 3 s limits, the input error type, the AudioSet window length (5 s) and music threshold (0.3); the realness model (`realness(path)` with `p_real`, `verdict`, `synth_windows`, `segments`, `summary`; file and clip thresholds) |
| timeline | this unit; `critique` calls the timeline matcher (`detect_events`, `level_envs`, `sync`) and `sheet` calls `timeline` |
| forge-core | none directly; its renders can be evaluation input (A5 fallback) |
| recipes-impacts | none directly; impact and UI renders are useful synthetic test material |
| recipes-motion | none directly; whoosh and riser renders exercise the swell and level-peak paths |
| bank-kit-sfx | none directly; a kit render with a placement list can replace the evaluation film (A5) |
| fit | none |
| surfaces | exposes `timeline`, `sheet` and `critique` through MCP and the gallery; it needs the result fields of section 1 |
| skills-and-docs | the `hark` skill documents these commands; the `sound-sourcing` skill is the target of the realness fix text; the playbook facts of critique 3.5 feed the skill docs |

## 4. Open questions

| id | question | proposed answer |
|---|---|---|
| Q1 | May we ship the summary statistics of the 7 reference films (critique table 3.4) as the seed profile? They are numeric facts, not the films, but the bundled per-film values are left out. | Yes: ship the seed profile with summary statistics only and `films: []`. |
| Q2 | Should the critique fix texts and tips keep the source's exact numeric advice (for example "cut 300-500 Hz by 2-3 dB")? | Yes for the numbers (they are facts); the wording must be our own. |
| Q3 | Timeline without the ML extras: the decisions say an ML command prompts to install them. Should `timeline` and `sheet` (which need CLAP for tags and sections) prompt, or should they run the measurements and leave tags and sections empty? | Prompt like other ML commands; add a `--no-models` path later only if the operator asks. |
| Q4 | `timeline` and `sheet` error output: the source raised plain errors through its generic CLI handler. Should they use the same `hark <command>: error:` line and exit 1 as `critique`? | Yes, one error-line format for every command (core-io). |
| Q5 | The default fonts: the source names a specific font family. | Use the plotting library's default sans-serif and monospace faces; the layout depends only on the monospace width factor 0.602. |
| Q6 | The `high` case of `lra` or `crest_lu` with an empty body falls back to the `low` sentence (critique 2.7). It is unreachable for files of 10 s or more. | Keep it (it cannot fire); a clean implementation may instead emit the high sentence without the stretch. Either is acceptable. |
| Q7 | The excess-centroid FFT truncates excerpts longer than 4096 samples (analysis 2.2). | Keep it for parity with the evaluation numbers. |
| Q8 | The `cut_method` value: the source names its own module. | Use `frame difference`. |

## 5. Coverage of the source files of this unit

| source file | covered in |
|---|---|
| timeline module | analysis part (all), index 1 |
| sheet module | sheet-video part 2 |
| video module | sheet-video part 1 |
| critique module | critique part 2-4 |
| bundled launch profile JSON | index A1, critique 3.4 |
| timeline evaluation (mixes, tune, tag background, tag tuning, label tuning, pulse, tempo, section and swell tuning, runner, results) | analysis 3.4 and 5, index A2, A4, A5 |
| launch evaluation (films, negatives, refs, results, runner) | critique 5, index A1, A6 |
| launch-video sound playbook | critique 1 and 3.5 |
