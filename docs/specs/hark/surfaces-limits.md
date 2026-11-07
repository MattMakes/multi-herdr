# hark spec: surfaces, part L (limits, validation, integration)

Part of `docs/specs/hark/surfaces.md`. Section numbers here start with L.

Content: each limit and each validation number that the source documents, plus the contract with a film
pipeline that renders video from code. hark docs (unit skills-and-docs) restate these in new words. When hark
has not measured a number itself, hark docs label it a source measurement (Q5).

## L1. Limits

### L1.1 Reliable

| area | fact |
|---|---|
| meters | LUFS, true peak, LRA, onset times, offsets to cuts and to cue frames (ms resolution), levels, spectrum, envelope, clipping, silence and the tempo of steady music are measured values, not judgements |
| kind of sound | for 1 isolated sound (key press, whoosh, rain, glass ting), a `strong` verdict proved dependable on recordings never used for tuning; treat `plausible` and `weak` as leads to confirm |
| selection | `rank` picks well among candidate recordings; `find` locates the clean moment inside real recordings |
| plainly synthetic sources | noise risers, simple FM bells and steady test tones: a firm verdict is right about 92% of the time; the false-synthetic rate on real clips is 6.7%, the false-real rate on synthetic clips is 5.5% |
| beeps | weaker than that: when the beep family was held out of training, beep clips were missed more often than most types (accuracy 0.71) |
| sync | for hard hits against cuts and cue frames, the reference film gave a median offset of 0 ms and 96% inside 1 frame |

### L1.2 Treat as hints

| area | fact |
|---|---|
| event labels inside a dense mix | correct for about half of the exposed events and a quarter of the events under music; the times and levels are still correct |
| genre and mood | frequently wrong; for 1 upbeat track the top mood was a dark one in 18% of cases. `describe` shows them separately and marks them unvalidated. Instruments, rain, birds and voice are mostly correct |
| realness `uncertain` | typical for designed library sounds (magic chimes, celesta, motion whooshes) and real electronic music, which fall between the 2 classes. It means "designed", not "fake" |
| launch scorecard | its ranges come from 7 films chosen by 1 person, all in 1 style (software and AI launch films with kinetic type and UI demos). Other genres (a cinematic short film, a calm solo piano track) can fail rules that do not fit them. A user can build a profile from other films with `hark profile` |
| list of cuts with no hit | soft build-ups (whoosh, riser, swell) can be listed wrongly; on the reference film both listed cues did have a whoosh. Check the sheet before adding a sound |
| critique "onset on visual events" share | only a new attack within +-100 ms of an event counts, because the reference ranges used that rule; a whoosh that swells across a cut is not counted. Critique fixes name only cuts that have no sound at all. The critique counts "stop-downs" per minute (brief drops in level just ahead of a hit) and timeline "gaps" (any silence) count different things |
| realness and length | at 6 s or less the clip thresholds apply (low threshold 0.17); above 6 s, the mean of the 4 s windows is judged with file thresholds (low threshold 0.40) that are only weakly calibrated, and a file with lower p_real can end up with the milder verdict. Judge the exact trim that will be used |
| AudioSet on sounds shorter than about 0.5 s | often unsure or a neighbour class (a swish as Zipper, a click as Finger snapping). Neutral evidence; only an unrelated class such as Speech, Music or Engine is a warning sign |

### L1.3 Out of reach

| area | fact |
|---|---|
| taste | hark cannot say whether a sound is beautiful, fits the brand or feels satisfying. It can report a strong match, a 3 ms attack and a 4 ms offset to the cut, never "the film feels premium" |
| hidden synthetic layer | a code-made layer under a louder real mix usually goes unnoticed; check the stems and the source files as well as the mix |
| provenance | realness detects synthesis, not rights. A licensed synthetic UI sound can be called code-made, and code-made rain, wind or phone tones can be called real |
| content above 14 kHz | CLAP hears nothing above 14 kHz and the AudioSet tagger nothing above 8 kHz; for that range use `measure` (bandwidth, bands) for air, hiss and codec cut-offs |
| words | voice is detected; speech is not transcribed |
| licences | hark does not decide the usage rights of a file. The `sound-sourcing` skill checks manifests, and a person reads the licence on the item's page |

### L1.4 Engineering limits

