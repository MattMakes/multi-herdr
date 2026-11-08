# skills-and-docs, part 2: the `sound-sourcing` skill: workflow, roles, templates and craft rules

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`. Scripts: part 3
(`skills-and-docs-scripts.md`) and part 4 (`skills-and-docs-mixer.md`).

## 1. Purpose and trigger

`sound-sourcing` produces the soundtrack of a code-rendered video or animation out of REAL recordings: a single
licensed music track, recorded foley and effects, and recorded field ambiences. Typical work: launch and product films,
feature demos, walkthroughs of a UI, explainer and trailer cuts, logo stings and short animations, from any renderer
(Remotion, three.js, GSAP, a captured game engine). Since the agent cannot hear, it decides everything by measurement.

Load the skill when a video needs any of: sound design, effects, foley, ambience, music or a score; UI sounds such as
clicks, whooshes, risers, impacts or a logo sting; or a music edit cut to picture. Also load it when a synthetic or
haphazard-sounding soundtrack is to be replaced by recordings, and when the user wants audio for a film sourced,
rights-checked, credited, mixed, mastered, critiqued or muxed.

A brief that wants sounds BUILT (code synthesis, the bank, a kit) belongs to the `hark` skill (part 1, loops 4 and 5).
A brief that wants recorded sound belongs here.

Central constraint: code may select, trim, place, set level, pan, equalise, change pitch by resampling, stretch the
music by small ratios, sum and master. Code may not create any tone, noise or reverb tail.

Target quality is a professional launch film. Each sound has a cause the viewer can see. The film uses one family of
materials. The music's drop lands on the hero reveal, and everything else is held back. The logo resolves, and the end
decays into silence.

## 2. Placeholders in briefs

The lead substitutes every placeholder before handing a brief to a sub-agent. No sub-agent ever sees an unfilled one.

| placeholder | value |
|---|---|
| `{{PY}}` | absolute path of the hark venv interpreter (`~/.local/state/hark/venv/bin/python`); it has numpy, scipy, soundfile, soxr, librosa, pyloudnorm and the hark package |
| `{{HARK}}` | the launcher, `skills/hark/scripts/hark` |
| `{{SKILL}}` | the folder of the `sound-sourcing` skill |
| `{{ROOT}}` | the root of the film project |
| `{{CATEGORY}}`, `{{PREFIX}}` | the category assigned to an agent, and its file-name prefix |
| `{{FILM}}`, `{{VIDEO}}`, `{{CUES}}` | film name, rendered video, cue file |
| `{{ROUND}}` | number of the critique round |
| `{{RENDER}}` | folder holding the render being reviewed (soundtrack.wav, stems) |
| `{{REPORT}}`, `{{TRACE}}`, `{{QA_JSON}}` | that render's mix report, placement trace and QA report |
| `{{CHANGED}}`, `{{FIXED}}` | changes since the previous round, to be checked most closely; issues already fixed, not to be raised again |
| `{{BAR_REFERENCE}}` | the quality target in one line, for example a premium launch film |
| `{{NEEDS}}`, `{{NEED_DESCRIPTION}}`, `{{SINGLE_EVENT_DESCRIPTION}}`, `{{EVENTS}}` | the needs, query texts and cue classes assigned to one agent |
| `{{KEY}}`, `{{BPM}}`, `{{BEATS_PER_BAR}}`, `{{BAR}}`, `{{FILM_BARS}}`, `{{DROP_T}}`, `{{DROP_BAR}}`, `{{BREAKDOWN}}`, `{{LOGO_T}}`, `{{DURATION}}` | facts about the music and the key moments of the film |
| `{{CRITIC_REPORTS}}` | the JSON returned by the critics, given to the fixer |

The source wants `python -I` on any interpreter able to import its package. hark splits the job: `{{PY}}` runs the
skill's scripts, and `{{HARK}}` runs hark commands (`{{HARK}} realness ...`). Every command below uses one of these 2.

## 3. The process

| step | owner | output | gate |
|---|---|---|---|
| 0 cue list | lead | `out/cues.json` | sync frames are computed from the picture code |
| 1 brief | lead | `audio/SOUND_BRIEF.md` | each cue names a sound need, or "nothing" |
| 2 sourcing, one agent per category, run in parallel | sourcing agents | `audio/assets/<cat>/`, `audio/assets/manifests/<cat>.json` | each file is measured, and hark realness says "real or pro-produced" for the trim in use. "uncertain" passes only if the manifest records the evidence and a check on the trim itself |
| 3 usage-rights audit | rights auditor | `audio/assets/manifests/rights.json` | `rights_audit.py` exits 0 for everything the film uses |
| 4 music edit | music supervisor | `audio/design/music.json`, `audio/assets/manifests/music_choice.json` | the grid is verified, the drop is on the reveal, the splices are clean |
| 5 design fragments, one per category, run in parallel | designers | `audio/design/<cat>.json` | the solo render and its QA are clean |
| 6 integrate | lead | soundtrack, stems, report, trace | the master gate passes (+-0.5 LU, <= -1.5 dBTP) |
| 7 QA and listening | lead | `out/qa/qa.json`, hark timeline, sheet, critique | QA has no fail |
| 8 critique, 5 lenses in parallel, read-only | critics | ranked issues, each with operations that were tested | |
| 9 fix | a single fixer | the design | QA: nothing that passed before fails now |
| 10 master, credits, mux | lead | `CREDITS.md`, the description text, the film | credits exit 0; mux lag 0 |

No 2 parallel agents write the same file. A single writer per file prevents lost edits and fixes that cancel out.

### 3.1 Roles and ownership

| role | brief (hark reference file) | writes | reads |
|---|---|---|---|
| lead | SKILL.md | the brief, `audio/sounddesign.json` (includes, buses, master), the final reports | everything |
| sourcing agent | `references/role-sourcing.md` | `audio/assets/<cat>/`, its own manifest, `out/audio-review/<cat>/` | the brief |
| rights auditor | `references/role-rights-audit.md` | the agent-owned fields of `rights.json`. If the item page contradicts a manifest's rights, rights_url, author, title, page_url or attribution, it corrects the manifest and reports the correction | manifests, item pages |
| music supervisor | `references/role-music.md` | `music_choice.json`, `audio/design/music.json`, `out/audio-review/music/` | music candidates, the picture map |
| designer | `references/role-designer.md` | `audio/design/<cat>.json`, `out/audio-review/<cat>/` | its manifest, the cues, solo renders |
| critic (one per lens) | `references/role-critic-<lens>.md` | nothing outside `out/audio-review/critic_<lens>/` | the render, the design, the trace |
| fixer | `references/role-fixer.md` | all fragments plus the top-level buses and master, in the fix round only | every critic report |

In the source these briefs are prompt templates with placeholders. Ticket 22 writes them as hark reference files in
our own words. Section 5 gives the job, steps and checks of each.

### 3.2 Step detail

0. Cue list. Each sound is tied to a frame computed by the picture code. Examples: the frame of a contact, the frame
   at which a pop-in is 50 % done, the velocity peak of a move, the hit at the top of a riser, the first frame where
   the wordmark is legible. If the renderer cannot export cues, derive the cue file (part 4, 1.1) from its timeline
   constants. Ids of the form `act:thing:n` stay stable, so a regex in a rule can match a whole class of actions.
1. Brief. Copy the template (4.2) to `audio/SOUND_BRIEF.md` and complete it: concept, picture map (each cue, what the
   screen shows, the sound need), ambience zones, permitted rights, quality bar, files. Choose the brand personality
   row (7.3). Sub-agents receive the brief unedited, because it is their contract.
2. Sourcing, one category at a time. A launch film usually has these categories: `ui` for pops, toggles, taps and
   clicks; `keyboard`; `motion` for risers and whooshes; `impacts` for the logo and the reveal hits; `ambience`; and
   `music`. A film with characters adds
   `foley`, their worlds and their objects. Aim for 15-40 candidates per category and 3-6 per need. Read the rights on
   the item's own page, download one file at a time with a pause, and measure everything:
   ```
   {{PY}} -I {{SKILL}}/scripts/analyze.py audio/assets/ui/* --png out/audio-review/ui --out out/audio-review/ui.json
   {{HARK}} realness audio/assets/ui/
   {{HARK}} rank "one dry laptop touchpad tap" audio/assets/ui/ --top 5
   {{HARK}} match <file> "a wine bottle cork popping" --alts "a cap gun" "a paper bag burst"
   {{HARK}} find <long take> "one press of a mechanical switch key" --top 8
   ```
3. Usage-rights audit. The auditor writes down what each page states, and `rights_audit.py` verifies the remainder:
   `{{PY}} -I {{SKILL}}/scripts/rights_audit.py --root . --report out/qa/mix_report.json [--fetch]`. A recording
   without confirmation never enters the design.
4. Music. Write the arc first (4.6). Then run `arrange.py <track> --film-bpm 120 --arc audio/arc.json --out
   out/audio-review/music/arrange.json`. When downbeat confidence is LOW, the script prints 2 grids. Pick one using the
   first note, where sections enter and where chords change, and rerun with `--phase N`. Then edit by hand: keep the
   harmony continuous at every splice, put the drop's downbeat on the hero frame and the last chord on the logo, and
   add a `tail_fade` at the end.
5. Design fragments. A designer writes samples (trims), slices, pools, rules on cue events, free placements and beds.
   The music supervisor writes the music block. Each verifies its fragment on its own:
   ```
   {{PY}} -I {{SKILL}}/scripts/mix.py --only ui --dump out/audio-review/ui/dump
   {{PY}} -I {{SKILL}}/scripts/mix.py --only ui --trace out/audio-review/ui/adds.json --out out/audio-review/ui/solo.wav
   {{PY}} -I {{SKILL}}/scripts/qa.py --only ui --report out/audio-review/ui/solo_report.json --stems out/audio-review/ui/solo_stems --trace out/audio-review/ui/adds.json --out out/audio-review/ui/qa.json
   {{HARK}} timeline out/audio-review/ui/solo.wav --cues out/cues.json
   ```
   Designers stay out of other fragments and out of the top level.
6. Integrate with `mix.py --root . --design audio/sounddesign.json --cues out/cues.json --trace out/qa/adds.json`.
   Clear blockers such as missing files or a failed gate before the critique starts.
7. QA and listening. Run `qa.py --layers`, then `qa.py --listen --realness --video out/film.mp4`, `{{HARK}} critique
   out/audio/soundtrack.wav --video out/film.mp4` and `{{HARK}} sheet out/audio/soundtrack.wav -o out/qa/sheet.png
   --video out/film.mp4 --cues out/qa/cues_sounded.json`.
8. Critique. 5 critics run in parallel and change nothing in the design (5.5-5.9). Each critic tries its fixes as a
   variant in its own folder (`variant.py --ops .../ops_v1.json --out .../v1 --render --qa`). It returns ranked
   issues with exact operations and before -> after numbers.
9. Fix. One fixer merges the operations and settles conflicts. It checks the merged variant against the previous
   `qa.json`, then applies it (`variant.py --apply`, with the originals backed up). A confirmed deliberate exception is
   marked `"qa_ok": "<reason>"`.
10. Master, credits, mux. The mixer masters the sum: -14 LUFS, a true-peak limiter at ceiling 0.77, a gate of
    +-0.5 LU and <= -1.5 dBTP on the WAV, which gives about -1 dBTP after AAC encoding. Then run
    `rights_audit.py --report out/qa/mix_report.json` (exit 0), `credits.py --root . --design
    audio/sounddesign.json --rights audio/assets/manifests/rights.json` and `mux.py --video out/film.mp4 --audio
    out/audio/soundtrack.wav --out out/film_sound.mp4`. Run `{{HARK}} timeline out/film_sound.mp4 --cues
    out/qa/cues_sounded.json` on the muxed file. Cross-correlate the MP4's decoded audio with the WAV and expect a
    lag of 0 samples. CC BY credits must appear in the video description, so the lead pastes
    `out/CREDITS_description.txt` there.

### 3.3 Rounds, stopping, reporting

- Run at least one critique round and one fix round. Add a second round if the first fix touched levels or the music
  edit.
- Finish when QA has no fail, each remaining warn has a `qa_ok` reason, and the director lens reports no P0.
- Any number in a report must come from a command that was actually run, and the report lists those commands.
- `out/audio-review/` (critic folders, variants, dumps) is kept. It is the evidence behind every number.
- A QA finding is something for a critic to investigate, not an instruction to change the design.

The source ran QA on its own finished reference mix. That mix was 60 s long, used 83 recordings in 264 placements, and
had already been through 5 critics and a supervisor. QA found:

- 7 clicks on the master, all within recordings (sharp transients the critics had kept);
- 5 of 153 events masked, at moments with heavy stacking;
- 3 events too loud at 2-6 kHz;
- 2 edge defects, both intended (a 25 ms choke, and a hard music cut into a quiet passage);
- no sync defect; 4 events sat off their cue by exactly a rule offset;
- 2 cue events with no sound.

A good mix should expect findings of about this size.

## 4. Templates (`skills/sound-sourcing/templates/`)

hark writes every template new. The field names are the interface, and example values are placeholders.

### 4.1 `cues.json`

Uses the cue format of part 4, 1.1. Contents: a `_doc` comment, `fps`, `bpm`, `total`, `acts`, `cue`, and a short
example event list for a 12 s demo at 60 fps and 120 BPM. The events are: a cursor click with `pan`; a run of `key`
events with a `variant` of word or space; a confirm click; a whoosh with `dur` and `apexFrac`; 3 card pops with pans
spread apart; a riser and a hit sharing one frame; a bell on the lockup.

### 4.2 `SOUND_BRIEF.md`

A Markdown brief with `{{FIELD}}` slots, plus example rows that the lead removes. Sections:

| section | content |
|---|---|
| header | the final picture (video path, duration, fps, BPM grid with the length of a beat and a bar in seconds) and the cue file. States that the soundtrack uses recorded or produced audio only: one music track edited at bar lines, recorded foley and effects on the cues, recorded ambiences wherever the picture shows a place. States that code is limited to selecting, trimming, placing, setting levels, panning, equalising, crossfading, stretching the music slightly, resampling for pitch and mastering. Names the bar reference. |
| sound concept | 4-8 bullets covering the palette (one family of close-miked sources); the character and arc of the music (quiet intro, groove, when the breakdown comes, the drop time on the hero reveal, the resolve under the logo, a tail to the end, instrumental, a tempo range); restraint (about 5 effects per 10 s); the places. |
| picture map | a table `t (s) | on screen | cue (kind / id) | sound need`. Example rows: silence under a cursor glide; the film's single click voice; a sliced keyboard while the music dips 6 dB; one recorded swish on the velocity peak; soft pops rising by 2 semitones; the music's build, or a recorded cymbal roll that ends 30 ms ahead of the hit; the DROP on the hero reveal with a low recorded impact; the music stopping on a bar line and one soft note in key on the lockup. |
| ambience zones | where each bed is heard |
| rights | a short form of the allowed and forbidden table in 6.1. A licence is never invented. |
| quality bar | Effects: recorded, close, clean; lossless or HQ previews; no clipping; between events the floor stays under -55 dBFS; the 95 % rolloff is above 12 kHz; foley has no untreated room; there are no background events, voices or handling noise. hark: realness "real or pro-produced" on every file in use, and match z >= 2.5 for the need it covers. Ambiences: clean, free of single loud events, at least 40 s usable, true stereo. Music: professionally produced, full bandwidth, instrumental, a steady 4/4 inside the tempo range. Mix: integrated -14 LUFS, LRA 5-9 LU, and true peak <= -1 dBTP once encoded to AAC. The peak moment is the drop, the logo is 8-20 dB quieter than that, and the end decays to silence. |
| files and tools | the download folder and the naming pattern `<source>-<id>_<slug>.<ext>` (for example `fs-123456_desk-typing.ogg`, `mk-2001_trackpad-tap.wav`). Audio files only: no archive containing programs, and nothing downloaded is ever executed. Also: the manifest shape, the rights audit, the design files, the review and QA folders, the analyze and hark commands, and download etiquette (one file at a time, a pause between, no more than about 40 files per category). |

### 4.3 `manifest_entry.json`

`audio/assets/manifests/<category>.json` is a JSON list with one object for each downloaded recording. Each field is
taken from the item's own page. Rights are never guessed.

| field | meaning |
|---|---|
| `id` | `<source prefix>-<item id>`: `fs-` for Freesound, `mk-` for Mixkit, another short prefix for other sites |
| `category` | the category |
| `file` | path on disk |
| `title`, `author` | as the page gives them |
| `source` | `freesound`, `mixkit`, ... |
| `page_url` | the item page (for Mixkit effects, the category page that lists the item) |
| `download_url` | the URL the file came from |
| `rights` | the licence as the page writes it, for example `CC BY 4.0` or `Mixkit Sound Effects Free License` |
| `rights_url` | URL of the licence |
| `attribution` | for CC BY: the credit form the author requests on the page. Without one: `"<title>" by <author> (<page_url>), licensed under CC BY 4.0 (<licence url>)`. `null` for CC0, Mixkit and Pixabay |
| `need` | the picture-map need it serves |
| `rank` | 1 means best for that need |
| `best_segment` | `[start, end]` in seconds of the clean section |
| `notes` | what the recording is, how it was made (per the page), what was measured, and what to use or avoid. Example: a press at 0.12 s, the release 85 ms after it and 3 dB softer, floor -78 dBFS, no room sound |
| `analysis` | the values `analyze.py` measured (duration, codec, kbps, src_rate, channels, peak_dbfs, lufs, noise_floor_dbfs, clipped_samples, attack_at, centroid_hz, rolloff95_hz, onsets) |
| `hark` | `{realness, match: {query, z, verdict}}` |

### 4.4 `rights_entry.json`

A single record of `rights.json` (part 3, 3.2). The auditor fills in `rights_seen`, `rights_confirmed` and
`agent_notes` and nothing else. The template places the script-computed fields under a comment key, so the auditor
can see what the script will add. `rights_seen` records the site, page URL, licence text as displayed with its URL,
the date it was read, the author and title given on the page, and confirmation that the preview's id and user id
agree with the download URL. `agent_notes` records requested credit forms and any restrictions. `rights_confirmed`
is set only after the page was read in the current session.

### 4.5 `sounddesign.json`

Top-level keys: `_doc`; `duration`; `cues`; `include` (the music, ambience, ui, keyboard, motion, impacts and logo
fragments); `_buses_doc`, which gives a reason for each ride (for example the music drops 6 dB under the typing,
returns for the transition, rises 1.5 dB into the drop and sits 3 dB lower under the logo); `buses` (music with an
`auto` ride, amb and sfx at 0); `_master_doc`, which explains -14 LUFS and ceiling 0.77 and allows -15 or -16 only for
a written reason, such as a dynamic orchestral track that the limiter would flatten; and `master` `{lufs: -14,
ceiling: 0.77, no_glue: true}`.

### 4.6 `arc.json`

`{_doc, film_bpm, beats_per_bar, drop_film_bar, arc}`. `arc` holds one number for each film bar: the intended loudness
in dB relative to the track's loud bars (0 for its biggest sections, -6 for a groove, -10 for a breakdown, -14 for a
quiet intro). It is derived from the picture map: quiet under the cold open and the typing, the groove for the
feature beats, a breakdown ahead of the turn, then the build, the big section from the drop on, and a resolve under the
logo. Example: a 30 s film at 120 BPM has 15 bars, with the drop on film bar 9 (16.0 s) and the logo from bar 13.

### 4.7 `design_fragment.json`

A full 12 s launch-beat design that exercises every mixer feature. It works as a single-file design or as a fragment.
Its file names are placeholders. It renders once each one is swapped for a real recording from the manifests, and
until then the mixer stops and lists the missing files. Contents, as a description (the values are ours):

- samples: a mouse click split into its press and its release (mono, high-pass, normalised, short fades); a spacebar;
  a stick swish, plus a lower-pitched and filtered copy of its tail; a cork pop; a cymbal-roll riser with an `env`
  climbing to 0 dB and a long fade-in; a deep drum impact with a short wood-slam layer on top; a celesta note for the
  logo; a soft air lift with a `glide`.
- slices: a single typing recording divided into at most 16 keys.
- pools: one for each sample, and one for the key slices using `#*`.
- rules: a single fixed click voice (the press on the cue, the release 85 ms later and 2 dB lower); typing in `cycle`
  with jitter and a narrow pitch range; the space bar; a whoosh aligned by `peak` with the cue's pan scaled, and its
  tail layer later and softer; pops in a time window with pitch and gain ramps; a riser aligned by `end` 30 ms ahead
  of the reveal; the hit with its top layer; the logo chime on the lockup frame.
- place: the air lift, peaking just ahead of the chime.
- ambience: a quiet room tone beneath the UI section, with a clean window, width 0.8, EQ and a ride down at the end.
- music: a single track with `stretch: none`, an intro segment, and a drop segment with a long `tail_fade`.
- buses and master, labelled as read only from the top level.

## 5. Role briefs: job and checks

Every brief supplies the role's context: root, Python, launcher, skill, brief path, round number, what changed and
what is already fixed. It lists the only paths the role may write. It demands that each reported number come from a
command the role ran. The role ends by returning a report to the lead.

### 5.1 Sourcing agent

Task: locate, download and measure recordings for a single category.

1. Search sites. On Freesound accept only CC0 and CC BY: filter by licence, and still read every page. On Mixkit take
   the lossless WAV. For music, use the composers' own sites. Favour recordings that hold many takes of one object,
   miked close and dry, lossless or HQ, at 44.1 kHz or above. Collect 15-40 candidates, 3-6 per need.
2. On each item page, read the title, the author, the exact licence wording, any credit form requested and the
   recording notes. NC, ND, SA, Sampling+, custom and unclear terms are rejected on sight. A licence is never assumed.
3. Download one file at a time with a pause in between. Store each as `audio/assets/<cat>/<source>-<id>_<slug>.<ext>`,
   audio files only. Pixabay refuses scripted downloads, so those items go on a list for the user to fetch by hand.
4. Measure each file with `analyze.py` (spectrograms on) and run `{{HARK}} realness` over the folder. Look at the
   spectrogram images of the strongest candidates. Reject a file when: it clips; the floor between events is above
   -55 dBFS; the spectrum drops off a cliff above 16 kHz (a sign of low bitrate); foley carries room echo; the needed
   span contains voices, bumps or dogs; or realness says "likely code-synthesized". An "uncertain" file survives only
   if realness is re-run on the designer's trim (`--start --end`) and the evidence goes into `notes`. Designed library
   sounds such as whooshes and chimes often come out uncertain.
5. Audition through hark for each need: `rank` the folder; `match` with plausible wrong readings given as `--alts`;
   `find` the best takes inside long files and skip any trim flagged head cut or tail cut; `describe`; `ab` between 2
   candidate takes; `compare --refs` against approved references. Report hark's verdict words (strong z >= 3,
   plausible 2.5-3, weak 2-2.5, no match < 2). `p_vs_alts` only tells which wording won.
6. Write the manifest with every field from 4.3. The notes must tell the designer precisely which part to use.

Limits: recordings only, no rips, nothing that needs a login, no design JSON, no other category. The report gives,
for each need, the ranked candidates (id, reason, best segment, key numbers, realness, match z); the rejects with
their reasons; the items the user has to download; and the commands run.

### 5.2 Rights auditor

Task: make no judgement on sound. Ensure that every recording that could end up in the film is permitted, has been
read on its own page, is credited correctly and is unaltered in its terms. The rights reference is read before
starting.

1. Run `rights_audit.py`. With a mix report present, only audible recordings are in scope; without one, every
   recording is. If `rights.json` does not exist, create it containing `[]`.
2. For every entry in scope, open `page_url`: the Freesound item page; for Mixkit, the licence terms page and the
   category page; for music, the composer's page for the track. Record who made it, what it is called, the exact licence
   wording, how the author wants to be credited, and any restriction (for example no redistribution of raw files, no stock use). Confirm
   that the download URL refers to that same item.
3. Fill in `rights_seen`, `rights_confirmed` and `agent_notes`. `rights_confirmed` is true only when the terms are
   allowed AND were read on the page during this session. If the page shows a different or forbidden licence, set
   `rights_confirmed: false`, correct the manifest and report the item.
4. Repeat the audit until it exits 0 for the in-scope set, adding `--fetch` once the film's recordings are settled.
   Then run `credits.py --rights ...`.

Limits: an aggregator never counts as a source. Anything disputed or unclear is rejected. Pages are opened one at a
time with a pause. The report gives the summary line; every blocker and warning with the action taken; manifest
corrections; rejected items, each with a substitute from the same manifest; credit lines in the author's requested
form; and the commands run.

### 5.3 Music supervisor

Task: select one licensed track and edit it at bar lines until the track's own landmarks (where it enters, the drop,
the stop, the closing chord) coincide with the beats of the picture.

1. Measure each candidate with `analyze.py --music --png`, `arrange.py --film-bpm {{BPM}} --arc ...`, `{{HARK}}
   describe` (mood, instruments) and `{{HARK}} timeline` (events, sections, grid, loudness). The arc is written before
   this step.
2. Confirm the grid by measurement: the tempo residual and inlier share (how steady it is), the downbeat phase and its
   confidence (cross-checked with low-band hits, the spectrogram, and where the bar table shows harmony moving),
   and the transient shift. Store the confirmed grid as `downbeats_s` in `music_choice.json`.
3. Decide with stated reasons. The structure must suit the arc (a quiet opening, a drop that can sit on the reveal, an
   ending that resolves). The mood must suit the brand. Stretch must be small (no more than about 6 %, and
   `stretch: none` when within 0.2 %). The master must be clean and the rights confirmed. Record the second choice.
4. Edit, starting from the draft. Splice at points where the harmony carries on (V -> I, or the same chord). Put the
   drop's downbeat on the hero frame and the last chord on the logo or the cut to black. End with `tail_fade`. Shift
   `at` by a few ms to sit on the measured transient. Label every segment with its bars, chords and reason.
5. Verify the music alone (`mix.py --only music --trace ...` and `{{HARK}} timeline` of the solo). Then read the QA
   edges section of the integrated mix for chopped splices, holes and segments that begin inside a held note. Set
   `music.gain_db` so the big sections reach about -14 LUFS short-term before mastering.

Limits: a single track; no rips; edits only at bar lines; stretch never beyond about 6 %; no bar repeated so often
that the loop can be heard. The report gives the winner and the second choice with measured reasons; the confirmed
grid; the segments and why each splice is where it is; the main points of the solo timeline (entrance, drop, last
chord); open risks such as Content ID on CC BY music; and the commands run.

### 5.4 Designer

Task: convert the ranked recordings of one category into `audio/design/<cat>.json`.

1. Trims. Take the manifest's best take, get exact trims from `{{HARK}} find`, and check them with `mix.py --only
   <cat> --dump`. In `_samples.json` each trim must have its attack 3-5 ms after the start; a head below -30 dB and a
   tail below -35 dB relative to the peak, unless a fade covers it; no second hit; and a reasonable length. A
   recording of many hits uses `slices`.
2. Shaping. `mono` for point sources. EQ to high-pass the rumble and calm 2-6 kHz with a high shelf or a peak cut.
   `pitch` for size: +2..+6 semitones makes a sound smaller. `norm_peak_db` to level a pool. Fades long enough that
   they can never click.
3. Rules. One rule for each layer of each event class. Align `attack` for impacts, clicks, keys and pops, `peak` for
   whooshes and `end` for risers. Repeated actions draw from pools of at least 4 takes in `cycle`, with `jitter_db` of
   1-2 and narrow pitch ranges. UI clicks use one fixed voice. Every rule has a name. Each number whose reason is not
   obvious has a `_doc`.
4. Solo verification: a solo render with a trace, solo QA, and a hark timeline of the solo. `qa.json` must show no
   click at an edge, no edge defect, sync inside the limits, no machine-gun rule, and a sound on every event of the
   fragment's classes. The timeline must show an event on each cue, with tags that match the intended sound.
5. Levels in context. Once the integrated mix exists, read the audibility rows for the fragment's events (read_db at
   least 3, preferably 6) and adjust rule gains. No sound is made louder than its cause warrants.

Limits: only recordings with confirmed rights. Processing is restricted to trim, fade, gain, pan, EQ, resampling and a
gain envelope. The report gives a table need -> samples -> rule -> events, the summary of the solo QA, the timeline
lines at key moments, any doubts, and the commands run.

### 5.5 Critic: sync

Asks whether each sound falls on its cause. Evidence:

- QA sync: attack, peak, apex and end against the cue, with deliberate offsets kept separate.
- The picture: `{{HARK}} timeline` of the film with the cues. Around a doubtful event, grab single frames with ffmpeg
  a few frames apart and open them as images to find the true contact or peak. The cue itself may be wrong.
- The envelopes of the dumped samples. Aligning by attack makes a slow-rising sample peak too late, and makes a whoosh
  arrive too soon.
- Each rule offset of 30 ms or more, checked against the frames.

Limits: a cue moves only when the frames show it is wrong. Otherwise the sound moves. A sound may lead by at most
about 45 ms; impacts may trail by up to about 75 ms; a whoosh apex stays within +-45 ms. Priority P0 is audible on a
key beat (hook, reveal, logo), P1 is noticeable on an ordinary listen, P2 is a refinement. Issues are ranked by
improvement per minute of work. Usual fixes: `align`, `offset`, a trim nearer the attack, or a take whose peak comes
earlier. The return value is JSON: `{lens: "sync", issues: [{id, priority, t, event, problem, evidence, ops,
verified}], overall, score (1-10)}`.

### 5.6 Critic: edges

Asks whether anything is truncated, clicks or is chopped. Evidence:

- QA clicks. A click at an edge position is a cut, rated P0 or P1. A click in the body needs a decision: the sound's
  own transient, or a defect.
- QA edges: abrupt entries, chopped decays, craft notes, second hits, music splices, placements beyond the film's ends.
- Dumps, and the source file around doubtful trims (`analyze.py --png`).
- The first 5 ms and the last 50 ms of the master.

The report gives click and edge counts before and after, and a fix must not cause a new click anywhere else. Usual
fixes: a 3-5 ms `fade_in` (never longer than the attack permits); `start` 3-5 ms ahead of the attack; `end` after the
decay or ahead of the next hit; a 20-80 ms `fade_out`; `stop_before_next`; a different take; a different splice bar;
`tail_fade`. The return value is JSON with lens `edges` and issues `{id, priority, t, what, problem, evidence, ops,
verified}`.

### 5.7 Critic: masking and density

Asks whether every effect can be heard, and whether the effect layer is too dense or too repetitive. Evidence:

- QA audibility: read_db, best band, covered_by, dead layers. If QA ran without `--layers`, run it again with it.
- QA density: events per second against about 5 effects per 10 s, crowding where the music is sparse, thinness after
  the drop, the most reused samples, how regular each rule is, machine-gun rules.
- Key beats before anything else: the hook within the first 2 s, each story beat, the reveal, the logo.
- `{{HARK}} timeline` of the render with the cues, which should show an event with the intended tags on each key beat.

The report gives read_db before -> after for every event it touched. It also confirms that nothing else became masked:
a louder effect can hide the next one, and a bus duck can remove the music's own downbeat. Usual fixes: rule gain; a
brief music duck under a single key moment; an EQ that clears the effect's best band (boost the effect or cut the
bed, but not both by a lot); a brighter or tighter take; deleting dead layers; adding takes; jitter or pitch for
variation; removing effects the music already provides. The return value is JSON with lens `masking` and issues
`{id, priority, t, event, problem, evidence, ops, verified}`.

### 5.8 Critic: tonal

Asks whether the mix is comfortable on phones and laptops. Evidence:

- QA harshness: events more than 6 dB above the music's loudest 2-6 kHz peak. The bright list is acceptable when it is
  the film's palette.
- Band levels over time for the master against the stems, in bands 2-6k, 6-10k, 10-16k and above 16 kHz with 20 ms
  frames, using a short script in the critic's own folder or stem spectrograms.
- The low end: partials of an impact below 200 Hz against the music's at that moment (beating, mud). The master
  folds everything under 120 Hz to mono, so nothing important may sit in the side signal there.
- Width per section: effects close to mono, and only whooshes, beds and music wide.
- Key: tuned samples against `{{KEY}}`, using f0 and note from `{{HARK}} describe` of the dumped sample and the track
  key from `analyze.py --music`.
- The limiter: gain reduction in `master.json`. If the drop is squashed, try a lower `master.lufs` or a ride on the
  loudest peaks.

The report gives harshness, band levels and audibility before -> after, because a softer top end must not leave the
event masked. Usual fixes: high-shelf or peak cuts; `norm_peak_db` a few dB lower (together with the cut; see part 4,
2.2); `lp` on a bright bed; `mono` on point sources; `pitch` into key within +-2 semitones, or a recording of another
note; `master.lufs` -1 for a dynamic track. The return value is JSON with lens `tonal` and issues `{id, priority, t,
what, problem, evidence, ops, verified}`.

### 5.9 Critic: director

Asks whether the soundtrack alone, eyes closed, tells the film's story. Is the hero reveal the loudest point? Does the
logo resolve rather than shout? Does the end thin out into silence? Does anything sound cheap, generic, empty or
random? Evidence:

- `{{HARK}} timeline`, plus `{{HARK}} sheet` with the image opened.
- `{{HARK}} critique`, the launch-film scorecard.
- QA loudness: integrated, LRA, the loudest momentary value and where it falls, medians per act, head and tail. The
  short-term curve is compared against the beats of the picture map, and plotted in the critic's folder if that
  helps.
- A walk through the picture map one beat at a time: what the transcript hears, the effect and music levels, and
  whether each sound improves the picture or merely occupies it.
- The weakest 3 seconds, with the reason.

It may test bus rides, the music edit, the choice of sounded events and the ending. Limits: no more than 12 issues,
ranked by impact. Taste is allowed, but each claim needs a measurement. No synthesized sounds. No on-screen text. The
return value is JSON: `{lens: "director", weakest_3s: {from, to, why}, issues: [{id, priority, t, problem, evidence,
ops, verified}], overall, score}`.

### 5.10 Fixer

Task: act as the sole writer of the design during the fix round. Combine the operations of the 5 critics into one
coherent change, apply it, render, and show that nothing got worse.

1. Gather every P0 and P1 with its operations. Discard issues whose `verified` numbers show no gain.
2. Settle each conflict and record the decision. When 2 critics touch the same sample, rule or ride, look for one
   change that meets both needs. Example: a hit that must be louder AND less harsh gets more low body plus a
   high-shelf cut. When masking and density pull opposite ways (louder versus fewer), deleting a dead layer is better
   than raising a level. A ride that contradicts the director's arc wins only with numbers behind it.
3. Apply everything together as one variant (`variant.py --ops out/audio-review/fix_round<N>.json --out
   out/audio-review/fix_round<N> --render --qa`). Compare its `qa.json` with the earlier one, section by section.
   Each targeted issue must improve, with no new fail, no new click, no new masked event, and loudness on target.
   Rework the variant until all of that holds. Then run `variant.py --ops ... --apply --backup
   out/audio-review/backup_round<N>`, `mix.py --trace out/qa/adds.json` and `qa.py --layers`. A line reading
   `unchanged (...)` means the design already contained that operation.
4. Mark each confirmed deliberate exception with `qa_ok` and one sentence, so that later rounds do not reopen it.
5. Explain the numbers in the `_doc` of every fragment it changed and in `_buses_doc`.

Limits: no synthesis; no edits to `mix.py` or `master.py`; no loosening of a QA threshold to get a pass. The report
gives the merged operations, every conflict and how it was settled, the QA summary before -> after for each section,
the rejected issues with reasons, open questions for the next round, and the commands run.

### 5.11 Operation format (shared by critics and the fixer)

The operations are those of part 3, 6.2: for example setting a sample's `norm_peak_db`, setting a rule's `offset`, or
replacing `buses.music.auto`. The source ran into this interaction: a 4 dB high-shelf cut on a plastic sample raised
its event's 2-6 kHz level (-15.8 -> -15.2 dBFS), since `norm_peak_db` normalises again after the EQ. Rendering a
variant exposes interactions like this, which reading the JSON does not reveal.

## 6. Usage rights of downloaded sounds

A launch video is a commercial use. It modifies each recording (trims it, layers it, syncs it to picture). It is
published on platforms that scan audio. Each recording therefore needs terms that permit all 3. Those terms are read
on the item's own page, written into the manifest, confirmed by the audit and credited. If there is any doubt, the
recording is dropped, since a substitute can always be found.

### 6.1 Allowed and forbidden

| terms | status | reason | credit |
|---|---|---|---|
| CC0 1.0, Public Domain Mark | allowed | no conditions attached | courtesy |
| CC BY 4.0, 3.0, 2.5, 2.0 | allowed | attribution is the only condition; selling and editing are both fine | required: title, author, source link, licence link, and a note that it was modified |
| Mixkit Sound Effects / Stock Music Free License | allowed | commercial video use is free with no attribution; check the current terms page | courtesy; raw files are never redistributed; the film is never registered with Content ID |
| Pixabay Content License | allowed | commercial use is free; scripts are blocked, so the user downloads by hand | courtesy |
| CC BY-NC (any version) | forbidden | the film is commercial | |
| CC BY-ND | forbidden | trimming and syncing create derivatives | |
| CC BY-SA | forbidden | the entire film would need CC BY-SA | |
| Sampling+, Sampling | forbidden | retired and restrictive | |
| all rights reserved, personal use, editorial, custom terms, "free with credit on social media" | forbidden | non-standard terms the audit cannot trust | |
| 2 sources disagree (disputed) | forbidden | the strictest reading applies | |
| "royalty free" without terms, platform rips, anything behind a login or paywall, AI-generated or code-synthesized audio | not used | the terms are unknown, or it is not a recording | |

`rights.py` implements this table (part 3, 2.1).

### 6.2 Verifying per site

- Freesound. The item page is `https://freesound.org/people/<user>/sounds/<id>/`. The licence appears in that page's
  sidebar and applies per sound, not per user. Original files need a login. The public HQ preview is at
  `https://cdn.freesound.org/previews/<id // 1000>/<id>_<user_id>-hq.ogg` (Ogg Vorbis at about 192 kbps; a `-hq.mp3`
  exists too). The user id is the uploader's number, shown on the same page. Read the description: an author's
  requested credit form goes into `attribution`, and a ban on posting raw files goes into `agent_notes`.
- Mixkit. Effects have no page of their own, so record the category page that lists the item and the title shown
  there. The WAV is at `https://assets.mixkit.co/active_storage/sfx/<id>/<id>.wav`. Read the licence terms page once
  per session and quote what it permits. Never publish the raw folder, and never register the film with a
  rights-management service.
- Music. Composer libraries that release under CC BY give the licence on the track page, often with a credit line to
  be used unchanged. Aggregators do not count; confirm on the artist's own page. In one case the source met, an
  aggregator showed CC BY 3.0 for a track whose own artist page asked only for a social-media credit, which made the
  terms disputed, and the track was rejected. On Free Music Archive, Internet Archive and SoundCloud, terms vary per track and are
  often NC or ND. Platform libraries bind their terms to one platform, so ask the user first.
- Pixabay. Give the user the page link and ask them to save the file into the assets folder. It is then audited like
  any other file.

### 6.3 Attribution and Content ID

A full CC BY credit gives the title, author, source link, licence name and licence link, and states that the work was
modified. `credits.py` fills in the missing links around an author's requested form. Some composers register CC BY
tracks with Content ID through a distributor, so a lawful use can still trigger an automatic claim. The use remains
permitted: keep the credit in the description and dispute the claim by citing the licence. Many composers release
claims when asked. Favour composers who document this, and warn the user before publishing. Mixkit and Pixabay
prohibit the opposite: a film using their items must never be registered.

## 7. Craft rules (`references/sound-design.md` in hark)

A value tagged (norm) is a measured norm of top-tier launch films that the source cites from measurements made in a
related project. The others are rules that worked on films the source shipped. The launch-film reference profile
itself is owned by the timeline unit.

### 7.1 The bar

- A sound needs a cause on screen, and its loudness matches that cause. Not every picture event is sounded, but once
  one class of action is sounded, all of its instances are, at a single level.
- Recorded and close: close-miked, dry, clean (floor below -55 dBFS between events), full bandwidth (95 % rolloff
  above 12 kHz), free of clipping, without a room of its own and without background events.
- A single palette per film: one family of materials (felt and wood, glass and soft plastic, metal and air), one click
  voice, one keyboard, one music track.
- The music drives the film and the effects punctuate it. Big moments and most cuts are carried by events in the music.
- Hold back: about 5 designed effects per 10 s (norm, range 0-28), grouped where the music is sparse (cold open,
  typing, the logo build) and fewer after the drop.

### 7.2 Picture event to sound

| picture | sound | sits on | level and notes |
|---|---|---|---|
| cursor moves, hovers, scrolls, progress bars, streaming text, rolling counters | none | | continuous motion is left to the music |
| click, tap, press | a single recorded click voice for the whole film (mouse, trackpad, a tap on glass); press and release 60-110 ms apart, typically 85, the release 0-3 dB softer (norm) | the press frame; professional films put the click 10-30 ms ahead of the visible press | roughly 3 dB under the music's short-term level but 12-20 dB above it over 3 kHz (norm); bright yet short (down 20 dB within 6-10 ms); centred; identical level each time |
| typing | one sliced recorded keyboard per film; space and return lower and longer | each sounded key on the frame its character appears; no more than about 12 keys/s; above about 25 chars/s, one key per word or silence (norm) | the music dips 6-10 dB under the run; vary the take and the level (sigma about 2 dB, never a jump above 6 dB) but not the pitch; in professional films about 75 % of on-screen typing has no sound (norm) |
| a card, chip, toast or badge appears | a soft recorded pop or tick, pitched up to suit its size | 50 % of its travel | a run rises 1-3 semitones; pops that read like words sit 2-9 dB above the music (norm) |
| toggle, switch, lock-in, snap | a recorded latch, switch or magnet clack | the contact frame | short, synced within one frame |
| a counter reaches its value | a single crisp tick on the final number | the frame it locks | about 8 dB below the music (norm) |
| a camera-scale move, whip, zoom or logo zoom | a recorded swish (bamboo, stick, cloth, air rushing past the mic) | apex on the velocity peak, onset 100-450 ms earlier (norm) | about 2 per film and never on every cut; may be wide; pans with the motion |
| hard cuts | none | | in professional films about 85 % of cuts carry no sound of their own (norm) |
| tension before the reveal | a recorded riser (soft-mallet cymbal roll, orchestral swell) or the music's own build | finishes 30-250 ms ahead of the hit, or stops on it | used by roughly 40 % of films (norm); a crash cymbal on the hit is never acceptable |
| hero reveal | the music's DROP on that frame, optionally with one recorded impact (drum, taiko, kick, a felt sub hit with a short top transient) | the contact or cut frame | the loudest point of the film, at 30-60 % of its length and 2-7 LU above integrated (norm) |
| logo, lockup | the music stops on a bar line or is filtered down; 0.2-0.7 s of near silence; ONE soft element that resolves in the track's key | the first frame where the wordmark is legible, never after | sits 8-20 dB under the loudest point of the film (norm); its tail lasts 0.5-2.5 s and falls below -60 dBFS before silence |
| product objects | recorded foley of the object's material | the contacts | dry and centred |

### 7.3 Music

- A single licensed track at 110-130 BPM, or half time at 55-65. Shape: quiet intro, groove, a breakdown or stop at
  40-80 % of the length, a drop for the reveal, then a resolve. A steady tempo in 4/4, mastered, full bandwidth, no
  vocals.
- Steer clear of vocals, corporate ukulele and whistling, aggressive EDM, lo-fi hiss, trailer braams on a product
  film, dark moods for a friendly product, and famous stock tracks heard everywhere.
- Brand personality rows: calm or literary -> soft piano, felt, strings, no booms. Fast or exact -> tight drums,
  plucks, a drop out of real silence. Bright or friendly -> marimba, claps, pops, a major key. Precise or expensive ->
  sparse piano, low strings, air. Raw or mechanical -> ticks, latches, metal. Warm or kind -> plucked strings, soft
  pads.
- Cut at bar lines (at 120 BPM in 4/4 a bar is 2 s). Splices keep the harmony going. The drop's downbeat sits on the
  hero frame. Stretch only if the tempo is more than about 0.2 % off, and never by more than about 6 %.
- Shape the track with rides rather than extra music: down under typing, up into the drop, settled under the logo.
  Every ride step goes on a splice or under a transient, never in the middle of a held note.

### 7.4 Levels, arc, mix

- -14 LUFS integrated for web and social, with true peak no higher than -1 dBTP after AAC (ceiling 0.77 achieves it).
  -15 or -16 only for a written reason.
- LRA 5-9 LU for a film that has an arc (norm; 1-2 LU sounds crushed). For its first 2-3 s the intro sits 5-12 LU
  under the body. It starts on frame 0 with purpose: no fade across the first bar and no click on sample 0.
- The loudest momentary value falls on the drop. It is not on the logo and not in the final 15 %.
- Keep effects dry, mono and within +-0.15 of centre. Width is reserved for music, ambiences, risers and whooshes. Everything
  under about 120 Hz is mono.
- Each effect stands at least 3 dB above the bed in its best band, and 6 dB is clear. A layer with no measurable
  contribution wastes a placement.
- Nothing in 2-6 kHz goes much above the music's own loudest presence peaks, because phones make that band painful
  first. The spectral centroid of an effect should not pass 6.5 kHz (norm), since high pings between 9 and 14 kHz are fatiguing.

### 7.5 Sync

- Align `attack` for impacts, clicks, keys and pops on the contact, press or 50 % frame. Align `peak` for whooshes on
  the velocity peak. Align `end` for risers on the hit, or 30-250 ms before it. Align `start` for beds and crowds.
- A sound more than about 45 ms early is heard as arriving before its cause. A peak more than about 75 ms late is heard
  as late (norm: accents land between -45 and +75 ms, median +8). Adjust the picture to the beat rather than pull the
  sound away from its cause.
- A sample with a slow attack, aligned by attack, can peak 80 ms after its first transient and so arrive late.

### 7.6 Variation without randomness

- A repeated action picks from at least 4 takes in `cycle`, with +-1-2 dB jitter and no more than +-0.3-0.5 semitones.
  Real keys keep their pitch from press to press, so vary the take instead.
- The UI click is the one exception: a single voice at a single level.
- A progression rises: a pitch ramp over a run, a gain ramp into a build.
- Machine-gunning (one sample, unvaried, in a regular rhythm) is among the quickest ways a soundtrack sounds cheap.

### 7.7 Tuning

- Anything with a pitch (a motif, celesta, bells, chimes) must match the key of the track. Tune by resampling within +-2 semitones,
  and prefer a recording close to the right note over a large shift.
- Booms and subs sit on the track's root or carry no pitch. An untuned boom under a resolving chord sounds cheap.

### 7.8 Film foley

- Size comes from pitch and distance. Steps of a 1 cm character are fingertip taps raised +2..+6 semitones, close and
  dry. Steps of a giant are heavy foley lowered -3..-6 semitones with a longer tail.
- Build layers by function. The body layer gives weight at 100-400 Hz. The transient layer is the brief, crisp contact
  in 2-6 kHz. The detail layer tells the material: a rattle, a creak, a rustle. Use one rule per layer on the same cue, and confirm with `--layers` that each one
  contributes.
- Ambience zones: a bed for each location, cut from clean windows of recorded field audio, changing with the picture's
  cuts and ducked under key actions with `auto`.
- Materials stay constant: a given key sounds like that same key every time.

### 7.9 Choosing a recording

- Measure with `analyze.py` and the spectrogram; hark `describe` and `match` (cite hark's verdict words); `rank`;
  `find` (gives ready trims); `compare --refs` and `ab`; and `realness`, which must say "real or pro-produced" on the
  trim in use (an "uncertain" result needs its evidence recorded).
- Favour lossless or HQ previews at 44.1/48 kHz or higher, many takes of a single object, and a clean floor between
  takes.
- Reject clipping, low bitrate (a cliff above 16 kHz), foley with room sound, voices or bumps in the needed window,
  and terms that cannot be read on the item's own page.

### 7.10 Cheap tells

Things that make a soundtrack sound cheap:

- beeps, sine pings and noise sweeps made in code;
- one sample machine-gunned;
- a whoosh on each cut, or a boom on each logo;
- sparkle sweeps used to mean "magic";
- reverb on small sounds;
- random pitch on typing;
- partial coverage, where some presses are heard and others are not;
- clicks at cuts;
- music cut in the middle of a note at a splice;
- an ambience loop the listener can hear;
- a different room on each sound;
- a logo louder than the drop;
- an ending that halts mid-note rather than decaying into silence.

## 8. Troubleshooting (`references/troubleshooting.md` in hark)

Part 4, section 6 covers mixer, QA and master errors. The rest:

| symptom | fix |
|---|---|
| every recording shows "no page reading" | `rights_seen` / `rights_confirmed` are still empty; run the rights auditor brief |
| `rights_seen` classifies differently from the manifest | reread the page, then correct the manifest's `rights` or reject the item |
| used in the film but missing from every manifest | add an entry and audit it, or remove the sound |
| the entry id and the Freesound page do not agree | the entry, the page and the download refer to different sounds; download again from the correct page |
| credits list PROBLEMS | a recording in use is forbidden, unknown or unconfirmed, or has no author, title or page_url |
| Freesound preview returns 404 | the preview folder is the sound id divided by 1000 (integer); after the `_` comes the uploader's numeric id, read from the item page |
| Pixabay answers 403 or with a challenge page | it blocks scripted downloads; ask the user to download |
| a Mixkit item has no page | record the category page and the title listed there |
| a file decodes with errors | download it again, preferring the lossless or HQ version |
| true peak above -1 dBTP after AAC | lower `master.ceiling` from 0.77 to 0.72 and render again; measure the MP4's decoded audio with `{{HARK}} measure film.mp4`, since `master.py --measure` accepts WAV and FLAC only |
| the audio stops before the picture | the soundtrack is shorter than the video; set `duration` to the picture's length (the mux reports the difference) |
| sync looks wrong after muxing | the mux copies the picture and trims the audio to it; check the cue frames and cross-correlate the MP4's audio with the WAV (expect lag 0); a picture offset is never fixed by shifting the audio |
| `qa.py --listen` reports that hark cannot be imported | run QA without `--listen` and call `{{HARK}} timeline` and the rest directly, or repair the venv |
| a click "inside a recording" | dump the sample and view its spectrogram; if it is a defect, trim around it or switch takes; if it belongs to the sound, acknowledge it with `qa_ok` |
| many "bright" notes | acceptable when the palette is bright (plastic, glass, bells); otherwise a high-shelf cut or a darker take |
| paths that contain spaces | quote them (`--root "/path/to/my film"`) |

## 9. Acceptance tests (workflow level)

| # | input | expected properties |
|---|---|---|
| W1 | a new project with the templates copied in (`cues.json`, `sounddesign.json`, and `design_fragment.json` as `audio/design/demo.json`) and no recordings | `mix.py` exits non-zero and reports each placeholder recording as missing, by kind and name |
| W2 | W1 with every placeholder swapped for a generated fixture file | `mix.py` renders; the master gate passes; the report contains placements for every template rule; `unsounded_events` is empty for the template cues |
| W3 | W2, then `qa.py --layers` | exits 0 or 1; every section listed in part 4, 1.7 is present; only `listen` may be `skipped` |
| W4 | W2, then `variant.py --ops [] --out out/audio-review/v0 --render --qa` | the variant soundtrack is bit-identical to the base render |
| W5 | W2 manifests where every entry is CC0 and confirmed, then `rights_audit.py --report out/qa/mix_report.json` and `credits.py --rights ...` | both exit 0; `CREDITS.md` names each file of `files_used` exactly once |
| W6 | every role brief in `references/` | no unfilled placeholder outside the set in section 2; names the only paths the role may write; asks for the commands that were run |
