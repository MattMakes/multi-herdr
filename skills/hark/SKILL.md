---
name: hark
description: Use before you say anything about how audio sounds - when you choose 1 effect, music track or ambience out of several, find the cleanest span in a long recording, decide if a file is a real recording or code-synthesized, audit a soundtrack or a video's audio (timing, picture sync, loudness, balance), compare 2 mixes, build a sound in code and refine it against measurements (forge, fit), dress a code-made film from the bundled bank (sfx, kit), or make, vary and check game SFX one-shots and loops. hark listens and meters on the local CPU.
---

# hark: ears for an agent

You have no ears. Rule: every statement about how audio sounds, or whether a sound suits its purpose, needs a hark
reading behind it, and the report quotes that reading. A good citation: "z +3.4 (strong); p_real 0.91; -13.8 LUFS
integrated". Without a reading, say that you did not measure it.

## Run it

- Launcher: `scripts/hark` in this skill folder (`skills/hark/scripts/hark` in the repo; the same relative path in a
  fleet skill bundle). Call it by path. Another skill reaches it as `<its folder>/../hark/scripts/hark`. It runs
  `HARK_PYTHON` (default `~/.local/state/hark/venv/bin/python`) with `engine/` on the path, and exits 3 when there is
  no venv.
- Python: run that Python with `<this folder>/engine` on `sys.path`, then `import hark; hark.find(...)`. 1 model load
  for a whole batch.
- `hark doctor` checks packages, ffmpeg and models. Errors are 1 line `hark <cmd>: error: ...`, exit 1. Every command
  takes `--json`, `--out FILE`, `--threads N`. On a shared machine use `HARK_THREADS=2`.
- Optional ML extra (CLAP and the AudioSet tagger: torch, transformers). The listening commands need it; `measure`,
  `sfx`, `kit` and `gallery` do not. Without it, an ML command exits 3 and prints the install command (on a terminal
  it asks y/N). Show the command to the user and ask before you install. Then `hark doctor --fetch`.
- Many calls in 1 session: `hark mcp` keeps models loaded. Setup, MCP registration, costs: [setup](references/setup.md).

## Which command

| need | command |
|---|---|
| shortlist files for a description | `hark rank "<description>" <folder>/ --top 5` |
| the best spans inside a long take, as trims | `hark find <file> "<single event>" --top 3 [--kind event\|texture]` |
| what a span sounds like (tags, attack, decay) | `hark describe <file> --start S --end E` |
| test 1 claim against confusions | `hark match <file> "<claim>" --alts "<alt>" "<alt>"` |
| real recording or code-synthesized | `hark realness <file or folder> [--start S --end E]` |
| which of 2 is closer; how B differs | `hark ab a b --query "<description>"` (or `--refs DIR`; trims `--a-start ...`) |
| fit with an approved palette | `hark compare <file> --refs <dir>/` |
| meters: LUFS, dBTP, LRA, floor, bands | `hark measure <file> [--music]` |
| transcript of a mix or film, sync per cue | `hark timeline <film> --cues cues.json [--detail low\|high]` |
| 1 annotated picture to open | `hark sheet <film> -o sheet.png --cues cues.json [--from S --to S]` |
| launch-film scorecard | `hark critique <film> [--fast]` |
| build a sound in code | `hark forge <recipe> --refs <dir> --budget 144 --reps 2 --tone 1.5 --keep 4 --robust 8 --dir <out>` |
| turn a recording into recipe code | `hark fit <file> -o recipe.py --takes` |
| bank sounds; 1 sound aimed at picture | `hark sfx list --kind K`, `hark sfx render ID -o x.wav [--pitch E5] [--dur S --apex S] [--seed N]` |
| audit any effect file | `hark sfx check <files>` |
| a film's kit | `hark kit init --preset calm\|crisp\|minimal\|launch`, `kit render KIT --out DIR`, `kit check KIT --out DIR` |

Detail and guidance per loop: [listening](references/listening.md) (choose an asset, read the numbers, write queries,
limits), [review](references/review.md) (review a mix, A/B a change), [forge](references/forge.md),
[kit](references/kit.md), [game SFX](references/game-sfx.md).

## Video SFX workflow

1. Cues: export cue frames from the picture code (contact, 50 % of a pop-in, velocity peak, the hit a riser builds
   to, the first legible frame of the logo). Hand-typed times land frames off.