| area | fact |
|---|---|
| start-up cost | every CLI run starts a fresh process that reloads the models (20 to 40 s on a loaded laptop), also for a cached file. The MCP server or a single Python process loads them once. `doctor` without `--load` loads no model |
| bf16 | only on CPUs with native bf16 (AVX512-BF16 or AMX); everywhere else fp32, because emulated bf16 measured 16 times slower. The shipped calibration was made with bf16 embeddings; fp32 matches it to cosine 0.9993 or better on 12 real clips |
| cache key | path, size, mtime and 16 sampled blocks of 4 KB. A new render with the same size and mtime that differs only outside those blocks gives a stale result; run `timeline --fresh` or update the mtime |
| test coverage | 1 library (Freesound and Mixkit files picked for 1 film), 7 launch films and 3 synthesis code bases. Orchestral sessions, field recordings from other climates and other synth engines were not tested |
| LLM critique | not checked against real answers in the source (it needs consent and an upload); in hark, not checked until the agy probe and a live test have run |
| cut detection | reacts to any large visual change, a blinking cursor included; slow crossfades go undetected |
| gallery benchmark numbers | shown only for the exact files that the benchmark measured (part G, G1.7) |
| forge self-scoring | the forge search raises hark's own model scores, and scores can rise while the sound does not improve. After a search, made sounds pass all model checks, held-out checks included, so these checks no longer separate made from real; the 1/3-octave comparison still does. A modal resonator with a room and a noise floor already fools the realness model. Judge made sounds with model-free measurements, spectrograms beside the references, re-renders on new seeds and a blind listening test |

## L2. Validation results (source measurements)

Setup of the source: 1 laptop CPU (Ryzen 7 260), bf16, 2 or 3 threads, with other work running. Data: the
licensed recordings of 1 film (219 kept, 125 rejected library files), that film's final 60 s soundtrack with a
log of every placement, a retired synthetic score, renders of 1 synth engine, separate numpy generators, and 7
professional launch films (measured locally and never committed).

### L2.1 Description matching, ranking, finding

| test | result |
|---|---|
| calibration on the tuning library (5 templates, term bias removed, z per sound) | pair AUC 0.966 against 0.951 for raw cosine; optimistic, because 67 of the 322 terms were added after its classes were known |
| held-out library (125 files, labels from file names fixed before scoring) | table M5 in part M. Strong band: 126 true and 3 wrong pairs, about 1300:1 lenient and 51:1 strict |
| recall | 0.51 of true pairs reach `strong`; 0.76 reach `weak` or better |
| uncertainty of the strong ratio | with only 3 wrong pairs the Poisson 95% range of that count is 0.6 to 8.8, so the ratio is somewhere between about 440:1 and 6000:1; the tuning library gave 170:1 |
| library search | a `strong` hit has the right kind of sound in 98% of cases and the exact variant in 61% |
| 24 practical queries on the tuning library only | a correct file in the top 3 for 23 (the miss: "a cork pop", where mouth pops ranked first), in first place for 19 |
| `find`, 17 separate key presses | 16 good trims on 16 different presses; the 17th hit is a double press that the curator had marked to skip; every trim starts 5 to 54 ms before the attack and never after it |
| `find`, a board of 32 presses | 9 of 10 |
| `find`, 44 s scene with 8 real one-shots | the right event ranks first 8 times of 8 (7 tight trims and 1 window of 5 s) |
| `find`, 300 s scene | all planted events found as tight trims; an early version stopped looking after about 95 s, fixed by gap-free windows and a 2-stage search; a re-run after the last changes gave the same numbers |
| 195 licensed real one-shots, query = own title | 168 pass (`weak` or better), 124 of them `strong` |
| the same, query = the job of the sound | phrases that describe the job (for example a pop for something appearing) passed for no real pop and no logo hit. Taking the better of 2 phrasings of the sound plus the job phrase passes 176 (152 with the job phrase alone), AUC 0.922 for the own job against other jobs (0.912). Rule for agents: describe the sound, not its purpose. A forced choice against a phrase like "a video game blip" does not test realness |
| spaced padding for short sounds | raises title matches (no match falls from 27 to 20) with the same AUC; not shipped, because the verdict bands were tuned with repeat padding |
| speed, models loaded, empty caches | model load 27 s per process; `describe` of a 0.56 s key 17 s; `match` with a new query on a cached file 2.0 s; `find` on an 18 s file 29.9 s first time, 1.2 s for a new query; `rank` 2.8 s per new file |

