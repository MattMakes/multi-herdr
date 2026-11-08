# The process: roles, steps, gates, rounds

The lead runs 11 steps. Parallel agents never write the same file. One writer per file stops lost edits and fixes
that cancel each other out.

## Placeholders

The lead fills every placeholder before a brief goes to a sub-agent. A sub-agent never sees `{{...}}`.

| placeholder | value |
|---|---|
| `{{PY}}` | the hark venv Python, `~/.local/state/hark/venv/bin/python` (numpy, scipy, soundfile, soxr, librosa, hark) |
| `{{HARK}}` | the hark launcher, `skills/hark/scripts/hark` |
| `{{SKILL}}` | this skill's folder |
| `{{ROOT}}` | the film project root |
| `{{CATEGORY}}`, `{{PREFIX}}` | the agent's category and its name prefix (`ui.`, `kb.`, `mo.`, `im.`, `am.`, `mu.`) |
| `{{FILM}}`, `{{VIDEO}}`, `{{CUES}}` | film name, rendered video, cue file |
| `{{ROUND}}` | critique round number |
| `{{RENDER}}`, `{{REPORT}}`, `{{TRACE}}`, `{{QA_JSON}}` | the render folder under review, its mix report, trace and QA report |
| `{{CHANGED}}`, `{{FIXED}}` | what changed since the last round (check it hardest); what is fixed (do not raise it again) |
| `{{BAR_REFERENCE}}` | the quality target in 1 line |
| `{{NEEDS}}`, `{{NEED_DESCRIPTION}}`, `{{SINGLE_EVENT_DESCRIPTION}}`, `{{EVENTS}}` | the needs, query texts and cue classes of 1 agent |
| `{{KEY}}`, `{{BPM}}`, `{{BEATS_PER_BAR}}`, `{{BAR}}`, `{{FILM_BARS}}`, `{{DROP_T}}`, `{{DROP_BAR}}`, `{{BREAKDOWN}}`, `{{LOGO_T}}`, `{{DURATION}}` | music facts and key moments |
| `{{CRITIC_REPORTS}}` | the critics' JSON, for the fixer |

`{{PY}} -I {{SKILL}}/scripts/<script>.py` runs a skill script. `{{HARK}} <command>` runs hark.

## Steps and gates

| step | owner | output | gate |
|---|---|---|---|
| 0 cue list | lead | `out/cues.json` | sync frames come from the picture code |
| 1 brief | lead | `audio/SOUND_BRIEF.md` | each cue names a sound need, or "nothing" |
| 2 sourcing (parallel, 1 agent per category) | sourcing agents | `audio/assets/<cat>/`, `audio/assets/manifests/<cat>.json` | every file measured; hark realness says "real or pro-produced" for the trim in use ("uncertain" only with evidence in the manifest and a check on the trim) |
| 3 usage-rights audit | rights auditor | `audio/assets/manifests/rights.json` | `rights_audit.py` exits 0 for everything the film uses |
| 4 music edit | music supervisor | `audio/design/music.json`, `music_choice.json` | grid verified, drop on the reveal, clean splices |
| 5 design fragments (parallel) | designers | `audio/design/<cat>.json` | solo render and solo QA clean |
| 6 integrate | lead | soundtrack, stems, report, trace | master gate passes (+-0.5 LU, <= -1.5 dBTP) |
| 7 QA and listening | lead | `out/qa/qa.json`, hark timeline, sheet, critique | QA has no fail |
| 8 critique (5 lenses, parallel, read-only) | critics | ranked issues with tested operations | |
| 9 fix | 1 fixer | the design | nothing that passed before fails now |
| 10 master, credits, mux | lead | `CREDITS.md`, description text, the film | credits exit 0; mux lag 0 |

## Ownership

| role | brief | writes | reads |
|---|---|---|---|
| lead | `SKILL.md` | the brief, `audio/sounddesign.json`, final reports | everything |
| sourcing agent | `role-sourcing.md` | `audio/assets/<cat>/`, its manifest, `out/audio-review/<cat>/` | the brief |
| rights auditor | `role-rights-audit.md` | the agent fields of `rights.json`; manifest corrections it reports | manifests, item pages |
| music supervisor | `role-music.md` | `music_choice.json`, `audio/design/music.json`, `out/audio-review/music/` | candidates, the picture map |
| designer | `role-designer.md` | `audio/design/<cat>.json`, `out/audio-review/<cat>/` | its manifest, cues, solo renders |
| critic (1 per lens) | `role-critic-<lens>.md` | only `out/audio-review/critic_<lens>/` | render, design, trace |
| fixer | `role-fixer.md` | every fragment, top-level buses and master, in the fix round only | every critic report |

## Commands per step

Sourcing (step 2):

```
{{PY}} -I {{SKILL}}/scripts/analyze.py audio/assets/ui/* --png out/audio-review/ui --out out/audio-review/ui.json
{{HARK}} realness audio/assets/ui/
{{HARK}} rank "one dry laptop touchpad tap" audio/assets/ui/ --top 5
{{HARK}} match <file> "a wine bottle cork popping" --alts "a cap gun" "a paper bag burst"
{{HARK}} find <long take> "one press of a mechanical switch key" --top 8
```

Rights (step 3): `{{PY}} -I {{SKILL}}/scripts/rights_audit.py --root . --report out/qa/mix_report.json [--fetch]`.

Music (step 4): write `audio/arc.json` first, then
`{{PY}} -I {{SKILL}}/scripts/arrange.py <track> --film-bpm 120 --arc audio/arc.json --out out/audio-review/music/arrange.json`.
On LOW downbeat confidence it prints 2 grids: pick one by the first note, the section entries and the chord changes,
and rerun with `--phase N`.

