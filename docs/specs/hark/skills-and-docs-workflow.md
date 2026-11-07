# skills-and-docs, part 2: the `sound-sourcing` skill: workflow, roles, templates and craft rules

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`. Scripts: part 3
(`skills-and-docs-scripts.md`) and part 4 (`skills-and-docs-mixer.md`).

## 1. Purpose and trigger

`sound-sourcing` builds a soundtrack for a code-made video or animation from REAL recordings: one licensed music
track, real foley and effects, real field ambiences. Typical films: launch films, product and feature videos, UI
walkthroughs, logo stings, explainers, trailers, animated shorts, made in any renderer (Remotion, three.js, GSAP, a
game engine capture). The agent cannot hear, so every choice is measured.

The skill triggers when a video needs sound design, effects, foley, ambience, music, a score, UI click / whoosh /
riser / impact / logo-sting sounds or a music edit to picture; when a synthesized or random-sounding soundtrack must be
replaced with recordings; or when the user asks to source, check usage rights of, credit, mix, master, critique or mux
audio for a film.

When the brief asks for MADE sounds (code synthesis, the bank, a kit), the `hark` skill is the tool (part 1, loops 4
and 5). When it asks for recorded sound, this skill is.

The hard rule of the skill: code selects, trims, places, levels, pans, equalises, pitches by resampling,
time-stretches the music by small ratios, sums and masters. Code never generates a tone, a noise or a reverb tail.

The quality bar: a professional launch film. Every sound has a visible cause. One material palette. The music's drop
on the hero reveal. Restraint everywhere else. A resolved logo. A tail into silence.

## 2. Placeholders in briefs

The lead fills every placeholder before a brief reaches a sub-agent. An unfilled placeholder must never reach one.

| placeholder | value |
|---|---|
| `{{PY}}` | the full path of the hark venv Python (`~/.local/state/hark/venv/bin/python`), which has numpy, scipy, soundfile, soxr, librosa, pyloudnorm and the hark package. |
| `{{HARK}}` | the hark launcher, `skills/hark/scripts/hark`. |
| `{{SKILL}}` | the `sound-sourcing` skill folder. |
| `{{ROOT}}` | the film project root. |
| `{{CATEGORY}}`, `{{PREFIX}}` | the category an agent owns and its name prefix. |
| `{{FILM}}`, `{{VIDEO}}`, `{{CUES}}` | the film's name, the rendered film, the cue file. |
| `{{ROUND}}` | the critique round number. |
| `{{RENDER}}` | the folder of the render under review (soundtrack.wav, stems). |
| `{{REPORT}}`, `{{TRACE}}`, `{{QA_JSON}}` | its mix report, placement trace and QA report. |
| `{{CHANGED}}`, `{{FIXED}}` | what changed since the last round (check those hardest); what is already fixed (do not report again). |
| `{{BAR_REFERENCE}}` | a one-line quality bar, for example a premium launch film. |
| `{{NEEDS}}`, `{{NEED_DESCRIPTION}}`, `{{SINGLE_EVENT_DESCRIPTION}}`, `{{EVENTS}}` | per-agent needs, query texts and cue classes. |
| `{{KEY}}`, `{{BPM}}`, `{{BEATS_PER_BAR}}`, `{{BAR}}`, `{{FILM_BARS}}`, `{{DROP_T}}`, `{{DROP_BAR}}`, `{{BREAKDOWN}}`, `{{LOGO_T}}`, `{{DURATION}}` | the music facts and key moments. |
| `{{CRITIC_REPORTS}}` | the critics' JSON returns, for the fixer. |

The source asks for `python -I` with an interpreter that can import its package. In hark, `{{PY}}` runs the skill
scripts and `{{HARK}}` runs the hark commands (`{{HARK}} realness ...`). The commands below use those 2.

## 3. The process

| step | owner | output | gate |
|---|---|---|---|
| 0 cue list | lead | `out/cues.json` | sync frames computed from the picture code |
| 1 brief | lead | `audio/SOUND_BRIEF.md` | every cue has a sound need, or "nothing" |
| 2 sourcing, 1 agent per category, in parallel | sourcing agents | `audio/assets/<cat>/`, `audio/assets/manifests/<cat>.json` | every file measured; hark realness "real or pro-produced" on the trim used (an "uncertain" only with its evidence and a trim-level check written in the manifest) |
| 3 usage-rights audit | rights auditor | `audio/assets/manifests/rights.json` | `rights_audit.py` exit 0 for what the film uses |
| 4 music edit | music supervisor | `audio/design/music.json`, `audio/assets/manifests/music_choice.json` | grid verified, drop on the reveal, clean splices |
| 5 design fragments, 1 per category, in parallel | designers | `audio/design/<cat>.json` | solo render + QA clean |
| 6 integrate | lead | soundtrack, stems, report, trace | master gate (+-0.5 LU, <= -1.5 dBTP) |
| 7 QA and listening | lead | `out/qa/qa.json`, hark timeline, sheet, critique | no QA fail |
| 8 critique, 5 lenses, in parallel, read-only | critics | ranked issues with tested operations | |
| 9 fix | exactly 1 fixer | the design | QA: nothing that passed now fails |
| 10 master, credits, mux | lead | `CREDITS.md`, description text, the film | credits exit 0, mux lag 0 |

Parallel agents never edit the same file. One writer per file avoids lost edits and contradictory fixes.

### 3.1 Roles and ownership

| role | brief (reference file in hark) | writes | reads |
|---|---|---|---|
| lead | SKILL.md | the brief, `audio/sounddesign.json` (includes, buses, master), the final reports | everything |
| sourcing agent | `references/role-sourcing.md` | `audio/assets/<cat>/`, its manifest, `out/audio-review/<cat>/` | the brief |
| rights auditor | `references/role-rights-audit.md` | the agent fields of `rights.json`; may correct a manifest's rights, rights_url, author, title, page_url, attribution when the page says otherwise (and says so) | manifests, item pages |
| music supervisor | `references/role-music.md` | `music_choice.json`, `audio/design/music.json`, `out/audio-review/music/` | music candidates, the picture map |
| designer | `references/role-designer.md` | `audio/design/<cat>.json`, `out/audio-review/<cat>/` | its manifest, the cues, solo renders |
| critic (1 per lens) | `references/role-critic-<lens>.md` | only `out/audio-review/critic_<lens>/` | the render, the design, the trace |
| fixer | `references/role-fixer.md` | every fragment and the top-level buses and master, during the fix round only | all critic reports |

The source keeps these briefs as prompt templates with placeholders. Ticket 22 turns them into hark reference files
in our own words; section 5 lists each brief's job, steps and checks.

### 3.2 Step detail

0. Cue list. Every sound sits on a frame that the picture code computes: a contact frame, a pop-in's 50 % frame, a
   move's velocity peak, a riser's hit, the frame the wordmark becomes readable. Write the cue format (part 4, 1.1)
   from the timeline constants when the renderer does not export it. Stable ids (`act:thing:n`) let rules match
   classes of action with a regex.
1. Brief. Copy the brief template (4.2) to `audio/SOUND_BRIEF.md` and fill it: the concept, the picture map (every
   cue, what is on screen, the sound need), ambience zones, allowed rights, quality bar, files. Pick the brand's
   personality row (7.3). Every sub-agent gets the brief verbatim: it is the contract.
2. Sourcing per category. Typical launch-film categories: `ui` (clicks, taps, toggles, pops), `keyboard`, `motion`
   (whooshes, risers), `impacts` (reveal hits, the logo), `ambience`, `music`. A character film adds `foley`, its
   worlds and its objects. 15-40 candidates per category, 3-6 per need. Rights read on each item's own page; polite
   sequential downloads; every file measured:
   ```
   {{PY}} -I {{SKILL}}/scripts/analyze.py audio/assets/ui/* --png out/audio-review/ui --out out/audio-review/ui.json
   {{HARK}} realness audio/assets/ui/
   {{HARK}} rank "a single crisp trackpad click" audio/assets/ui/ --top 5
   {{HARK}} match <file> "a soft cork pop" --alts "a gunshot" "a balloon burst"
   {{HARK}} find <long take> "a single mechanical key press" --top 8
   ```
3. Usage-rights audit. The auditor records what each page says; `rights_audit.py` checks the rest:
   `{{PY}} -I {{SKILL}}/scripts/rights_audit.py --root . --report out/qa/mix_report.json [--fetch]`.
   Nothing unconfirmed reaches the design.
4. Music. Write the arc (4.6), then `arrange.py <track> --film-bpm 120 --arc audio/arc.json --out
   out/audio-review/music/arrange.json`. Under LOW downbeat confidence, choose between the 2 printed grids from the
   first note, the section entrances and the chord changes, then run again with `--phase N`. Then edit by judgement:
   harmony continues across each splice, the drop's downbeat on the hero frame, the final chord on the logo,
   `tail_fade` into the end.
5. Design fragments. Each designer writes samples (trims), slices, pools, rules on cue events, free placements, beds
   and (the music supervisor) the music block. It verifies alone:
   ```
   {{PY}} -I {{SKILL}}/scripts/mix.py --only ui --dump out/audio-review/ui/dump
   {{PY}} -I {{SKILL}}/scripts/mix.py --only ui --trace out/audio-review/ui/adds.json --out out/audio-review/ui/solo.wav
   {{PY}} -I {{SKILL}}/scripts/qa.py --only ui --report out/audio-review/ui/solo_report.json --stems out/audio-review/ui/solo_stems --trace out/audio-review/ui/adds.json --out out/audio-review/ui/qa.json
   {{HARK}} timeline out/audio-review/ui/solo.wav --cues out/cues.json
   ```
   A designer never touches another fragment or the top level.
6. Integrate: `mix.py --root . --design audio/sounddesign.json --cues out/cues.json --trace out/qa/adds.json`. Fix
   blockers (missing files, a failed gate) before critique.
7. QA and listening: `qa.py --layers`; `qa.py --listen --realness --video out/film.mp4`; `{{HARK}} critique
   out/audio/soundtrack.wav --video out/film.mp4`; `{{HARK}} sheet out/audio/soundtrack.wav -o out/qa/sheet.png
   --video out/film.mp4 --cues out/qa/cues_sounded.json`.
8. Critique: 5 read-only critics in parallel (5.4-5.8). Each tests its fixes as a variant in its own folder
   (`variant.py --ops .../ops_v1.json --out .../v1 --render --qa`) and returns ranked issues with exact operations and
   before -> after numbers.
9. Fix: 1 fixer merges the operations, resolves conflicts, checks the merged variant against the previous `qa.json`,
   and applies it (`variant.py --apply`, originals backed up). Confirmed deliberate exceptions get `"qa_ok":
   "<reason>"`.
10. Master, credits, mux. The mixer masters (-14 LUFS, true-peak limiter, ceiling 0.77; gate +-0.5 LU and <= -1.5
    dBTP on the WAV, about -1 dBTP after the AAC encode). Then `rights_audit.py --report out/qa/mix_report.json`
    (exit 0), `credits.py --root . --design audio/sounddesign.json --rights audio/assets/manifests/rights.json`,
    `mux.py --video out/film.mp4 --audio out/audio/soundtrack.wav --out out/film_sound.mp4`, then `{{HARK}} timeline
    out/film_sound.mp4 --cues out/qa/cues_sounded.json` on the finished file, and a cross-correlation of the MP4's
    decoded audio against the WAV (lag 0 samples). Paste `out/CREDITS_description.txt` into the video description:
    CC BY credits are required there.

### 3.3 Rounds, stopping, reporting

- Minimum: 1 critique round and 1 fix round. A second round when the first fix changed levels or the music edit.
- Stop when QA shows no fail, every remaining warn carries a reason (`qa_ok`), and the director lens has no P0.
- Every number in a report comes from a command that ran, and the report lists the commands.
- Keep `out/audio-review/` (critic folders, variants, dumps): it is the audit trail of every number.
- QA findings are questions for a critic, not orders.

What the source's QA found on its finished reference mix (60 s, 83 recordings, 264 placements, already through 5
critics and a supervisor): 7 master clicks, all inside recordings (crisp transients the critics had accepted); 5 of 153
events masked (busy stacking moments); 3 events hot at 2-6 kHz; 2 edge defects, both deliberate (a 25 ms choke, a
hard music cut into a quiet section); no sync defect, with 4 events off their cue by exactly a rule offset; 2 cue
events unsounded. This shows the expected scale of findings on a good mix.

## 4. Templates (`skills/sound-sourcing/templates/`)

hark writes every template fresh. Fields are the interface; example values are placeholders.

### 4.1 `cues.json`

The cue format of part 4, 1.1, with a `_doc` comment, `fps`, `bpm`, `total`, `acts`, `cue` and a short example event
list for a 12 s demo at 60 fps and 120 BPM: a cursor click with `pan`, a run of `key` events with `variant` word and
space, a confirm click, a whoosh with `dur` and `apexFrac`, 3 card pops with spread pans, a riser and a hit on the same
frame, a bell on the lockup.

### 4.2 `SOUND_BRIEF.md`

A Markdown brief with `{{FIELD}}` slots and example rows the lead deletes. Sections:

| section | content |
|---|---|
| header | the final picture (video path, duration, fps, BPM grid with beat and bar seconds), the cue file, and the rule that the soundtrack uses only real recorded or produced audio (one music track edited at bar lines, real foley and effects on the cues, real ambiences where the picture has a place) and that code only selects, trims, places, levels, pans, equalises, crossfades, stretches the music slightly, pitches by resampling and masters. The bar reference. |
| sound concept | 4-8 bullets: the palette (one family of close-miked sources), the music's character and arc (soft intro, groove, breakdown time, the drop time on the hero reveal, the resolve under the logo, tail to the end, instrumental, tempo range), restraint (about 5 effects per 10 s), places. |
| picture map | a table `t (s) | on screen | cue (kind / id) | sound need`. Example rows: nothing under a cursor glide; the film's one click voice; a sliced keyboard with the music dipping 6 dB; 1 real swish on the velocity peak; soft pops climbing 2 semitones; the music's build or a real cymbal roll ending 30 ms before the hit; the DROP on the hero reveal with a low real impact; the music stopping on a bar line and 1 soft note in key on the lockup. |
| ambience zones | where each bed plays. |
| rights | the allowed and forbidden table of 6.1, in short. Never fabricate a licence. |
| quality bar | effects: real, close, clean; lossless or HQ previews; no clipping; floor under -55 dBFS between events; 95 % rolloff above 12 kHz; no untreated room on foley; no voices, handling noise or background events. hark: realness "real or pro-produced" for every file used; match z >= 2.5 for the need it fills. Ambiences: clean, no single loud events, at least 40 s usable, real stereo. Music: professionally produced, full bandwidth, instrumental, steady 4/4 in the tempo range. Mix: -14 LUFS, true peak <= -1 dBTP after AAC, LRA 5-9 LU, the loudest moment on the drop, the logo 8-20 dB under it, a tail into silence. |
| files and tools | the download folder and naming `<source>-<id>_<slug>.<ext>` (for example `fs-450282_mech-typing.ogg`, `mk-1386_keyboard-typing.wav`); audio files only, never an archive with programs, never run anything downloaded; the manifest shape; the rights audit; the design files; review and QA folders; the analyze and hark commands; politeness (sequential downloads, a pause, at most about 40 files per category). |

### 4.3 `manifest_entry.json`

A JSON list. One object per downloaded recording in `audio/assets/manifests/<category>.json`. Every field is read
from the item's own page. Never guess the rights.

| field | meaning |
|---|---|
| `id` | `<source prefix>-<item id>`: `fs-` Freesound, `mk-` Mixkit, a short prefix for other sources. |
| `category` | the category. |
| `file` | the local path. |
| `title`, `author` | as on the page. |
| `source` | `freesound`, `mixkit`, ... |
| `page_url` | the item page (Mixkit effects: the category page that lists the item). |
| `download_url` | the file fetched. |
| `rights` | the licence as written, for example `CC BY 4.0`, `Mixkit Sound Effects Free License`. |
| `rights_url` | the licence URL. |
| `attribution` | for CC BY: the credit form the author asks for on the page, else `"<title>" by <author> (<page_url>), licensed under CC BY 4.0 (<licence url>)`. `null` for CC0, Mixkit, Pixabay. |
| `need` | the need it fills (from the picture map). |
| `rank` | 1 = best for that need. |
| `best_segment` | `[start, end]` seconds of the clean part. |
| `notes` | what it is, how it was recorded (from the page), what was measured, what to use and what to avoid, for example a press at 0.12 s and a release 85 ms later, 3 dB quieter, floor -78 dBFS, no room. |
| `analysis` | what `analyze.py` measured (duration, codec, kbps, src_rate, channels, peak_dbfs, lufs, noise_floor_dbfs, clipped_samples, attack_at, centroid_hz, rolloff95_hz, onsets). |
| `hark` | `{realness, match: {query, z, verdict}}`. |

### 4.4 `rights_entry.json`

One record of `rights.json` (part 3, 3.2). The auditor writes only `rights_seen`, `rights_confirmed` and
`agent_notes`; the template shows the computed part under a comment key so the auditor knows what the script adds.
`rights_seen` holds: the site, the page URL, the licence text as shown and its URL, the date read, the page's author
and title, and that the preview's id and user id match the download URL. `agent_notes` holds requested credit forms
and restrictions. Never set `rights_confirmed` without having read the page in this session.

### 4.5 `sounddesign.json`

The top level: `_doc`, `duration`, `cues`, `include` (music, ambience, ui, keyboard, motion, impacts, logo fragments),
`_buses_doc` (a reason for every ride: for example the music dips 6 dB under the typing, comes back for the
transition, lifts 1.5 dB into the drop, settles 3 dB down under the logo), `buses` (music with an `auto` ride, amb and
sfx at 0), `_master_doc` (why -14 LUFS and ceiling 0.77; lower to -15/-16 only for a written reason such as a dynamic
orchestral track the limiter would squash), `master` `{lufs: -14, ceiling: 0.77, no_glue: true}`.

### 4.6 `arc.json`

`{_doc, film_bpm, beats_per_bar, drop_film_bar, arc}`. `arc` has 1 number per film bar: the target loudness in dB
relative to the track's loud bars (0 = its biggest sections, -6 a groove, -10 a breakdown, -14 a soft intro). Written
from the picture map: soft under the cold open and the typing, the groove for the feature beats, a breakdown before the
turn, the build, the big section from the drop, a resolve under the logo. The example is a 30 s film at 120 BPM: 15
bars, the drop on film bar 9 (16.0 s), the logo from bar 13.

### 4.7 `design_fragment.json`

A complete 12 s launch-beat design that shows every mixer feature, usable as a single-file design or as a fragment.
Its file names are placeholders: it renders once each is replaced by a real recording from the manifests (until then
the mixer lists the missing files and stops). Contents (described, values ours):

- samples: a mouse click split into a press and a release (mono, high-pass, normalised, short fades); a spacebar; a
  stick swish and a pitched-down filtered tail of it; a cork pop; a cymbal-roll riser with an `env` rising to 0 dB and
  a long fade-in; a deep drum impact and a short wood-slam top layer; a celesta note for the logo; a soft air lift with
  a `glide`.
- slices: one typing recording cut into up to 16 keys.
- pools: one per sample, plus the key slices with `#*`.
- rules: one fixed click voice (press on the cue, release +85 ms and 2 dB down); typing in `cycle` with jitter and
  a small pitch range; space; a whoosh by `peak` with cue pan scaled, plus its tail layer later and quieter; pops in a
  time window with pitch and gain ramps; a riser aligned by `end` 30 ms before the reveal; the hit and its top layer;
  the logo chime on the lockup frame.
- place: the air lift peaking just before the chime.
- ambience: a quiet room tone under the UI part with a clean window, width 0.8, EQ and a ride down at the end.
- music: one track, `stretch: none`, an intro segment and a drop segment with a long `tail_fade`.
- buses and master, marked as read from the top level only.

## 5. Role briefs: job and checks

Each brief gives the role its context (root, Python, launcher, skill, brief path, round, what changed, what is fixed),
names the only paths it may write, and requires that every number it reports comes from a command it ran. Each returns
a report the lead reads.

### 5.1 Sourcing agent

Job: find, download and measure real recordings for one category.

1. Search Freesound (CC0 and CC BY only; use the site's filter, then read each page anyway), Mixkit (lossless WAV),
   and for music the composers' own sites. Prefer recordings with many takes of one object, close-miked, dry,
   lossless or HQ, 44.1 kHz or more. 15-40 candidates, 3-6 per need.
2. Read each item's page: title, author, exact licence text, requested credit form, recording notes. Reject NC, ND,
   SA, Sampling+, custom and unclear terms at once. Never guess a licence.
3. Download politely and sequentially. Save as `audio/assets/<cat>/<source>-<id>_<slug>.<ext>`. Audio files only.
   Pixabay blocks scripts: list those items for the user to download by hand.
4. Measure every file: `analyze.py` with spectrograms, `{{HARK}} realness` on the folder. Open the spectrogram images
   of the best candidates. Reject: clipped; a floor above -55 dBFS between events; a cliff above 16 kHz (low bitrate);
   room echo on foley; voices, bumps or dogs in the part needed; realness "likely code-synthesized". An "uncertain"
   file stays only with a realness re-check on the trim the designer will use (`--start --end`) and its evidence
   written in `notes` (designed library sounds such as whooshes and chimes often read uncertain).
5. Listen through hark per need: `rank` the folder, `match` with realistic wrong readings as `--alts`, `find` the best
   takes inside long files (never use a trim marked head cut or tail cut), `describe`; `ab` for 2 candidate takes;
   `compare --refs` against approved references. Quote hark's verdict words (strong z >= 3, plausible 2.5-3, weak
   2-2.5, no match < 2). `p_vs_alts` only says which wording wins.
6. Write the manifest: every field of 4.3, with notes that tell the designer exactly what to use.

Rules: real recordings only; no rips; nothing behind a login; no design JSON; no other category. Return: per need the
ranked candidates (id, why, best segment, key numbers, realness, match z), the rejected ones with reasons, the items
the user must download, the commands run.

### 5.2 Rights auditor

Job: decide nothing about sound; make sure every recording that can reach the film is allowed, read on its own page,
credited correctly and intact. Read the rights reference first.

1. Run `rights_audit.py` (with the mix report when one exists: then only audible recordings are in scope; without it,
   all are). Create `rights.json` as `[]` when missing.
2. For each in-scope entry open `page_url` (Freesound item page; Mixkit: the licence text page plus the category page;
   music: the composer's track page). Read the licence as written, the author, the title, the requested credit form,
   any restriction (no raw redistribution, no stock use). Check the download URL belongs to the same item.
3. Write `rights_seen`, `rights_confirmed` (true only when allowed AND read on the page in this session) and
   `agent_notes`. A different or forbidden licence on the page: `rights_confirmed: false`, correct the manifest, list
   it.
4. Run the audit again (with `--fetch` once the film's recordings are final) until it exits 0 for the in-scope set.
   Then run `credits.py --rights ...`.

Rules: an aggregator is never a source; disputed or unclear = rejected; one page at a time with a pause. Return: the
summary line, each blocker and warning and what was done, manifest corrections, rejected items with a replacement
suggestion from the same manifest, credit lines that follow an author's requested form, commands run.

### 5.3 Music supervisor

Job: pick the one licensed track and cut it to the picture at bar lines so its own events (entrance, drop, stop, final
chord) land on the picture's beats.

1. Measure every candidate: `analyze.py --music --png`, `arrange.py --film-bpm {{BPM}} --arc ...`, `{{HARK}}
   describe` (mood, instruments), `{{HARK}} timeline` (events, sections, grid, loudness). Write the arc first.
2. Verify the grid by measurement: tempo residual and inlier share (steadiness), downbeat phase and its confidence
   (against the low-band hits, the chord changes in the bar table and the spectrogram), the transient shift. Record
   the verified grid as `downbeats_s` in `music_choice.json`.
3. Choose with reasons: structure fits the arc (soft opening, a drop that can land on the reveal, a resolving ending),
   mood fits the brand, little or no stretch (within about 6 %; `stretch: none` within 0.2 %), clean master, rights
   confirmed. Record the runner-up.
4. Edit: start from the draft; splice where the harmony continues (V -> I, or the same chord); the drop's downbeat on
   the hero frame; the final chord on the logo or the cut to black; `tail_fade` into the end; nudge `at` by a few ms
   onto the measured transient; a label with bars, chords and reason on every segment.
5. Verify alone (`mix.py --only music --trace ...`, `{{HARK}} timeline` of the solo) and read the QA edges section of
   the integrated mix (chopped splices, holes, segments that start inside a held note). Set `music.gain_db` so the big
   sections sit near -14 LUFS short-term before the master.

Rules: one track; never a rip; edit only at bar lines; never stretch more than about 6 %; never loop a bar so often it
becomes audible. Return: winner and runner-up with measured reasons, the verified grid, the segments with splice
reasons, the solo's timeline highlights (entrance, drop, final chord), open risks (Content ID for CC BY music),
commands run.

### 5.4 Designer

Job: turn one category's ranked recordings into `audio/design/<cat>.json`.

1. Trims: from the manifest's best take, exact trims from `{{HARK}} find`, checked with `mix.py --only <cat> --dump`.
   In `_samples.json`: the attack 3-5 ms after the start; head under -30 dB and tail under -35 dB re the peak unless
   faded; no second hit; a sensible length. Use `slices` for recordings of many hits.
2. Shape: `mono` for point sources; EQ (high-pass rumble, tame 2-6 kHz with a high shelf or a peak cut); `pitch` for
   scale (+2..+6 semitones makes a sound smaller); `norm_peak_db` to even a pool; fades long enough to never click.
3. Rules: one rule per layer per event class; `attack` for impacts, clicks, keys, pops; `peak` for whooshes; `end` for
   risers. Pools of 4 or more takes in `cycle` for repeated actions, `jitter_db` 1-2, small pitch ranges; one fixed
   voice for UI clicks. Name every rule. A `_doc` for every number that is not obvious.
4. Verify alone: solo render with trace, solo QA, hark timeline of the solo. In `qa.json`: no click at an edge, no edge
   defect, sync within limits, no machine-gun rule, every event of its classes sounded. In the timeline: an event on
   every cue, tags that read as the intended sound.
5. Levels in context: when the integrated mix exists, read its audibility rows for its events (read_db >= 3, ideally
   >= 6) and adjust rule gains, never louder than the cause deserves.

Rules: only recordings whose rights are confirmed; processing limited to trim, fade, gain, pan, EQ, resampling, gain
envelope. Return: a table need -> samples -> rule -> events, the solo QA summary, the timeline lines for key moments,
doubts, commands run.

### 5.5 Critic: sync

Question: does every sound land on its cause? Measures: QA sync (attack, peak, apex, end vs cue; deliberate offsets
apart); the picture (`{{HARK}} timeline` of the film with the cues; single frames grabbed with ffmpeg a few frames
apart around a doubtful event, opened as images, to see the real contact or peak; a cue can be wrong too); the dumped
samples' envelopes (a slow-rising sample aligned by attack peaks late; a whoosh aligned by attack lands early); every
rule offset of 30 ms or more, checked on the frames. Rules: move a cue only when frames prove it wrong, otherwise move
the sound; lead at most about 45 ms; impacts up to about 75 ms late; whoosh apex within +-45 ms. Priorities: P0 audible
on a key beat (hook, reveal, logo); P1 noticeable on a normal listen; P2 a refinement. Rank by impact per minute of
fixing. Typical fixes: `align`, `offset`, a trim closer to the attack, a take with an earlier peak. Return JSON:
`{lens: "sync", issues: [{id, priority, t, event, problem, evidence, ops, verified}], overall, score (1-10)}`.

### 5.6 Critic: edges

Question: is anything cut, clicked or chopped? Measures: QA clicks (an edge position = a cut, P0 or P1; a body position
= decide between the sound's own transient and a defect); QA edges (abrupt entries, chopped decays, craft notes, second
hits, music splices, placements past the film ends); dumps and the source around doubtful trims (`analyze.py --png`);
the master's head (first 5 ms) and tail (last 50 ms). Report click and edge counts before and after; a fix must not
create a new click elsewhere. Typical fixes: a 3-5 ms `fade_in` (never longer than the attack allows), `start` 3-5 ms
before the attack, `end` after the decay or before the next hit, a 20-80 ms `fade_out`, `stop_before_next`, another
take, another splice bar, `tail_fade`. Return JSON with lens `edges` and issues `{id, priority, t, what, problem,
evidence, ops, verified}`.

### 5.7 Critic: masking and density

Question: can every effect be heard, and is the effect layer dense or repetitive? Measures: QA audibility (read_db,
best band, covered_by, dead layers; run QA with `--layers` when it was not); QA density (events per second against
about 5 effects per 10 s, dense where the music is sparse and thin after the drop; most reused samples; per-rule
regularity; machine-gun rules); key beats first (the hook in the first 2 s, every story beat, the reveal, the logo);
`{{HARK}} timeline` of the render with the cues (an event on each key beat with the intended tags). Report read_db
before -> after for every touched event, and check that nothing else got masked (a louder effect can hide its
neighbour; a bus duck can drop the music's own downbeat). Typical fixes: rule gain; a short music duck under one key
moment; an EQ that carves the effect's best band (a boost on the effect or a cut on the bed, never both large); a
brighter or tighter take; removing dead layers; more takes; jitter or pitch for variation; dropping effects the music
already carries. Return JSON with lens `masking` and issues `{id, priority, t, event, problem, evidence, ops,
verified}`.

### 5.8 Critic: tonal

Question: is it pleasant on phones and laptops? Measures: QA harshness (events more than 6 dB over the music's loudest
2-6 kHz peak; the bright list, which is fine when it is the film's palette); band levels over time of master vs stems
(2-6k, 6-10k, 10-16k, above 16 kHz, 20 ms frames, a short script in its own folder, or stem spectrograms); the low end
(an impact's partials under 200 Hz against the music's at that moment: beating, mud; the master folds below 120 Hz to
mono, so nothing important may live in the side signal); width per section (effects near mono; only whooshes, beds and
music wide); key (tuned samples against `{{KEY}}`, f0 and note from `{{HARK}} describe` of the dumped sample, the
track key from `analyze.py --music`); the limiter (`master.json` gain reduction; when the drop is squashed, test a lower
`master.lufs` or a ride on the loudest peaks). Report harshness, band levels and audibility before -> after (a softer
top must not mask the event). Typical fixes: high-shelf or peak cuts, `norm_peak_db` down a few dB (with the cut, see
part 4, 2.2), `lp` on a bright bed, `mono` on point sources, `pitch` to the key within +-2 semitones or another note's
recording, `master.lufs` -1 for a dynamic track. Return JSON with lens `tonal` and issues `{id, priority, t, what,
problem, evidence, ops, verified}`.

### 5.9 Critic: director

Question: does the soundtrack tell the film's story with eyes closed? Is the hero reveal the loudest moment, is the
logo resolved rather than loud, does the ending subtract into silence, is anything cheap, generic, empty or random?
Measures: `{{HARK}} timeline` and `{{HARK}} sheet` (open the image); `{{HARK}} critique` (the launch-film scorecard);
QA loudness (integrated, LRA, loudest momentary and its position, per-act medians, head and tail; the short-term curve
against the picture map's beats, plotted in its own folder when that helps); a beat-by-beat walk of the picture map
(what the transcript hears, the effect and music levels, whether the sound makes the picture better or is only there);
the weakest 3 seconds and why. Tests: bus rides, the music edit, which events are sounded, the ending. Rules: at most 12
issues ranked by impact; taste is allowed but every claim needs a measurement; no synthesized sounds; no on-screen text.
Return JSON: `{lens: "director", weakest_3s: {from, to, why}, issues: [{id, priority, t, problem, evidence, ops,
verified}], overall, score}`.

### 5.10 Fixer

Job: the only writer of the design in the fix round. Merge the 5 critics' operations into one consistent change,
apply it, render, prove nothing regressed.

1. Collect every P0 and P1 with its operations. Drop issues whose `verified` numbers show no improvement.
2. Resolve conflicts and write each decision down. 2 critics on one sample, rule or ride: find the change that
   satisfies both (a hit that must be louder AND less harsh: more low body plus a high-shelf cut). Masking against
   density (louder against fewer): prefer removing a dead layer over raising a level. A ride against the director's
   arc wins only with numbers.
3. Apply all at once as a variant (`variant.py --ops out/audio-review/fix_round<N>.json --out
   out/audio-review/fix_round<N> --render --qa`). Compare its `qa.json` with the previous one section by section:
   every targeted issue better, no new fail, no new click, no new masked event, loudness on target. Iterate on the
   variant until it holds. Then `variant.py --ops ... --apply --backup out/audio-review/backup_round<N>`, `mix.py
   --trace out/qa/adds.json`, `qa.py --layers`. A line `unchanged (...)` means the operation was already in the
   design.
4. Acknowledge confirmed deliberate exceptions with `qa_ok` (one sentence each) so later rounds do not argue them again.
5. Record in each touched fragment's `_doc` and in `_buses_doc` why the numbers are what they are.

Rules: never synthesize; never change `mix.py` or `master.py`; never loosen a QA threshold to pass. Return: the merged
operations, each conflict and its resolution, the QA summary before -> after per section, rejected issues and why, open
questions for the next round, commands run.

### 5.11 Operation format (shared by critics and the fixer)

The operations of part 3, 6.2. Examples: set a sample's `norm_peak_db`; set a rule's `offset`; replace
`buses.music.auto`. A real interaction the source met: a 4 dB high-shelf cut on a plastic sample made its event louder
at 2-6 kHz (-15.8 -> -15.2 dBFS), because `norm_peak_db` renormalises after EQ. Variants catch such interactions that
nobody predicts by reading JSON.

## 6. Usage rights of downloaded sounds

A launch video is commercial use; it is an edit of each recording (trimmed, layered, synced to picture); and it is
published on platforms that scan audio. Every recording needs terms that allow all 3, read on the item's own page,
recorded in the manifest, confirmed by the audit and credited. When in doubt, leave it out: another recording exists.

### 6.1 Allowed and forbidden

| terms | status | reason | credit |
|---|---|---|---|
| CC0 1.0, Public Domain Mark | allowed | no conditions | courtesy |
| CC BY 4.0, 3.0, 2.5, 2.0 | allowed | commercial use and edits allowed with attribution | required: title, author, source link, licence link, and that it was modified |
| Mixkit Sound Effects / Stock Music Free License | allowed | free for commercial video, no attribution; read the current terms page | courtesy; never redistribute the raw files; never register the film with Content ID |
| Pixabay Content License | allowed | free for commercial use; scripted downloads are blocked, the user downloads by hand | courtesy |
| CC BY-NC (any version) | forbidden | the film is commercial | |
| CC BY-ND | forbidden | trimming and syncing are derivatives | |
| CC BY-SA | forbidden | the whole film would have to carry CC BY-SA | |
| Sampling+, Sampling | forbidden | retired, restrictive | |
| personal use, editorial, custom, "free with credit on social media", all rights reserved | forbidden | not standard terms the audit can trust | |
| 2 sources disagree (disputed) | forbidden | the strictest reading wins | |
| "royalty free" with no terms, platform rips, anything behind a login or paywall, AI-generated or code-synthesized audio | not used | unknown terms, or not a real recording | |

`rights.py` encodes this table (part 3, 2.1).

### 6.2 Verifying per site

- Freesound: the item page is `https://freesound.org/people/<user>/sounds/<id>/`; the licence is in that page's
  sidebar and is per sound, not per user. Originals need a login; the public HQ preview is
  `https://cdn.freesound.org/previews/<id // 1000>/<id>_<user_id>-hq.ogg` (Ogg Vorbis about 192 kbps; `-hq.mp3` also
  exists), where the user id is the uploader's number from the same page. Read the description: an author may ask for
  a credit form, which goes into `attribution`, or forbid raw files online, which goes into `agent_notes`.
- Mixkit: effects have no item page; record the category page that lists the item and its title there. WAV download
  `https://assets.mixkit.co/active_storage/sfx/<id>/<id>.wav`. Read the licence terms page once per session and
  quote what it allows. Never publish the raw folder; never register the film with a rights-management service.
- Music: composer libraries that release CC BY music state the licence on the track page and often a credit line, used
  verbatim. Aggregator sites are not authoritative: confirm on the artist's own page (the source met an aggregator
  that listed a track as CC BY 3.0 while the artist's page said "free with credit on social media": rejected as
  disputed). Free Music Archive, Internet Archive, SoundCloud: terms are per track and often NC or ND. Platform
  libraries tie terms to one platform: ask the user first.
- Pixabay: give the user the page link and ask them to download into the assets folder; then audit it like any file.

### 6.3 Attribution and Content ID

A complete CC BY credit names the title, the author, the source link, the licence name and link, and says the work was
modified. `credits.py` completes an author's requested form with the missing links. Some composers register CC BY
tracks with Content ID through a distributor; a correct use can still get an automatic claim. The licence still
allows the use: keep the credit in the description and dispute a claim citing the licence (many composers clear claims
on request). Prefer a composer who explains this, and tell the user before publishing. Mixkit and Pixabay forbid the
reverse: never register a film with their items.

## 7. Craft rules (`references/sound-design.md` in hark)

Numbers marked (norm) are measured norms of top-tier launch films that the source quotes from its sister project's
measurements; the rest held up on shipped films. The launch-film reference profile itself belongs to the timeline unit.

### 7.1 The bar

- Every sound has a visible cause, at a loudness that matches the cause. Not every picture event gets a sound; once a
  class of action is sounded, every instance is, at one level.
- Real and close: close-miked, dry, clean (floor under -55 dBFS between events), full bandwidth (95 % rolloff above
  12 kHz), no clipping, no room of its own, no background events.
- One palette: one material family per film (felt and wood, glass and soft plastic, metal and air), one click voice,
  one keyboard, one music track.
- The music carries the film; effects punctuate; the music's own events carry the big moments and most cuts.
- Restraint: about 5 designed effects per 10 s (norm, range 0-28), clustered where the music is sparse (cold open,
  typing, logo build), thinning after the drop.

### 7.2 Picture event to sound

| picture | sound | sits on | level and notes |
|---|---|---|---|
| cursor moves, hovers, scrolls, progress, streaming text, rolling counters | nothing | | the music carries continuous motion |
| click, tap, press | one real click voice for the film (mouse, trackpad, a tap on glass); press and release 60-110 ms apart, about 85, the release 0-3 dB quieter (norm) | the press frame; pro films land clicks 10-30 ms before the visible press | about 3 dB under the music's short-term level, 12-20 dB over it above 3 kHz (norm); bright but short (-20 dB within 6-10 ms); centred; the same level every time |
| typing | one sliced real keyboard per film; space and return deeper and longer | each sounded key on its character frame; at most about 12 keys/s; above about 25 chars/s one key per word, or silence (norm) | music dips 6-10 dB under the run; vary the take and level (sigma about 2 dB, no jump over 6 dB), not the pitch; about 75 % of on-screen typing in pro films is silent (norm) |
| a card, chip, toast, badge appears | a soft real pop or tick, pitched up for size | 50 % of its travel | a run climbs 1-3 semitones; word-like pops 2-9 dB over the music (norm) |
| toggle, switch, lock-in, snap | a real latch, switch, magnet clack | the contact frame | short, in sync within a frame |
| a counter locks | one crisp tick on the final value | the lock frame | about 8 dB under the music (norm) |
| camera-scale move, whip, zoom, logo zoom | a real swish (bamboo, stick, cloth, air past the mic) | apex on the velocity peak, onset 100-450 ms ahead (norm) | about 2 per film, never on every cut; may be wide; pans with the motion |
| hard cuts | nothing | | about 85 % of cuts in pro films have no sound of their own (norm) |
| tension into the reveal | a real riser (soft-mallet cymbal roll, orchestral swell) or the music's build | ends 30-250 ms before the hit, or stops on it | in about 40 % of films (norm); never a crash cymbal on the hit |
| hero reveal | the music's DROP on the frame; optionally one real impact (drum, taiko, kick, felt sub hit with a short top transient) | the contact or cut frame | the loudest moment, at 30-60 % of the runtime, 2-7 LU over integrated (norm) |
| logo, lockup | the music stops on a bar line or filters down; 0.2-0.7 s near-silence; ONE soft resolved element in the track's key | the frame the wordmark becomes readable, never later | 8-20 dB under the film's peak (norm); a 0.5-2.5 s tail to below -60 dBFS, then silence |
| product objects | real foley of the object's material | contacts | dry, centred |

### 7.3 Music

- One licensed track at 110-130 BPM (or half time 55-65): soft intro, groove, a breakdown or stop at 40-80 % of the
  runtime, a drop for the reveal, a resolve. Steady tempo, 4/4, mastered, full bandwidth, instrumental.
- Avoid vocals, corporate ukulele with whistling, aggressive EDM, lo-fi hiss, trailer braams on a product film, dark
  moods for a friendly product, famous overused stock tracks.
- Brand personality rows: calm or literary -> soft piano, felt, strings, no booms; fast or exact -> tight drums,
  plucks, a drop on true silence; bright or friendly -> marimba, claps, pops, major key; precise or expensive ->
  sparse piano, low strings, air; raw or mechanical -> ticks, latches, metal; warm or kind -> plucked strings, soft
  pads.
- Edit at bar lines (120 BPM in 4/4 = 2 s bars); splices continue the harmony; the drop's downbeat on the hero frame;
  stretch only when the tempo is off by more than about 0.2 %, and by at most about 6 %.
- Shape with rides, not with new music: dip under typing, lift into the drop, settle under the logo. Put each ride
  step on a splice or under a transient, never inside a held note.

### 7.4 Levels, arc, mix

- -14 LUFS integrated for web and social; true peak at most -1 dBTP after AAC (ceiling 0.77 gives that); -15/-16 only
  for a written reason.
- LRA 5-9 LU for a film with an arc (norm: 1-2 LU sounds crushed). The intro 5-12 LU under the body for its first 2-3
  s, entering on frame 0 with intent (no fade over the first bar, no click on sample 0).
- The loudest momentary on the drop, not on the logo, not in the last 15 %.
- Effects dry, mono, centred (+-0.15 pan); only whooshes, risers, ambiences and music wide. Low end under about 120 Hz
  mono.
- Every effect at least 3 dB over the bed in its best band (6 is clear); a layer that adds nothing measurable is a
  wasted placement.
- Nothing in 2-6 kHz much louder than the music's own loudest presence peaks (that band hurts first on phones). Effect
  centroids at or below 6.5 kHz (norm); pings at 9-14 kHz tire the ear.

### 7.5 Sync

- `attack` for impacts, clicks, keys, pops on the contact, press or 50 % frame; `peak` for whooshes on the velocity
  peak; `end` for risers on (or 30-250 ms before) the hit; `start` for beds and crowds.
- A sound leading by more than about 45 ms reads "sound first"; a peak more than about 75 ms late reads late (norm:
  accents land -45..+75 ms, median +8). Move the picture to the beat, not the sound off its cause.
- A slow-attack sample aligned by attack can peak 80 ms after its first transient and land late.

### 7.6 Variation without randomness

- A repeated action draws from 4 or more takes in `cycle`, +-1-2 dB jitter, at most +-0.3-0.5 semitones (real keys do
  not change pitch per key: vary the take).
- The UI click is the exception: one voice, one level.
- A progression climbs: a pitch ramp across a run, a gain ramp into a build.
- Machine-gun (one sample, no variation, regular rhythm) is the second-fastest cheap tell.

### 7.7 Tuning

- Tuned elements (chimes, bells, celesta, a motif) play in the track's key; tune by resampling within +-2 semitones;
  choose a recording near the right note rather than shifting far.
- Booms and subs on the track's root, or unpitched; an untuned boom under a resolving chord sounds cheap.

### 7.8 Film foley

- Scale through pitch and closeness: a 1 cm character's steps are fingertip taps pitched +2..+6 semitones, close and
  dry; a giant's are heavy foley slowed -3..-6 semitones with a longer tail.
- Layer by role: body (weight, 100-400 Hz), transient (the crisp contact, 2-6 kHz, short), detail (material: rattle,
  creak, rustle). One rule per layer on the same cue; check each adds something (`--layers`).
- Ambience zones: one bed per location, from clean windows of real field recordings, cut with the picture's cuts,
  ducked under key actions with `auto`.
- Materials stay consistent: the same key sounds like the same key every time.

### 7.9 Choosing a recording

- Measure: `analyze.py` and the spectrogram; hark `describe` and `match` (quote hark's verdict words); `rank`; `find`
  (ready trims); `compare --refs` and `ab`; `realness` ("real or pro-produced" required on the trim used; an
  "uncertain" needs its evidence written down).
- Prefer: lossless or HQ previews, 44.1/48 kHz or more, many takes of one object, a clean floor between takes.
- Reject: clipped, low bitrate (a cliff above 16 kHz), roomy foley, voices or bumps in the window needed, terms that
  cannot be read on the item's own page.

### 7.10 Cheap tells

Code-synthesized beeps, sine pings and noise sweeps; one sample machine-gunned; a whoosh on every cut; a boom on every
logo; sparkle sweeps standing in for "magic"; reverb on tiny sounds; random pitch on typing; partial coverage (some
presses heard, others silent); clicks at cuts; music chopped mid-note at a splice; an audible ambience loop; a
different room on every sound; a logo louder than the drop; an ending that stops mid-note instead of a tail into
silence.

## 8. Troubleshooting (`references/troubleshooting.md` in hark)

The mixer, QA and master errors are in part 4, section 6. The others:

| symptom | fix |
|---|---|
| every recording "no page reading" | the auditor has not written `rights_seen` / `rights_confirmed` yet: run the rights auditor brief |
| `rights_seen` classifies differently from the manifest | read the page again; fix the manifest `rights`, or reject the item |
| used in the film but in no manifest | add the entry (and audit it) or remove the sound |
| entry id and the Freesound page disagree | entry, page and download describe different sounds: download again from the right page |
| credits list PROBLEMS | a used recording is forbidden, unknown, unconfirmed or lacks author, title or page_url |
| Freesound preview 404 | the folder is id // 1000; the number after `_` is the uploader's user id from the page |
| Pixabay returns 403 or a challenge page | scripted downloads are blocked: ask the user to download |
| a Mixkit item has no page | record the category page and the listed title |
| a file decodes with errors | download again; prefer the lossless or HQ version |
| true peak over -1 dBTP after AAC | lower `master.ceiling` (0.77 -> 0.72) and render again; measure the MP4's decoded audio with `{{HARK}} measure film.mp4` (`master.py --measure` reads WAV and FLAC only) |
| the audio ends early | the soundtrack is shorter than the picture: set `duration` to the picture's length (the mux reports the mismatch) |
| sync looks off after the mux | the mux copies the picture and cuts the audio to it; cross-correlate the MP4 audio with the WAV (lag 0) and check the cue frames; never re-time the audio for a picture offset |
| `qa.py --listen` says hark is not importable | run QA without `--listen` and call `{{HARK}} timeline` etc. directly, or fix the venv |
| a click "inside a recording" | dump the sample and look at its spectrogram; a defect: trim around it or use another take; the sound itself: acknowledge with `qa_ok` |
| many "bright" notes | fine when the palette is bright (plastic, glass, bells); else a high-shelf cut or a darker take |
| quoting paths with spaces | quote them (`--root "/path/to/my film"`) |

## 9. Acceptance tests (workflow level)

| # | input | expected properties |
|---|---|---|
| W1 | a fresh project with the templates copied (`cues.json`, `sounddesign.json`, `design_fragment.json` as `audio/design/demo.json`) and no recordings | `mix.py` exits non-zero and lists every placeholder recording as missing, by kind and name. |
| W2 | W1 with each placeholder replaced by a generated fixture file | `mix.py` renders, the master gate passes, the report has placements for every template rule, `unsounded_events` is empty for the template cues. |
| W3 | W2 then `qa.py --layers` | exits 0 or 1; every section from 1.7 of part 4 is present; no section is `skipped` except `listen`. |
| W4 | W2 then `variant.py --ops [] --out out/audio-review/v0 --render --qa` | the variant's soundtrack is bit-identical to the base render's. |
| W5 | W2 manifests with every entry CC0 and confirmed, then `rights_audit.py --report out/qa/mix_report.json` and `credits.py --rights ...` | both exit 0; `CREDITS.md` lists every file in `files_used` exactly once. |
| W6 | each role brief in `references/` | contains no unfilled placeholder outside the documented set of section 2, names the only paths the role may write, and asks for the commands run. |