### L2.2 Real versus code-synthesized

Data: 1484 clean clips, 691 real or professionally produced and 793 synthesized. Grouped cross-validation: a
group is never in train and test at once.

| scheme | ROC-AUC | accuracy at threshold 0.5 |
|---|---|---|
| hold out 1 real category or 1 synth family (main figure) | 0.922 | 0.860 |
| hold out 1 synth generator type | 0.958 | 0.902 |
| real against the retired score, never trained on it | 0.948 | 0.862 |
| the same, with its whole engine also left out of training | 0.909 | 0.839 |
| zero-shot CLAP prompts, no training | 0.858 | 0.749 |

| test | result |
|---|---|
| verdict thresholds | real when p_real >= 0.77, synthesized when p_real <= 0.17; each verdict is right in about 92% of cases, on a pool that is 47% real and 53% synthetic. The thresholds were chosen on these same predictions, so 92% is in-sample for that choice |
| share with a firm verdict | 79%; the rest is `uncertain` (the README rounds this to 1 in 5) |
| error rates without base rate | 6.7% of real clips judged synthetic; 5.5% of synthetic clips judged real |
| long files | file thresholds (low 0.40) on the mean of 4 s windows, set from only 23 long synthetic files; the tool reports the weak calibration |
| professionally produced music, 1 provider held out at a time | mean p_real 0.86; 44 of 52 judged real; 0 judged synthetic |
| rule for steady tones | a 30 s 440 Hz sine: 0.48 from the model, 0.03 after the rule; the rule did not fire on any of 4741 audible clips from 345 real recordings |

### L2.3 Soundtrack listening

| test | result |
|---|---|
| detection against 147 placed effects | recall 93.2%: 96.4% for effects above the bed, 89.1% below it, 64.7% for effects more than 6 dB below the music |
| sync against cue frames (140 of 153 cues placed on the attack) | offset within 5 ms of the logged offset for 84%, within 1 frame for 96% |
| cuts-without-hit list | the weak part: both cues it listed had a whoosh |
| beat grid of a 120 BPM score | found at 120.0 BPM |
| tags | right kind for about half of the exposed events and a quarter of the buried ones; whooshes 56%, key presses 58 to 82%, grass steps under music 0% |
| run time | 81 s for the 60 s soundtrack, 93 s for a 93 s film (detail normal, 2 threads, 2 to 4 other busy processes); the README rounds to about 1.5 min for a 60 to 90 s film |

### L2.4 Launch scorecard

| test | result |
|---|---|
| each of the 7 reference films against a profile of the other 6 | 120 of 140 range checks inside (86%); min-max ranges of 6 films give about 71% without overfit. This is not a blind result, because rules were edited after the first run. `critique` recognizes when it scores 1 of the 7 and switches to leave-one-out on its own |
| reference film, real-recordings soundtrack | verdict `close`: 21 of 23 metrics, p_real 0.94; flags: a quiet first second and 1 loud passage |
| reference film, old synthetic soundtrack | verdict `needs work`: critical p_real 0.33 with the synthetic windows listed; major: it fades out instead of ending on a hit; sparse |
| planted defects | quiet, hot, mono, muffled, late start, dead air, long fade, hole, squashed, effects only, ambience only, synth bed, sine, pink noise: each was flagged (details belong to unit timeline) |
| launch film re-scored with forge v2 sounds | metrics in range 16 to 19 of 22; realness 0.73 to 0.84; cues with an onset inside 1 frame 86% to 98% of 43; -14.0 LUFS; true peak -2.2 dBTP |

### L2.5 Interfaces and the test suite

| test | result |
|---|---|
| MCP | the 13 listening tools ran through a live stdio session; test files: silent audio, click audio, a mono and a stereo file, an ogg, an mp3, the 60 s soundtrack and an mp4 film; bad input gave readable errors and no crash; every JSON part parsed |
| model residency | models stay in memory between MCP calls; the handshake takes a few seconds (not timed) |
| LLM critique | no live call in the source; every path tested with a fake HTTP layer, also that the key never prints and that uploads are removed after use |
| test suite at release | 128 passed and 14 slow tests skipped in about 65 s; the 14 slow tests pass in about 3 min |
| real-recordings mixer | rebuilds the reference soundtrack bit-identical from its design file with an empty decode cache, and its mix report byte-identical (the mixer belongs to unit skills-and-docs) |