Solo verification (step 5), per fragment:

```
{{PY}} -I {{SKILL}}/scripts/mix.py --only ui --dump out/audio-review/ui/dump
{{PY}} -I {{SKILL}}/scripts/mix.py --only ui --trace out/audio-review/ui/adds.json --out out/audio-review/ui/solo.wav
{{PY}} -I {{SKILL}}/scripts/qa.py --only ui --report out/audio-review/ui/solo_report.json --stems out/audio-review/ui/solo_stems --trace out/audio-review/ui/adds.json --out out/audio-review/ui/qa.json
{{HARK}} timeline out/audio-review/ui/solo.wav --cues out/cues.json
```

Integrate (step 6): `mix.py --root . --design audio/sounddesign.json --cues out/cues.json --trace out/qa/adds.json`.
Clear missing files and a failed gate before the critique.

QA and listening (step 7):

```
{{PY}} -I {{SKILL}}/scripts/qa.py --layers
{{PY}} -I {{SKILL}}/scripts/qa.py --listen --realness --video out/film.mp4
{{HARK}} critique out/audio/soundtrack.wav --video out/film.mp4
{{HARK}} sheet out/audio/soundtrack.wav -o out/qa/sheet.png --video out/film.mp4 --cues out/qa/cues_sounded.json
```

Critique (step 8): each critic tests its fixes as a variant in its own folder:
`variant.py --ops out/audio-review/critic_<lens>/ops_v1.json --out out/audio-review/critic_<lens>/v1 --render --qa`.

Fix (step 9): 1 fixer merges the operations, checks the merged variant against the last `qa.json`, then runs
`variant.py --ops ... --apply --backup out/audio-review/backup_round<N>`. A confirmed deliberate exception gets
`"qa_ok": "<reason>"`.

Finish (step 10): the mixer masters to -14 LUFS with a true-peak limiter at ceiling 0.77, gated at +-0.5 LU and
<= -1.5 dBTP on the WAV (about -1 dBTP after AAC). Then:

```
{{PY}} -I {{SKILL}}/scripts/rights_audit.py --report out/qa/mix_report.json
{{PY}} -I {{SKILL}}/scripts/credits.py --root . --design audio/sounddesign.json --rights audio/assets/manifests/rights.json
{{PY}} -I {{SKILL}}/scripts/mux.py --video out/film.mp4 --audio out/audio/soundtrack.wav --out out/film_sound.mp4
{{HARK}} timeline out/film_sound.mp4 --cues out/qa/cues_sounded.json
```

Cross-correlate the MP4's decoded audio with the WAV: the lag must be 0 samples. Paste
`out/CREDITS_description.txt` into the video description, because CC BY needs the credit there.

## Critic lenses

| lens | question | key evidence | brief |
|---|---|---|---|
| sync | does each sound land on its cause? | QA sync, frames, sample envelopes | `role-critic-sync.md` |
| edges | is anything cut, clicking or chopped? | QA clicks and edges, dumps, master ends | `role-critic-edges.md` |
| masking | can every effect be heard; is the layer too dense? | QA audibility (`--layers`), density | `role-critic-masking.md` |
| tonal | is the mix comfortable on phones and laptops? | QA harshness, band levels, low end, key | `role-critic-tonal.md` |
| director | does the soundtrack alone tell the story? | timeline, sheet, critique, loudness curve | `role-critic-director.md` |

Priorities: P0 is audible on a key beat (hook, reveal, logo). P1 is noticeable on a normal listen. P2 is a refinement.
Rank issues by improvement per minute of work.

## Thresholds and where they come from

| threshold | value | origin |
|---|---|---|
| master | -14 LUFS, ceiling 0.77, gate +-0.5 LU and <= -1.5 dBTP | web delivery practice; the ceiling leaves room for AAC overshoot |
| sync | lead <= 45 ms; impact peak <= +75 ms; whoosh apex +-45 ms; riser end -250..+30 ms (checked -300..+60) | measured norm of professional launch films: accents land -45..+75 ms, median +8 |
| audibility | read >= 6 clear, 3-6 ok, 0-3 barely, < 0 masked | 1/3-octave margin of the effect over everything else |
| harshness | 2-6 kHz 5 ms RMS more than 6 dB over the music's loudest presence peak | phones make that band painful first |
| density | about 5 designed effects per 10 s | launch-film norm (range 0-28) |
| LRA | 5-9 LU target, warn outside 4-10 | launch-film norm |

## Variants

A variant is a full copy of the design in its own folder. `variant.py --out DIR` patches only the changed values,
so `diff -r` shows exactly what a critic tried. `--render --qa` renders it and writes `DIR/qa.json`. Compare that
with `out/qa/qa.json` section by section. `--ops []` gives a bit-identical render of the base design. Interactions
show up only in a render: a high-shelf cut followed by `norm_peak_db` can make the cut band louder, because the
peak is normalised again after the EQ.

## Rounds, stopping, reporting

- Run at least 1 critique round and 1 fix round. Add a 2nd round when the fix changed levels or the music edit.
- Stop when QA has no fail, every remaining warn has a `qa_ok` reason, and the director lens reports no P0.
- Every number in a report comes from a command that ran. The report lists those commands.
- Keep `out/audio-review/`. It holds the evidence behind every number.
- A QA finding is a lead for a critic, not an order to change the design.
- Expected scale on a good 60 s mix: a handful of clicks inside recordings (their own transients), a few masked events
  where layers stack, a few harsh events, 1-2 deliberate edge findings, and 1-2 cue events with no sound.
