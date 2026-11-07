# skills-and-docs, part 1: the `hark` tool skill

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`.

This part specifies what the `hark` skill (`skills/hark/SKILL.md` plus `skills/hark/references/`) teaches an agent:
when to reach for hark, and the workflows. The commands themselves belong to other units (named per command below).
This part fixes only the flags and fields the skill relies on.

## 1. Purpose and trigger

An agent cannot hear. The skill makes one rule absolute: never say how audio sounds, or that a sound fits, without a
hark reading behind the claim, and quote the number in the report (for example "strong match, z +3.9; p_real 0.96;
-14.2 LUFS").

Trigger: any moment an agent must judge, choose, trim, verify or compare sound it cannot hear:

- picking an effect, a music track or an ambience from candidates;
- finding the cleanest moment inside a recording;
- checking whether audio reads as a real recording or as code-synthesized;
- reviewing a soundtrack or a video's audio: what happens when, sync to picture, loudness, the mix;
- A/B-ing a mix change;
- making a sound in code and improving it by listening (forge);
- giving a code-made film its effects from the shipped bank (sfx, kit);
- game SFX: auditioning, varying and checking one-shots and loops (section 11).

The frontmatter description of the skill lists these triggers so the skill loads before any claim about sound.

## 2. Cost model and surfaces

hark runs locally on CPU. Audio leaves the machine only through the optional LLM critique (section 9). Every embedding
and result is cached on disk.

| surface | cost | use when |
|---|---|---|
| CLI: `hark <command> ... [--json] [--out result.json]` | each call is a new process that loads the models first: about 20-40 s on a busy laptop, even for a cached file | a handful of calls |
| MCP server: `hark mcp` (models stay loaded) | seconds per call after the first | many calls in one session |
| one Python script: `from hark import api; api.find(...); api.realness(...)` | models load once per script | batches |

CLI conventions the skill states (owned by the `core-io` unit): text output starts with a plain summary; times print
as `m:ss.ss` in text and as seconds in JSON; an error is one line on stderr, `hark <command>: error: <message>`, exit
code 1. Any file ffmpeg reads works, including the audio of an mp4 (a video without an audio track says so).

Setup, once:

- The launcher `skills/hark/scripts/hark` runs hark from the base venv `~/.local/state/hark/venv`.
- `hark doctor` checks packages, ffmpeg and models (pinned revision, cache state). `--load` also times a model load.
- The ML listening extras (CLAP and the AudioSet tagger via torch and transformers) are optional. When an ML command
  runs without them, hark asks on a TTY (y/N) and, without a TTY, exits with the exact install command (decision of
  the operator).
- Threads: on a shared machine set `HARK_THREADS=2` or `3`, or pass `--threads 2` to any command.
- The first model call of a process loads CLAP and the tagger (about 20-60 s); later calls in that process are fast.

MCP registration (owned by the `surfaces` unit): register the server for Claude Code with the launcher and
`HARK_THREADS=3`. Pass absolute paths to the tools. `full=true` returns the whole JSON. If the client shows no hark
tools, start it with `MCP_TIMEOUT=60000`. `hark mcp --warm` loads the models at startup. Tools: measure, describe,
match, rank, find, compare, ab, realness, timeline, sheet, critique, doctor, llm_critique, and the model-free bank
tools sfx_list, sfx_render, kit_render. Never write a literal key or secret into a project file.

## 3. Loop 1: choose an asset

| step | command | what to read |
|---|---|---|
| 1 | `hark rank "<description>" <folder>/ --top 5` | the shortlist. |
| 2 | `hark find <winner> "<single-event description>" --top 3` | the best moments as trims (start, end seconds). |
| 3 | `hark describe <winner> --start S --end E` | the top tags and the shape (attack, decay) of the trim. |
| 4 | `hark realness <winner> --start S --end E` (or a folder) | the verdict on the trim to be used. |
| 5 | `hark ab a b --query "<description>" [--a-start --a-end --b-start --b-end]` | which of 2 candidates is closer and how B differs. |
| opt | `hark match <file> "<claim>" --alts "<confusion>" "<confusion>"` | test one claim. |
| opt | `hark compare <file> --refs <approved>/ [--start --end]` | fit with an approved palette. |

Rules the skill teaches for this loop (commands owned by the `listen` unit):

- Find segment kinds: `stroke` (one onset until its decay) and `event` (strokes that belong together, such as a key's
  down and up) start 5 ms before the attack and end after the decay; `window` is a plain stretch whose edges find
  snapped onto the sound for one-shot queries, or a plain stretch to crossfade for beds. `--kind event` returns only
  onset trims (hits, clicks, steps), `--kind texture` only windows (beds); the default reads the query.
- `head` and `tail` give the level of the first 5 ms and the last 10 ms against the trim's peak. -30 dB or lower is
  clean. A trim marked `head cut` or `tail cut` starts or ends inside a sound: never use it as it is.
- Tagger classes are weak on one-shots under about 0.5 s. "Tagger unsure" or a near class (a zipper for a swish,
  finger snapping for a click) is neutral. Only an outright different class (speech, music, engine, vehicle) is a red
  flag.
- Judge realness on the trim that will be used. A multi-take file's p_real is a mean over every take and the silence
  between them.
- When the user wants real or professionally produced sound: never use a "likely code-synthesized" trim; read the
  evidence of an "uncertain" one before using it and write why it was kept. When synthesized sound is the point
  (procedural music, UI tones, forge sounds), realness is information, not a gate.
- Realness thresholds depend on length (each summary names them): up to 6 s, the clip thresholds lo 0.17 and hi 0.77;
  longer files, the mean over 4 s windows with weakly calibrated file thresholds lo 0.40 and hi 0.77. Compare
  verdicts, not raw p_real, across files of different lengths.
- Realness hears how a sound sounds, not where it came from: clean professional synthesized tones (UI sounds, chimes,
  synth stabs) also read synthetic. If realness is unavailable, say plainly that real-vs-synthetic was not checked;
  tagger classes such as sine wave, beep or synthesizer are weaker evidence.
- Exact digital zeros prove nothing alone: edit and fade padding is common in real recordings (the source measured
  59 % of its licensed real set; NEEDS-DATA for hark, see the index).
- A and B may be 2 trims of the same multi-take file.

## 4. Loop 2: review a mix or a film

| step | command | what to read |
|---|---|---|
| 1 | `hark timeline <film.mp4> --cues cues.json` (or `<soundtrack.wav> --video <film.mp4>`) | the listening transcript: events with time, level and tags, sections, the loudness curve, picture cuts, sync per cut and cue. `--detail low` is fast; `high` tags up to 96 events, most important first, and the summary says how many were tagged (an untagged event is not a missing sound). |
| 2 | `hark sheet <film> -o sheet.png --cues cues.json [--from 10 --to 25] [--detail low]` | an annotated image: frames, sections, labelled events, spectrogram, loudness, beats, cue markers coloured by sync (green within a frame, amber within 100 ms, red no sound). OPEN the image with the image reader: gaps, pile-ups, masking and late hits show at a glance. |
| 3 | `hark critique <film> [--fast]` | a scorecard against the measured range of 7 professional launch films: each metric ok, low or high with its weight, actions with timestamps, a verdict (launch-grade, close, needs work, not launch-grade). `--fast` skips the tagger and realness passes. |
| 4 | `hark measure <mix.wav> [--music]` | exact meters: LUFS, true peak dBTP, LRA, noise floor, attack and decay, brightness, bands. |

Rules (commands owned by the `timeline` unit):

- Sync stats: `offset_ms` = sound minus picture (negative = early). The `chance` fraction says what random edit points
  would score; only better-than-chance sync means anything.
- A cut listed with no hit can still carry a soft whoosh: check it on the sheet before adding a sound.
- Fix critical scorecard rows first, then run steps 1-3 again on the new render.
- The scorecard's "onset on visual events" share counts new attacks within +-100 ms only (the reference films'
  definition); its fixes name the cuts with no sound at all, by the same matcher as the timeline.
- When the file is one of the profile's own reference films, critique says so and scores it against the others.

## 5. Loop 3: A/B a change

- `hark ab before.wav after.wav --query "<description>"`, or `--refs <pro refs>/`: which is closer, and how B differs
  from A in measured words (louder, brighter, longer decay).
- For sounds inside longer files pass the trims. A whole-file A/B of multi-take files compares every take plus the
  silence between them.
- Then `hark timeline after.wav` and compare the event lines and sync stats with the earlier run.
- Change one thing at a time.

## 6. Loop 4: make a sound in code (forge)

Use it when the user wants a sound MADE, not chosen. Every sample is computed: no recorded audio inside, not even an
impulse response. Real recordings are only the references the critic compares with. (Engine and recipe interface:
`forge-core`, `recipes-impacts`, `recipes-motion` units.)

1. Write or pick a recipe in `hark/forge/recipes/<name>.py`. A recipe declares its parameter ranges, its defaults, its
   target description, its realistic confusions, and a render function from parameters and a random generator to a
   48 kHz array, built from the forge DSP kit (modal resonators, contact excitations, noise colours, a time-varying
   state-variable filter, a computed room, a mic with a noise floor). Model the physics of the real thing, measured on
   the references: what strikes what, what rings, how long, in what room.
2. Run one round: `hark forge <recipe> --refs <real refs> --budget 144 --reps 2 --tone 1.5 --keep 4 --robust 8 --dir
   <round dir>`, or one Python script per round that loads the models once. `--tone` adds the model-free tonal balance
   and tail to the objective. `--reps 2` ranks each candidate by 2 renders, against lucky renders. A round takes about
   3-5 minutes per 160 renders on a busy laptop CPU, including a 30-60 s model warm-up.
3. Read what it heard. Each pick's summary names what it sounds like, how far its decay sits outside the references,
   and how far its tone sits from the references' median per band (which bands fall outside every reference). Check
   the picks 4 ways: the model-free tone words (bands outside every reference, energy shares, the tail, the length:
   the most trustworthy signal once the model scores are high); `robust` (the mean over new seeds, not the lucky best
   render); `alt_zs` (which confusion is closest); spectrograms next to the references (`ffmpeg -i x.wav -lavfi
   showspectrumpic=s=900x360:scale=log:legend=0 x.png`).
4. The tagger tags in `report.json` stay held out only while nobody picks or steers by them. Fix the selection rule
   (for example the robust mean) before looking, and report the tags afterwards.
5. Change the recipe's STRUCTURE, not only its ranges, for what is wrong (a missing release tick, a metallic ring, a
   tail too long). Repeat. Keep a log of each round's change and numbers.
6. Report honestly, next to the real references scored by the same critic (`report.json` reference scores,
   leave-one-out).

Goodhart warnings the skill must state plainly:

- The search optimises hark's own models. Made sounds routinely out-score real recordings on them: the critic is
  satisfied, which does not mean the sound is better.
- p_real is easy to fool: modal resonators plus a room and a mic noise floor read real.
- Typicality rewards being an average of the references.
- The model checks saturate: after a search, every model check (held-out phrasings and independent real sets included)
  puts the made sound inside the real ones. Only measurements and ears still separate them.
- Judge by measured closeness to the references (tonal balance, bands outside every reference, tails, lengths), robust
  re-renders and spectrograms.
- Ask for a human blind test (random order, loudness-matched, hidden key) before calling a made sound as good as real.

The source reports a first test in which an independent judge found a made key press 8-13 dB off the real tonal
balance and made pops band-unlimited with long hissy tails, while every model check called them real. Steered by the
model-free check, the remade sounds moved inside the real recordings' measured spread. No human had listened yet. hark
has no such test of its own (NEEDS-DATA, see the index).

## 7. Loop 5: a film's sound kit (sfx and kit)

Use it when a code-made film (or any cue sheet) needs effects from the shipped bank instead of new synth code. No
models load. A whole kit renders in about a minute and re-renders in seconds. (Interface: `bank-kit-sfx` unit.)

1. Start from a preset: `hark kit init --preset <calm|crisp|minimal|launch> -o audio/kit.json`. calm for text-led
   films, crisp for energetic product films, minimal for designed effects-only films, launch for films cut to music.
   Each role's `note` says why its sound was chosen. calm, crisp and minimal keep riser, swell and suck off (a reveal
   resolves on one element, the lockup is one clean hit). launch sounds one riser or reverse swell at a section change
   and uses the music-led families below.
2. Fit it to the film: `hark sfx list --kind hit` (or `--category <family>`) shows candidates with lengths, peak times
   and knobs. Change a role's `sound`, its `edits` (`pitch` to the film's root for the hit or drop; `dur` and `apex`
   for a fixed move), its `takes` (12 or more for typing, 2-4 elsewhere), its `level_db`. Audition one candidate with
   `hark sfx render <ID> -o x.wav [--pitch E5] [--dur 0.8 --apex 0.32]` and read its measured onset, peak and end.
3. Render: `hark kit render audio/kit.json --out audio/kit`. The takes and `kit.lock.json` (measured onset, peak, end,
   loudness per take) are what the mixer places on cue frames: the onset on the frame for hits and touches, the peak
   on a whoosh's apex, the end on a riser's hit (its peak when the role aligns on `peak`, as the launch riser and swell
   do, because those members fall after their climax; `kit check` notes an end-aligned one).
4. Check: `hark kit check audio/kit.json --out audio/kit`. It refuses what a render cannot use (a knob or pitch the
   sound cannot take, a level over 0 dBFS, 2 takes that would share a file). Every warning names a take and a reason:
   clicks, a cut tail, comb ring, silence, a pitch that climbs more than 4 semitones, a hit with 2 attacks or a build
   before it, `pitch_off` (a note the knob did not land). Swap the sound or the knob rather than keep a warned take.
   Read the notes too (a sounded riser, a composite sting).
5. Listen through the finished film with loop 2 (`timeline --cues`, `sheet`) and check that each cue landed.

The music-led families (`hark sfx list --category <name>`): `ui_pop` (rounded mid pops for chips, bubbles and rows
arriving, the most used effect in such films), `knock` (mid-band wood and plastic), `cut_hit` (short sub hits with a
kick), `mallet` (gated glockenspiel, celesta and marimba notes, a soft pluck, chime runs; `--pitch` tunes them), `drip`
(water drops whose pitch rises; the check allows a bubble its climb), `shutter` (snap zooms, whip-cuts),
`reverse_swell` (peaks just after a cut, one per film), plus the big-whoosh thumps, riser dark / build / snatch, swish
bright / flick, fast-whoosh whip, glitch burst / mid / chirp, and notify mid-tine / mid-wood. Prefer them over bright
5 ms blips and long cinematic tails when the film is cut to music.

Levels against the music, not fixed (the launch preset, or any role with `bed_db`). In films cut to music, effects
ride 3-10 dB over the music and read by timing and spectrum; a fixed level is buried in loud passages or sticks out in
quiet ones. Per cue: render the kit (each take carries its loudness, each role its `bed_db` and `offset_ms`), measure
the bed (the music's median 50 ms level over the second before the cue), compute the take gain from the lock (never
over -1 dBFS peak), place the take (taps, knocks and clicks lead; whoosh peaks land after), and space a run of cues by
the role's minimum gap (pops 114 ms, keys 87 ms). The kit module offers these 4 helpers. Pull the music down 6-12 dB
for 0.3-0.5 s before a hit, a drop, a big whoosh or a swell and bring it back on the hit (the lock's `mix.stop_down`);
never duck the music under every effect. Without a music bed, `level_db` is the fallback; where the music is silent
before a cue the bed level is none and the gain uses `level_db`. Report the gains set and the bed levels measured.

Honesty about the bank: its sounds are made in code (the music-led families tuned toward the measured style of a
private set of licensed reference clips and checked to stay original: CLAP similarity at most 0.84 against any of
them), plus 15 sounds fitted as code from CC0 recordings (`fit/<role>/<name>`, each with its source; the presets use
them for clicks, ticks, keys, touches and the snap). All were chosen and checked by measurement, none by a listening
test. They are not recordings: when the brief asks for recorded sound, use the `sound-sourcing` skill. Adding a fit of
one's own: `hark fit --takes`, then `hark bank add-fit` (`fit` unit).

## 8. Reading the numbers

| number | what it is | how far to trust it |
|---|---|---|
| LUFS, dBTP, LRA, dBFS, ms, Hz | measurements (BS.1770 meter, spectrum, envelope) | exact; use them for levels |
| z (match, rank, find, describe tags) | how far a description stands out among the few hundred vocabulary descriptions scored on the same sound | z >= 3 strong, 2.5-3 plausible, 2-2.5 weak, < 2 no match |
| p_vs_alts | an uncalibrated, sharp forced choice between the query and its alternatives | only which wording fits best; judge by z |
| cosine | raw CLAP similarity | never a probability; compare only for one sound and one query |
| tagger labels (0..1) | independent per-class probabilities of the AudioSet tagger | good for broad classes; hears only up to 8 kHz |
| p_real, verdict | real or pro-produced vs code-synthesized, from a small fitted model | strong evidence, not proof; read its evidence; thresholds differ for clips (<= 6 s) and long files |
| digital_silence_frac | share of exact-zero samples | weak alone |
| typicality z (compare) | distance to the reference set vs the set's own spread | >= -1 fits the set, < -2 an outlier |
| sync offset_ms, chance | sound minus picture per cut or cue; what random edits would score | trust it when well above chance |
| critique status | value vs the range of 7 professional launch films (loudness: web-delivery practice; true peak <= -1 dBTP for a master, <= 0 dBTP for decoded AAC or MP3, which already contains the codec overshoot) | a range, not a target: ok = inside what pros do |

Order of trust: measurements, then verdicts, z and probabilities. Never present a cosine as confidence. (The source
keeps held-out evidence per z band in an evaluation results file; hark's `listen` unit owns the equivalent.)

## 9. Optional second opinion in words (LLM critique)

The source sends audio to a cloud audio model for a rubric critique (sync, realism, mix, loudness, what sounds cheap,
fixes with times). Per the operator's decision hark routes it through the Antigravity CLI (`agy -p`) instead, if probe
ticket 11 shows that agy hears audio. The skill keeps these rules whatever the backend:

- Run it only when the user has explicitly agreed that the audio is uploaded. Ask first. Never pass the consent flag
  (`--yes-upload`, or `consent=true` in MCP) on the user's behalf.
- Only audio is sent, as mono at 64 kbps.
- Options: attach the timeline (`--with-timeline`), ask a pointed question (`--prompt "..."`).
- Its timestamps are approximate and its scores are one model's opinion: confirm every fix with timeline and ab.

## 10. Writing queries and limits

Queries:

- Describe the sound, not its use: source + material + action + character ("a heavy wooden door slamming in a small
  room", not "a sound for the logo").
- Give alternatives that are realistic confusions (thock vs click vs typewriter).
- When a z is borderline, try 2 or 3 phrasings and trust a reading that holds under all of them.
- Job words are not heard. The source found that a job phrase ("something appearing") matched none of 9 real pops that
  their own titles matched. To check a sound for a job, describe the job's SOUND 2 or 3 ways and keep the best reading
  (on 195 real effects this raised passes from 152 to 176 and separated jobs slightly better; NEEDS-DATA for hark).

Limits to state plainly when they matter:

- Taste is not measured: hark says what a sound is and how it measures, not whether it is beautiful or right for the
  story. The user decides on taste; give the evidence and the shortlist.
- No speech recognition (it can tell there is speech, not what is said), no lyric or language judgement.
- CLAP hears 50 Hz - 14 kHz and the tagger up to 8 kHz: check air and low-pass problems with measure (bandwidth, bands).
- Very short sounds are padded to a 10 s window; isolated hits read best with find or describe on a trimmed segment.
- attack_ms and decay_ms come from smoothed envelopes (5 ms and 10 ms RMS): an instant attack reads about 4 ms; very
  short clicks cannot read below about 10-20 ms of decay.
- Picture cuts come from a frame-difference detector: hard cuts and whips are reliable; slow crossfades are missed or
  found late; motion inside a shot is not an event, so pass cue times (`--cues`) for animation.
- Dense mixed material blurs tags: describe a stem or a segment rather than the whole mix.
- Rights, provenance and attribution are not audio questions: hark cannot check them (the `sound-sourcing` skill has
  the usage-rights audit).

## 11. Game SFX (new in hark, not in the source)

The source's skill is film-only. The operator wants game SFX too. The source gives these reusable parts, and ticket 22
designs the game workflow from them (link to the `godot-sound-design` skill for engine wiring):

- Variation pools: the kit's `takes` and the bank render produce N variants per role; the variation rules of part 2,
  7.6 (4+ takes, +-1-2 dB, at most +-0.3-0.5 semitones, never the same take twice in a row) apply to repeated game
  actions (footsteps, hits, UI).
- Per-take measurement: `kit.lock.json` gives onset, peak, end and loudness per take, which a game needs for trimming
  and for even levels across variants.
- `kit check` warnings (clicks, cut tails, silence, pitch drift) are exactly the defects that break a looping or
  rapidly retriggered game sound.
- Loops (ambience beds): the equal-power loop crossfade of part 4, 2.6 and the edges check of part 4, 4.4 show how to
  judge a loop point by measurement.
- Loudness: films master to -14 LUFS; game assets usually ship normalised per asset class and are mixed live. The
  game target is an open question (index Q6).

## 12. Acceptance tests (skill level)

| # | input | expected properties |
|---|---|---|
| H1 | `skills/hark/SKILL.md` | under 12288 bytes; frontmatter `name: hark`; a description that names the triggers of section 1. |
| H2 | the skill text | names every command it teaches with flags that exist in the hark CLI (`hark <cmd> --help` lists each flag). |
| H3 | the skill text | contains the rule never to claim how audio sounds without a reading, and to quote numbers. |
| H4 | the skill text | contains the 6 Goodhart warnings of section 6 and the bank honesty paragraph of section 7. |
| H5 | the skill text | states the LLM consent rule and never shows a consent flag in an example command without the "ask first" rule beside it. |
| H6 | the skill text | contains no upstream name and no secret or key value. |