### L2.6 Forge and fit numbers that the README quotes

These belong to units forge-core and fit. They appear in the source's quality summary. hark docs must not
claim them for hark without a new measurement.

| test | result |
|---|---|
| blind test of v1 recipes | 5 made sounds (key press, UI click, pop, fast whoosh, impact) against real ones of the same kind: the made one was found 8 times in 15, chance level; only the impact was spotted (short tail). 1 listener, 15 trials |
| v2 against v1 on the held-out benchmark | CLAP cosine between variants from 0.89-0.97 to 0.71-0.85 (real sets 0.52-0.77); click t40 from 0.56 to 1.44 x the bench median, fast whoosh from 0.51 to 0.88; tone closer in 3 of 5 (key press 8.2 to 7.4 dB, fast whoosh 8.6 to 6.2, impact 6.7 to 4.4); in 18 of 21 categories with a number the made balance is as close as a second real set or closer. No v2 blind-test score exists |
| fit, earlier engine, 12 recordings | median CLAP cosine 0.945 between recording and render (lowest 0.881); the top `describe` tag agrees for 10 of 12; in a 12-trial blind test the listener could not pick the recording (1 listener) |
| bank | 252 sounds; 236 clean at their own seed; all 72 takes of the launch preset pass the audit; the apex lands within 2.5 ms (target 5 ms) |

## L3. Film-pipeline integration

The source plugs into a code-rendered launch-film pipeline, a separate skill by the same author. hark offers
the same contract to any pipeline of that kind, and hark docs describe it without naming that skill.

### L3.1 Route A: effects from a hark kit (the default when hark is present)

Split of work: the pipeline keeps timing, the typing plan, levels, ducking, the arc, mastering, the report
and the stems. hark provides the sounds. A kit render is DSP only: no models, no torch, a few seconds per film
after the first render.

Flow: picture code -> cue file; the pipeline's score settings select the effects engine (`auto`) and name the
kit file; a bridge renders the kit into takes and a lock; the score step places the takes; the master step
writes the soundtrack.

1. Kit file. Made from a preset with `hark kit init --preset calm|crisp|minimal|launch`. Use `calm` for films
   led by text and `launch` for films cut to music. Per film the user may tune the lockup hit to the root note
   (`edits.pitch`), swap a member for a role, or use a fitted sound. Roles are the pipeline's cue kinds and
   variants (L3.2). A null role makes no sound.
2. Bridge. It looks for hark in this order: importable from the pipeline's Python; a python path in the
   score settings; the `HARK_PYTHON` variable (Q6); `hark` on PATH. It runs `hark kit render <kit> --out
   <dir>` once (nothing renders when the kit has not changed) and stores the takes inside the film project,
   so the film can be rebuilt where hark is not installed.
3. Placement rules. Each event gets 1 take (chosen by a hash per event; typing steps through the takes in
   turn). The sync point comes from the lock, by the role's `align`: `onset` puts the take's onset on the cue
   frame; `peak` puts a whoosh's loudest 5 ms window on its apex; `end` puts a riser's end on the hit, except
   for a riser or swell aligned on `peak` (the launch preset), whose climax lands on the cue and whose
   material continues after it. Level = `level_db` plus the event weight. An event whose role is null is
   skipped, and the report says so.
4. Launch-preset mix keys. `offset_ms` shifts the sync point away from the frame (`kit.place`). `bed_db` sets
   the level of the take relative to the music under the cue (`kit.bed_level` and `kit.gain_db`; it replaces
   `level_db`). `min_gap_ms` spreads a run (`kit.space_run`). The lock's `mix.stop_down` asks the music to dip
   before hits instead of ducking after them. A renderer that does not know these keys still gets usable takes
   and `level_db`. Unit bank-kit-sfx defines these keys.
5. Fallback order: render with hark; else the takes stored in the project; else the pipeline's own synth
   voices, with a prominent warning. A missing hark alone never fails the build.
6. Listening: 1 pipeline step runs `hark timeline --cues`, `hark sheet` and `hark critique` in a single
   process and writes 1 QA JSON.
7. Lockup: 1 stop-down plus 1 clean element. `hark kit check` warns when the hit role has 2 attacks or builds
   up before the hit, and when the pitch of any role rises (a rising synth whip). The pipeline itself warns
   when a second designed effect starts within 3 frames of the lockup or payoff.

### L3.2 Role vocabulary