2. Sound source. Real recordings wanted: the `sound-sourcing` skill. Built sounds: a kit from the bank
   ([kit](references/kit.md)), or forge for a sound the bank lacks. Pick between candidates with rank, find, ab and
   realness.
3. Place: hits, clicks and keys by onset on the cue; a whoosh by its peak on the apex; a riser by its end on the hit.
   The `video-audio` skill owns the voice-over, the beat grid (`audio/beats.json`), ducking and the -14 LUFS / -1 dBTP
   master: use its grid to snap hits and reveals to the beat.
4. Review: `hark timeline --cues`, `hark sheet` (open it), `hark critique`. Fix the critical rows first, render, repeat.
5. A/B each change with `hark ab` and a fresh timeline. 1 change per comparison.

Restraint: about 5 designed effects per 10 s, 1 click voice per film, no whoosh on every cut, the drop is the loudest
moment, the logo resolves 8-20 dB under it.

## Game SFX workflow

1. List events with their rate and whether they carry a value. The rate sets the length (more than 8/s: 30-90 ms;
   2-8/s: at most 150 ms; rare rewards 400-1000 ms).
2. Make or pick each sound (bank `sfx render --seed N`, a kit, forge, fit, or recordings via `sound-sourcing`).
3. Build variation pools (rules below). `kit.lock.json` holds onset, peak, end and loudness per take: trim and level
   the pool from it.
4. Check every asset: `hark sfx check` or `hark kit check` (clicks, truncated tails, silence, pitch drift, 2 attacks);
   onset within 1 ms and 20 dB under the peak within 5 ms; loops judged at the seam by measurement.
5. Loudness: normalise per asset class with 1 peak ceiling, not to -14 LUFS; the engine mixes at run time.
6. Hand off an asset pack with a manifest. The `godot-sound-design` skill owns latency, buses, the event-to-sound map
   and the in-game audit. Full steps: [game SFX](references/game-sfx.md).

## Variation for repeated sounds

- At least 4 takes per repeated action (8-12 for steps and typing), never the same take twice in a row.
- Level jitter +-1-2 dB. Pitch at most +-0.3-0.5 semitones on real materials; vary the take, not the pitch.
- A progression rises: a pitch ramp over a run, a gain ramp into a build.
- No variation on a sound that carries a value (a tier, a combo step) or on the single UI click voice.
- Machine-gunning (1 sample, unvaried, in a regular rhythm) is the fastest way to sound cheap.

## Trust

Measurements (LUFS, dBTP, ms, Hz) are exact. z: >= 3 strong, 2.5-3 plausible, 2-2.5 weak, < 2 no match.
`p_vs_alts` only says which wording won. A cosine is never a confidence. The tagger is deaf above 8 kHz and unsure on
one-shots under 0.5 s. Realness thresholds depend on length (clips <= 6 s: lo 0.17, hi 0.77; longer: lo 0.40, hi
0.77). Name the sound, not its purpose, in queries, and try 2-3 wordings near a boundary. hark measures identity and
level, not taste, and cannot check rights.

## Forge: Goodhart risks

1. The search maximises hark's own model scores; built sounds often beat real recordings there. A satisfied critic is
   no evidence of a better sound.
2. p_real is easy to fool: modal resonators with a room and a mic noise floor already score as real.
3. Typicality favours a sound that averages the references.
4. Model checks saturate: after a search every model check places the sound among the real ones.
5. Judge by measured distance to the references (tone bands, tails, lengths), robust re-renders and spectrograms.
6. Call a built sound as good as real only after a human blind test: shuffled, loudness-matched, key hidden.

## The bank

The bank's sounds come from code. The music-led families were tuned to measurements of a private set of licensed
reference clips and checked for originality (CLAP similarity at most 0.84 to any reference). 15 sounds were fitted as
code from CC0 recordings, each naming its source. Selection used measurement only; nobody ran a listening test. None
are recordings: a brief that asks for recorded sound goes to `sound-sourcing`.

## Second opinion in words (optional)

`hark llm` sends a short mono audio clip to an audio model through the Antigravity CLI (`agy`). Ask the user first:
the audio leaves the machine. Never set the consent flag for the user. Only after they agree: `hark llm <file>
--yes-upload [--with-timeline] [--prompt "..."]`. Its times are rough (off by 0.2-0.3 s); confirm every fix with
`hark timeline` and `hark ab`.
