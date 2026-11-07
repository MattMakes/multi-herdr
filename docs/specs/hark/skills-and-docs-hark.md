# skills-and-docs, part 1: the `hark` tool skill

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`.

Scope: the content of `skills/hark/SKILL.md` and `skills/hark/references/`. The skill tells an agent when hark is the
right tool and which command sequence to follow. Other units own the commands (each loop names its unit). This part
pins down only the flags and output fields the skill text depends on.

## 1. Purpose and trigger

The agent has no ears. Its central rule: a statement about how audio sounds, or about whether a sound suits its
purpose, needs a hark measurement behind it, and the report cites that measurement. A good citation looks like
"z +3.4 (strong); p_real 0.91; -13.8 LUFS integrated".

The skill applies whenever the agent has to judge, select, trim, check or compare audio. Cases:

- selecting one effect, music track or ambience out of several;
- locating the cleanest span inside a longer recording;
- deciding whether a file is a real recording or was synthesized by code;
- auditing a soundtrack or the audio of a video: event timing, picture sync, loudness, balance;
- comparing 2 versions of a mix;
- building a sound in code and refining it against measurements (forge);
- dressing a code-made film with effects from the bundled bank (sfx, kit);
- game SFX work on one-shots and loops: audition, variation, checks (section 11).

The `description` in the frontmatter names these cases. Its job is to make the skill load before the agent says
anything about sound.

## 2. Cost model and surfaces

Everything runs on the local CPU. The one path that sends audio off the machine is the optional LLM critique
(section 9). hark caches embeddings and results on disk.

| surface | cost | use when |
|---|---|---|
| CLI: `hark <command> ... [--json] [--out result.json]` | a fresh process per call, and model loading comes first: roughly 20-40 s on a loaded laptop, cached file or not | only a few calls are needed |
| MCP server: `hark mcp` (models stay loaded) | after the first call, a few seconds each | a session makes many calls |
| one Python script: `from hark import api; api.find(...); api.realness(...)` | one model load for the whole script | batch work |

CLI conventions the skill repeats (the `core-io` unit owns them): text output opens with a plain-language summary.
Times are `m:ss.ss` in text and seconds in JSON. Errors are a single stderr line, `hark <command>: error: <message>`,
with exit code 1. Input is anything ffmpeg can decode, including the audio track of an mp4. A video with no audio track
produces a message saying so.

One-time setup:

- `skills/hark/scripts/hark` is the launcher. It starts hark with the base venv at `~/.local/state/hark/venv`.
- `hark doctor` reports on packages, ffmpeg and the models (pinned revision, cached or not). With `--load` it also
  times one model load.
- CLAP and the AudioSet tagger (through torch and transformers) form an optional ML extra. If an ML command finds the
  extra missing, hark prompts y/N on a TTY. With no TTY it exits and prints the install command to run. The operator
  chose this behaviour.
- On a machine shared with other work, limit threads with `HARK_THREADS=2` or `3`, or `--threads 2` on any command.
- A process pays the CLAP plus tagger load (roughly 20-60 s) on its first model call only.

MCP registration (the `surfaces` unit owns it): Claude Code registers the server through the launcher with
`HARK_THREADS=3`. Tool arguments take absolute paths. `full=true` makes a tool return its complete JSON. When a client
lists no hark tools, restart it with `MCP_TIMEOUT=60000`. `hark mcp --warm` loads the models when the server starts.
Tool list: measure, describe, match, rank, find, compare, ab, realness, timeline, sheet, critique, doctor,
llm_critique, plus 3 bank tools that need no models: sfx_list, sfx_render, kit_render. Project files never hold a
literal key or secret.

## 3. Loop 1: choose an asset

| step | command | output to read |
|---|---|---|
| 1 | `hark rank "<description>" <folder>/ --top 5` | a shortlist |
| 2 | `hark find <winner> "<single-event description>" --top 3` | the top spans as trims (start and end in seconds) |
| 3 | `hark describe <winner> --start S --end E` | the strongest tags, plus the attack and decay of the span |
| 4 | `hark realness <winner> --start S --end E` (or a folder) | the real-or-synthetic verdict for the exact span to be used |
| 5 | `hark ab a b --query "<description>" [--a-start --a-end --b-start --b-end]` | the closer of 2 candidates, and the measured ways B differs |
| opt | `hark match <file> "<claim>" --alts "<confusion>" "<confusion>"` | a test of a single claim |
| opt | `hark compare <file> --refs <approved>/ [--start --end]` | how well the file sits with an approved palette |

Guidance for this loop (the `listen` unit owns the commands):

- find returns 3 segment kinds. A `stroke` runs from one onset through its decay. An `event` groups strokes that
  belong to one action, such as the press and release of a key. Both begin 5 ms ahead of the attack and close after
  the decay. A `window` is a plain span: for one-shot queries its edges are snapped to the sound, and for beds it is
  meant to be crossfaded. `--kind event` limits results to onset trims (hits, clicks, steps). `--kind texture` limits
  them to windows (beds). Without the flag, find infers the kind from the query.
- `head` is the level of the first 5 ms and `tail` the level of the last 10 ms, both relative to the trim's peak. At or
  below -30 dB the edge is clean. A `head cut` or `tail cut` flag means the trim begins or ends in the middle of a
  sound and must not be used unchanged.
- On one-shots shorter than roughly 0.5 s the tagger is unreliable. "Tagger unsure", or a neighbouring class (zipper
  for a swish, finger snapping for a click), counts as neither for nor against. A clearly unrelated class (speech,
  music, engine, vehicle) is a warning sign.
- Run realness on the span that will ship. For a file with many takes, p_real averages all takes and the gaps between
  them.
- If the user asked for real or professionally produced audio, a "likely code-synthesized" trim is out. An
  "uncertain" trim is usable only after reading its evidence, and the report gives the reason for keeping it. If the
  goal is synthetic audio (procedural music, UI tones, forge output), realness is a reading to report, not a filter.
- The realness thresholds change with length, and each summary prints the ones it used. Clips of 6 s or less use lo
  0.17 and hi 0.77. Longer files average 4 s windows and use file thresholds lo 0.40 and hi 0.77, which are only
  loosely calibrated. Across files of different lengths, compare the verdicts, not the raw p_real.
- Realness judges the sound itself, not its origin. Polished professional synthetic tones (UI sounds, chimes, synth
  stabs) also score as synthetic. When realness cannot run, the report says that real versus synthetic was not
  checked. Tagger classes like sine wave, beep or synthesizer are only weak evidence.
- Runs of exact digital zero are not evidence by themselves. Real recordings often carry edit or fade padding. The
  source found such padding in 59 % of its licensed real set (NEEDS-DATA for hark, see the index).
- A and B can be 2 trims taken from one multi-take file.

## 4. Loop 2: review a mix or a film

| step | command | output to read |
|---|---|---|
| 1 | `hark timeline <film.mp4> --cues cues.json` (or `<soundtrack.wav> --video <film.mp4>`) | a text transcript of the audio: timed events with level and tags, sections, the loudness curve, picture cuts, and sync for each cut and cue. `--detail low` runs quickly. `--detail high` tags at most 96 events in order of importance, and the summary reports the tagged count, so an event without tags is not a missing sound. |
| 2 | `hark sheet <film> -o sheet.png --cues cues.json [--from 10 --to 25] [--detail low]` | one annotated picture: frames, sections, labelled events, a spectrogram, loudness, beats, and cue markers coloured by sync (green: within one frame; amber: within 100 ms; red: no sound). The agent opens the PNG with its image reader. Gaps, crowding, masking and late hits are visible immediately. |
| 3 | `hark critique <film> [--fast]` | a scorecard measured against the range of 7 professional launch films. Each metric is ok, low or high with a weight, followed by timed actions and an overall verdict (launch-grade, close, needs work, not launch-grade). `--fast` leaves out the tagger and realness passes. |
| 4 | `hark measure <mix.wav> [--music]` | precise meters: LUFS, true peak in dBTP, LRA, noise floor, attack and decay, brightness, bands |

Guidance (the `timeline` unit owns the commands):

- `offset_ms` is sound time minus picture time, so a negative value is early. The `chance` fraction is the score
  random edit points would get. Sync is meaningful only when it beats chance.
- A cut with no hit listed may still have a quiet whoosh. Look at the sheet before adding anything.
- Work on the critical scorecard rows first. After the fix, render again and repeat steps 1-3.
- For the "onset on visual events" share, the scorecard counts a new attack only if it falls within +-100 ms of the
  visual event. The reference films were scored the same way. Its suggested fixes list cuts that have no sound at all, using the timeline's matcher.
- If the input is one of the profile's reference films, critique reports that and scores it against the remaining
  references.

## 5. Loop 3: A/B a change

- Run `hark ab before.wav after.wav --query "<description>"`, or use `--refs <pro refs>/` instead of a query. The
  result names the closer file and describes B against A in measured terms (louder, brighter, longer decay).
- When the sounds sit inside longer files, give the trim times. Comparing 2 whole multi-take files compares all takes
  together with the gaps between them.
- Follow with `hark timeline after.wav` and compare its events and sync figures with the previous run.
- Make one change per comparison.

## 6. Loop 4: make a sound in code (forge)

This loop is for a sound the user wants built rather than picked. All output samples are computed. No recorded audio
goes in, impulse responses included. Real recordings serve only as the references that the critic scores against.
(The `forge-core`, `recipes-impacts` and `recipes-motion` units own the engine and the recipe interface.)

1. Pick an existing recipe or write one at `hark/forge/recipes/<name>.py`. A recipe states its parameter ranges,
   defaults, target description and plausible confusions. Its render function takes parameters and a random generator
   and returns a 48 kHz array. It builds the sound from the forge DSP kit: modal resonators, contact excitations,
   coloured noise, a state-variable filter whose settings move over time, a computed room, and a microphone model with
   a noise floor. Base the recipe on the physics of the real source as measured on the references: which object
   strikes which, what resonates, for how long, and in what space.
2. Run a round with `hark forge <recipe> --refs <real refs> --budget 144 --reps 2 --tone 1.5 --keep 4 --robust 8 --dir
   <round dir>`, or with a per-round Python script that loads the models a single time. `--tone` adds tonal balance
   and tail, both model-free, to the objective. `--reps 2` scores every candidate on 2 renders so a lucky render cannot
   win. Budget about 3-5 minutes per 160 renders on a loaded laptop CPU, plus a model warm-up of 30-60 s.
3. Read the round's findings. The summary of each pick says what it resembles, how far its decay lies outside the
   references, and how far each band of its tone lies from the reference median (with the bands that fall outside
   every reference). Check each pick from 4 angles. First, the model-free tone words (bands outside every reference,
   energy shares, tail, length), which are the most reliable signal once the model scores are high. Second, `robust`,
   the mean over fresh seeds rather than the luckiest render. Third, `alt_zs`, the confusion that comes closest.
   Fourth, spectrograms beside those of the references (`ffmpeg -i x.wav -lavfi
   showspectrumpic=s=900x360:scale=log:legend=0 x.png`).
4. The tagger tags in `report.json` remain a held-out check only if no one selects or steers by them. Decide the
   selection rule first (the robust mean, for example), then look at the tags and report them.
5. When something is wrong (no release tick, a metallic ring, an overlong tail), change how the recipe is built, not
   just its parameter ranges. Run again. Log the change and the numbers for every round.
6. Report the result beside the real references, scored by the same critic (the reference scores in `report.json`,
   computed leave-one-out). Do not overstate it.

The skill must spell out these Goodhart risks:

- The search maximises scores from hark's own models. Built sounds often beat real recordings on those scores. A
  satisfied critic is not evidence of a better sound.
- p_real is simple to fool. Modal resonators with a room and a microphone noise floor already score as real.
- Typicality favours a sound that averages the references.
- Model checks saturate. After a search, every model check, including held-out phrasings and separate real sets,
  places the built sound among the real ones. At that point only measurements and human listening still tell them
  apart.
- The basis for judgement is measured distance to the references (tonal balance, bands outside every reference, tails,
  lengths), robust re-renders and spectrograms.
- A built sound is called as good as real only after a human blind test: shuffled order, matched loudness, key hidden
  until the end.

The source describes one early trial. An independent judge measured a built key press 8-13 dB away from the real
tonal balance, and built pops with no band limit and long hissy tails. Every model check had rated them real. After
the model-free check steered the next rounds, the rebuilt sounds fell inside the measured spread of the real
recordings. Nobody had listened to them at that point. hark has run no such trial (NEEDS-DATA, see the index).

## 7. Loop 5: a film's sound kit (sfx and kit)

This loop serves a code-made film, or any cue sheet, that needs effects from the bundled bank rather than new synth
code. It loads no models. A full kit renders in roughly a minute, and a re-render takes seconds. (The `bank-kit-sfx`
unit owns the interface.)

1. Begin with a preset: `hark kit init --preset <calm|crisp|minimal|launch> -o audio/kit.json`. Fit: calm for films led
   by text, crisp for energetic product films, minimal for films with designed effects only, launch for films edited
   to music. Every role has a `note` giving the reason for its sound. In calm, crisp and minimal, riser, swell and suck
   are switched off, so a reveal resolves on a single element and the lockup is a single clean hit. launch uses one
   riser or reverse swell at a section change, plus the music-led families listed below.
2. Adapt it to the film. `hark sfx list --kind hit` (or `--category <family>`) lists candidates with their lengths,
   peak times and knobs. Per role, adjust `sound`; `edits` (`pitch` set to the film's root for the hit or the drop,
   `dur` and `apex` for a move of fixed length); `takes` (at least 12 for typing, 2-4 for other roles); and
   `level_db`. To audition one candidate, run `hark sfx render <ID> -o x.wav [--pitch E5] [--dur 0.8 --apex 0.32]`
   and read the onset, peak and end it measured.
3. Render with `hark kit render audio/kit.json --out audio/kit`. The mixer works from the takes and from
   `kit.lock.json`, which records onset, peak, end and loudness for each take. Placement rules: hits and touches put
   the onset on the cue frame, a whoosh puts its peak on the apex, and a riser puts its end on the hit. A role that
   aligns on `peak` (the launch riser and swell do) uses the peak instead, because its members reach their climax
   before they end. `kit check` adds a note for a role aligned on its end.
4. Check with `hark kit check audio/kit.json --out audio/kit`. It rejects settings no render can honour: a knob or
   pitch the sound does not support, a level above 0 dBFS, or 2 takes that would write the same file. Each warning
   gives a take and a cause: clicks, a truncated tail, comb ring, silence, a pitch rising more than 4 semitones, a hit
   with 2 attacks or with a build in front of it, or `pitch_off` (the knob missed the note). Replace the sound or
   change the knob instead of keeping a take with a warning. The notes matter as well (a sounded riser, a composite
   sting).
5. Play the finished film through loop 2 (`timeline --cues`, `sheet`) and confirm that every cue landed.

Music-led families, listed with `hark sfx list --category <name>`:

- `ui_pop`: rounded mid-range pops for chips, bubbles and list rows coming in. Films of this kind use it more than any
  other effect.
- `knock`: wood and plastic in the mid band.
- `cut_hit`: short sub-bass hits with a kick.
- `mallet`: gated notes of glockenspiel, celesta and marimba, a soft pluck, and chime runs. `--pitch` tunes them.
- `drip`: water drops with a rising pitch. The check permits this rise for a bubble.
- `shutter`: snap zooms and whip-cuts.
- `reverse_swell`: peaks just after a cut. Use one per film.
- Also: the big-whoosh thumps; riser dark, build and snatch; swish bright and flick; fast-whoosh whip; glitch burst,
  mid and chirp; notify mid-tine and mid-wood.

For films edited to music these families beat bright 5 ms blips and long cinematic tails.

Levels follow the music instead of a fixed value (the launch preset, or any role that sets `bed_db`). Over music,
effects sit 3-10 dB above the bed and are heard through their timing and spectrum. A fixed level drowns in loud
sections and pokes out of quiet ones. Steps for each cue:

1. Render the kit. Each take stores its loudness, and each role stores `bed_db` and `offset_ms`.
2. Measure the bed: the median 50 ms level of the music in the second before the cue.
3. Derive the take's gain from the lock, capped so the peak stays at or below -1 dBFS.
4. Place the take. Taps, knocks and clicks lead the frame, and whoosh peaks fall after it.
5. Keep a run of cues apart by at least the role's minimum gap (pops 114 ms, keys 87 ms).

The kit module provides one helper for each of steps 2-5. Before a hit, a drop, a big whoosh or a swell, dip the
music 6-12 dB for 0.3-0.5 s and restore it on the hit (`mix.stop_down` in the lock). Do not duck the music for every
effect. With no music bed, `level_db` applies. If the music is silent just before a cue, the bed level is none and the
gain falls back to `level_db`. The report lists the gains applied and the bed levels measured.

What the skill must say about the bank: its sounds come from code. The music-led families were tuned to match measurements
taken from a private collection of licensed reference clips, then checked for originality (CLAP similarity no higher
than 0.84 to any reference). Another 15 sounds were fitted as code from CC0 recordings (`fit/<role>/<name>`, each
naming its source). The presets use those for clicks, ticks, keys, touches and the snap. Selection and checking were
done by measurement only. Nobody ran a listening test. None of these are recordings, so a brief that asks for recorded
sound goes to the `sound-sourcing` skill. To add a fit of your own: `hark fit --takes`, then `hark bank add-fit` (the
`fit` unit).

## 8. Reading the numbers

| number | meaning | trust |
|---|---|---|
| LUFS, dBTP, LRA, dBFS, ms, Hz | meter readings (BS.1770 loudness, spectrum, envelope) | exact; set levels from these |
| z (match, rank, find, describe tags) | how strongly one description stands out from the few hundred vocabulary descriptions scored against the same sound | z >= 3 strong, 2.5-3 plausible, 2-2.5 weak, < 2 no match |
| p_vs_alts | a sharp, uncalibrated forced choice among the query and its alternatives | tells only which wording wins; use z for the judgement |
| cosine | raw CLAP similarity | not a probability; compare only within one sound and one query |
| tagger labels (0..1) | per-class probabilities from the AudioSet tagger, each independent | fine for broad classes; deaf above 8 kHz |
| p_real, verdict | a small fitted classifier: real or professionally produced, or code-synthesized | strong but not conclusive; read the evidence; clip (<= 6 s) and long-file thresholds differ |
| digital_silence_frac | fraction of samples that are exactly zero | weak on its own |
| typicality z (compare) | distance to the reference set relative to the set's own spread | >= -1 fits; < -2 is an outlier |
| sync offset_ms, chance | sound minus picture per cut or cue; the score random edits would get | meaningful when well above chance |
| critique status | the value against the range of 7 professional launch films (loudness follows web-delivery practice; true peak <= -1 dBTP for a master and <= 0 dBTP for decoded AAC or MP3, which already include codec overshoot) | a range, not a target; ok means within what professionals do |

Trust order: measurements first, then verdicts, z values and probabilities. A cosine is never reported as a
confidence. (The source stores held-out evidence for each z band in an evaluation results file. In hark the `listen`
unit owns the equivalent.)

## 9. Optional second opinion in words (LLM critique)

The source uploads audio to a cloud audio model, which returns a rubric critique: sync, realism, mix, loudness, what
sounds cheap, and timed fixes. The operator decided hark uses the Antigravity CLI (`agy -p`) for this, provided probe
ticket 11 confirms that agy can hear audio. These rules hold for any backend:

- The critique runs only after the user explicitly agrees to the upload. The agent asks first. It never sets the
  consent flag (`--yes-upload`, or `consent=true` over MCP) for the user.
- The upload is audio only: mono, 64 kbps.
- Options: `--with-timeline` attaches the timeline, and `--prompt "..."` adds a specific question.
- Its times are rough and its scores are one model's view. Each suggested fix is confirmed with timeline and ab.

## 10. Writing queries and limits

Query wording:

- Name the sound, not what it is for. Use source, material, action and character: "a metal locker door banging shut in a
  tiled corridor", not "a sound for the title card".
- Supply alternatives that a listener could actually mistake for it (thock vs click vs typewriter).
- If a z is near a boundary, try 2 or 3 wordings, and believe a reading only if it survives all of them.
- hark cannot hear a purpose. In the source's test, a purpose phrase ("something appearing") matched none of 9 real
  pops, while each pop's own title matched it. To test a sound for a purpose, describe the sound that purpose calls
  for in 2 or 3 ways and keep the best reading. Across 195 real effects this lifted passes from 152 to 176 and
  separated purposes a little better (NEEDS-DATA for hark).

Limits the agent states when they apply:

- hark does not measure taste. It identifies a sound and meters it. Beauty and fit with the story are the user's
  call, so the agent supplies evidence and a shortlist.
- No speech recognition: hark detects speech but not its words. No judgement of lyrics or language.
- CLAP covers 50 Hz - 14 kHz and the tagger stops at 8 kHz. Problems with air or a low-pass need measure (bandwidth,
  bands).
- Very short sounds get padded to a 10 s window. Isolated hits read best through find or describe on a trimmed span.
- attack_ms and decay_ms use smoothed envelopes (5 ms and 10 ms RMS). An instant attack shows about 4 ms, and very
  short clicks cannot show a decay below about 10-20 ms.
- Picture cuts come from frame differences. Hard cuts and whips are detected reliably. Slow crossfades are missed or
  found late. Movement within a shot is not detected as an event, so animation needs cue times (`--cues`).
- Dense mixes smear the tags. Describe one stem or one segment, not the full mix.
- Rights, provenance and attribution are outside what audio can answer. hark cannot check them. The `sound-sourcing`
  skill carries the usage-rights audit.

## 11. Game SFX (new in hark, not in the source)

The source's skill covers film only. The operator also wants game SFX. Ticket 22 designs the game workflow from the
reusable parts below, and links to the `godot-sound-design` skill for engine wiring:

- Variation pools: kit `takes` and bank renders give N variants per role. The variation rules in part 2, 7.6 (4 or
  more takes, +-1-2 dB, no more than +-0.3-0.5 semitones, no take twice in a row) carry over to repeated game actions
  such as footsteps, hits and UI.
- Per-take measurement: `kit.lock.json` holds onset, peak, end and loudness for each take. A game uses these to trim
  takes and to keep variant levels even.
- `kit check` warnings (clicks, truncated tails, silence, pitch drift) are the same defects that ruin a looped or
  rapidly retriggered game sound.
- Loops (ambience beds): the equal-power loop crossfade in part 4, 2.6 and the edges check in part 4, 4.4 show how to
  judge a loop point from measurements.
- Loudness: films master to -14 LUFS. Game assets usually ship normalised per asset class and get mixed at runtime.
  The game target is open (index Q6).

## 12. Acceptance tests (skill level)

| # | input | expected properties |
|---|---|---|
| H1 | `skills/hark/SKILL.md` | under 12288 bytes; frontmatter `name: hark`; a description naming the cases of section 1 |
| H2 | the skill text | every command it teaches uses only flags that exist in the hark CLI (each appears in `hark <cmd> --help`) |
| H3 | the skill text | states that no claim about how audio sounds is made without a reading, and that readings are quoted |
| H4 | the skill text | contains the 6 Goodhart risks of section 6 and the bank statement of section 7 |
| H5 | the skill text | states the LLM consent rule, and any example command that shows a consent flag has the ask-first rule beside it |
| H6 | the skill text | contains no upstream name and no secret or key value |