| pipeline cue (kind, variant) | kit role | align |
|---|---|---|
| tick, tick crisp | `tick`, `tick.crisp` | onset |
| tock | `tock` | onset |
| hit | `hit` | onset |
| land, land light | `land`, `land.light` | onset |
| pop | `pop` | onset |
| whoosh (short or long move) | `whoosh.short`, `whoosh.long` | peak |
| suck, riser, swell | `suck`, `riser`, `swell`. Silent in calm, crisp and minimal. The launch preset sounds riser and swell and aligns both on peak | end |
| drop | `drop` | onset |
| key, key space, key return | `key`, `key.space`, `key.return` | onset |
| click, click confirm | `click`, `click.confirm` | onset |
| snap | `snap` | onset |
| bell (motif notes, progress steps) | `bell`, 1 take per pitch | onset |
| text fragment that resolves | `text` | onset |

### L3.3 Route B: soundtrack from real recordings (`sound-sourcing` skill)

The pipeline stays as it is. The mixer of the `sound-sourcing` skill produces the soundtrack in place of the
synthetic score.

| pipeline output | holds | role in route B |
|---|---|---|
| cue file | the format of index 1.5, with sync frames derived from the picture code | the mixer reads it as is. Mixer rules select events by `kind` (regex), `id` (regex), `variant` and a time window; `weight` feeds `weight_db`; `pan` feeds `pan: "cue"` |
| timeline constants (frame rate, BPM, total frames, act and cue tables) | the grid | total / frame rate = the design length; BPM = the arranger's film tempo; acts = per-act loudness in QA |
| score settings | key, BPM, 1 chord per bar, sections (`intro`, `build`, `groove`, `drive`, `breakdown`, `resolve`, `hold`), drops and silences, `payoff`, the signature tick, tock and motif, typing mode, mix, `master.lufs` | the musical plan: sections -> an arc for the arranger; payoff and drops -> the drop bar; key and chords -> which tuned recordings to find; master -> the design's master |
| treatment and brief | 3 brand adjectives, the personality row, the beat sheet including its sound column | concept and picture map of the sound brief |
| the pipeline's soundtrack WAV | the master that its render step muxes | replaced by the mixer output, which lives outside the bundled public folder so the premaster and stems are not bundled |
| stems folder | input of the pipeline's AV audit | the mixer writes `sfx.wav`, `amb.wav` and `music.wav`; the audit treats `sfx*.wav` as effects and `music.wav` as the music bus |

Arc derived from the score sections, 1 value per film bar in dB below the loudest sections: intro -14 up to
-10; build -10 up to -4 across its bars; groove -6; drive -2 to 0; breakdown -10; resolve -8 down to -14;
hold and tail -20. `payoff` (normally the bar of the drop) becomes the drop bar. A drop's `silence` is a
planned gap in the music edit: the segment before the drop stops that much earlier, and QA reports it as a
note, not a defect.

Replacement of each synth voice by recordings:

| cue kind | synth voice in the pipeline | recording that replaces it |
|---|---|---|
| music stems | built per chord and section | 1 licensed track cut at bar lines; the track's own drop lands on the payoff |
| tick, tock | tuned wooden mallet | a wood block or a soft mechanical switch, 2 takes a fourth apart, resampled at most 2 semitones toward the key |
| key | dark key voice per key, thinned to about 12 per s | slices of 1 keyboard recording played in cycle mode; space and return variants use a deeper take; a per-word style is a rule on variant `word` only |
| click | bright voice at 2 kHz plus a release | a mouse or trackpad press and release, as 2 rules about 0.085 s apart |
| pop | pitched blip | a cork, a mouth pop or a plastic tick, pitch rising over a run |
| snap | click plus body on the chord root | a latch, toy brick or magnet clack with a low body layer |
| land | wooden knock plus sub | foley thumps scaled by weight; light taps for variant `light` |
| hit | sub boom, clap and burst | a drum, taiko or kick hit plus a short high transient, as 2 rules |
| drop | long sub boom | the drop of the music edit, optionally with a low impact under it |
| whoosh | swept noise band | a swish of bamboo, a stick or cloth, aligned on peak |
| riser, swell | noise with a saw glide, or a reversed chord | a cymbal roll or an orchestral swell, aligned on end (or use the track's own build) |
| suck | reversed whoosh | an air-suck recording, or no sound (the mixer never reverses recordings) |
| bell (motif) | FM bell | a celesta, bell or glass note in the key, 1 sample per note |

Steps of route B, pipeline unchanged:
1. Run the picture steps as usual; the brief's spec line says the audio comes from real recordings.
2. Export the cues.
3. Copy the sound brief template into the film. Fill the picture map from the beat sheet and the cue file,
   and the personality and arc from the score settings.
4. Run the `sound-sourcing` process (sourcing, licence audit, music edit, design fragments, critique, fix),
   with paths relative to the film project.
5. Render with the mixer (`--root`, `--design`, `--cues`, `--out`, `--stems`, `--report`, `--trace`). Set the
   design's `master.lufs` to the score's `master.lufs` (-14 for launch films). Do not run the pipeline's own
   audio step.
6. Render the film with the real soundtrack, never with the option that rebuilds the audio. The pipeline's
   sync check still gates lag and true peak on the decoded MP4.
7. Review with hark (L3.4).
8. Write credits from the mix report into the delivery text.

### L3.4 hark in the review loop

| pipeline step | add | result for the reviewers |
|---|---|---|
| after the soundtrack exists | the mixer QA with layers and stems | clicks and cuts with the sample at fault, masked events, harsh peaks at 2 to 6 kHz, chopped splices, per-event sync, partial coverage, loudness and LRA |
| measure | `hark timeline <preview.mp4> --cues <sounded cues>` | a transcript of the decoded film: tagged events, sections, loudness over time, beats, offsets of sounds to cuts and cues |
| measure | `hark sheet <soundtrack.wav> -o <png> --video <preview.mp4> --cues <sounded cues>` | 1 image that the director and sound reviewers read beside the contact sheets |
| measure | `hark critique <soundtrack.wav> --video <preview.mp4>` | the scorecard against the launch profile |
| AV audit | the pipeline audit with the sounded-cues file and the stems | its own onset and sync audit on the real soundtrack; the sounded-cues file holds only events that got a sound, so planned silences do not count as misses |
| sound-sync reviewer | give it the QA JSON, transcript, sheet and scorecard | it can name the rule or sample to change; its fixes are design edits, not score parameters |
| verifier | disprove a sound issue with hark evidence (`hark match` on the placed sample, the transcript at that moment) | fewer false top-priority findings that come from reading code only |
| a synthetic score already exists | `hark realness <soundtrack.wav>` | per 4 s window, where the score sounds code-made, for briefs that ask for recorded sound |

### L3.5 Pipeline changes that the source proposes (not hark code)

1. An engine switch in the score settings (`synth` or `real`) and a design path. With `real`, the audio step
   runs the mixer with the same cue and stem options and writes outside the public folder.
2. Typing thinning at cue export (at most 12 keys per s; per word above 25 characters per s), with skipped
   keys marked `silent`, and a design made of recordings plays the identical set of keys.
3. With `real`, the AV audit reads the sounded-cues file and skips the replay of typing.
4. A section on real recordings in the pipeline's sound reference, with a palette per personality: calm (felt
   piano, soft wood), fast (tight drums, metal latches), bright (marimba, claps, cork pops), precise (sparse
   piano, glass), raw (switches, latches), warm (plucked strings, soft taps).
5. With `real`, the sound-sync reviewer proposes recordings or a new sourcing need, and never synthetic sounds.
6. The measure step of the review loop runs the mixer QA and `hark timeline`, `sheet` and `critique`.
7. A scaffold option for new films with real audio that creates the brief, design and manifest folders.
8. A single master script when the pipeline and the skill ship together (the source keeps 2 identical copies).

## L4. Acceptance tests (limits and integration)

| id | input | properties of the output |
|---|---|---|
| L-T1 | hark docs (unit skills-and-docs) | each L2 number that hark quotes carries a source-measurement label or is a hark measurement |
| L-T2 | `hark kit init --preset calm`, then `hark kit render` 2 times | the second run writes no take (cache hit); the lock names the bank version |
| L-T3 | a kit with role `riser` = null and a cue file with 1 riser event | no riser take in the takes folder; a placement that reads the lock skips the event |
| L-T4 | the cue file of index 1.5 with `fps` 30 and 1 event at `f` 90 | `hark timeline` reads a cue at 3.0 s |
| L-T5 | tool descriptions, server instructions, skills and docs | no sentence states 1 user's preference as an instruction (a phrase-list test) |
