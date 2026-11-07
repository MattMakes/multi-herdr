# forge-core: the forge engine of hark (index)

The forge makes sound effects from code. A recipe is a parameterised renderer. The forge searches the recipe's
parameters, scores each render with a critic (text-audio model judgements plus model-free tone, envelope, artifact
and polish checks against real reference recordings), and writes the best, varied, robust picks as WAV files with a
report that re-renders each pick sample for sample. Knobs aim a finished sound at target numbers (length, apex,
pitch, decay, onset, brightness, width). A held-out benchmark and a blind listening test measure the results against
real recordings that no search ever used.

The package is `hark`. The engine lives at `skills/hark/engine/hark/`, so the modules of this unit are
`hark.forge`, `hark.forge.dsp`, `hark.forge.fx`, `hark.forge.tone`, `hark.forge.polish`, `hark.forge.knobs`,
`hark.forge.schema`, `hark.forge.search`, `hark.forge.critic`, `hark.forge.refsets`, `hark.forge.viz`, the recipe
package `hark.forge.recipes` with its helper `hark.forge.recipes._tonal`, and the evaluation scripts in `eval/forge/`.
Environment variables: `HARK_HOME` (cache root), `HARK_THREADS` (worker threads), `HARK_REFSETS` (refs registry path),
`HARK_REFS_PRO` (pro-study trims folder for the results scrubber).

The spec is above 60 KB, so it is split into parts. This index is the entry point. Every part has its own
Interface, Behaviour or Algorithms, Constants, Errors and edge cases, and Acceptance tests sections. The index-level
sections below (Data assets, Dependencies on other units, Open questions) cover the whole unit.

## Parts

| part | file | covers | tests |
|---|---|---|---|
| 1 | [forge-core-loop.md](forge-core-loop.md) | the package `hark.forge`: `forge()`, families, `family_box`, `palette()`, `variant_recipe()`, `pick_refs`, `render_params`, audition, report.md, the CLI `hark forge`; the recipe contract (DEFAULTS, SPACE, TARGET, ALTS, REFSET, render(p, r) and the optional hooks); `hark.forge.recipes` discovery; the tonal helpers `_tonal` | L1-L14 |
| 2 | [forge-core-dsp.md](forge-core-dsp.md) | `hark.forge.dsp` (time vectors, RNG, noise, envelopes and curves, contact and excitation, modal synthesis and modal clouds, Butterworth and peaking filters, the TPT state-variable filter, room IR and mic colour, stereo, fades, placement, tail trim, normalise, sweeps, PolyBLEP oscillators, Shepard glissando, wander, air turbulence, sub drop, bells with the BELLS table, FM, onset alignment, panning, stacking and crossfades) and `hark.forge.fx` (the IR cache, early reflections, the 16-line FDN reverb with modulation, noise late field and air absorption; oversampled saturation; transient shaper; look-ahead limiter; LR4 band split; min-phase EQ match; Doppler; decorrelation, pan and width; sweep filter, varispeed, tape stop, crush, stutter, glitch; granular; reverse swell) | D1-D20 |
| 3 | [forge-core-tone.md](forge-core-tone.md) | `hark.forge.tone`: ToneRef (28-band 1/3-octave profile, distance, ratio, outside bands, shares, t40, words, penalties) and EnvRef (bandgram envelope distance with anchor, stretch and shift; outside cells) | T1-T10 |
| 4 | [forge-core-polish.md](forge-core-polish.md) | `hark.forge.polish`: the artifact detectors, PolishRef, the RULES and POLISH penalty tables, words, K-weighted loudness (BS.1770), loudness spread | P1-P15 |
| 5 | [forge-core-knobs.md](forge-core-knobs.md) | `hark.forge.knobs`: the 7 knobs, the LENGTH, PITCH, TONAL and UNTUNED tables, the secant landing over re-renders. Morphs are not in this unit: the source keeps them in its fit package (unit fit) | K1-K11 |
| 6 | [forge-core-schema.md](forge-core-schema.md) | `hark.forge.schema`: render blocks (schema 2), resolve, raw and level, `check_final`, `fill`, `upgrade`, legacy inference, its CLI | S1-S8 |
| 7 | [forge-core-search.md](forge-core-search.md) | `hark.forge.search`: SPACE encode and decode, start filling, render and render_many, the worker rule, ES and separable CMA-ES, the model-free pre-screen, diverse and robust picks, the seed bank | R1-R12 |
| 8 | [forge-core-critic.md](forge-core-critic.md) | `hark.forge.critic`: Critic (match z, realness, typicality, feature penalty, the v2 tone, envelope, artifact and polish terms, designed and real modes), `total_v2`, STUDY_TARGETS, holdout | C1-C10 |
| 9 | [forge-core-refsets-viz.md](forge-core-refsets-viz.md) | `hark.forge.refsets` (the forge / bench registry, the split rule, KIT_BENCH, scan and write) and `hark.forge.viz` (`compare_png`) | RS1-RS5, V1-V3 |
| 10 | [forge-core-bench.md](forge-core-bench.md) | `eval/forge/bench.py` (the held-out benchmark), `frozen_tone.py`, `build_refsets.py`, `scrub_paths.py`, the method, the target numbers and the change log rule | B1-B15 |
| 11 | [forge-core-blind.md](forge-core-blind.md) | `eval/forge/blind_v2.py`: the blind A/B test (picks, symmetric trims, loudness match, cleanliness limits, answer sheet, sealed key, scoring with the binomial p) and the progress file | BT1-BT11 |

Total: 134 acceptance tests.

Source coverage. Every file of the unit maps to a part:

| source file | part |
|---|---|
| forge package init, recipes package init, `_tonal` | 1 |
| dsp, fx | 2 |
| tone | 3 |
| polish | 4 |
| knobs | 5 |
| schema | 6 |
| search | 7 |
| critic | 8 |
| refsets, viz | 9 |
| the forge v2 design notes and the design overview | 1 (loop, CLI defaults), 7 (search setup), 8 (critic terms), 10 (method) |
| the overnight round log | 10 (target numbers, method, change log), 11 (tiers of the blind test plan) |
| bench, frozen_tone, build_refsets, the results summary | 10 |
| blind_v2 | 11 |
| scrub_paths (called by bench) | 10 |

## Main flow (for orientation)

1. `forge(recipe, refs, ...)` loads the recipe (part 1) and builds a Critic (part 8) with tone, envelope and polish
   references (parts 3, 4) from the forge split of the recipe's REFSET (part 9), or in designed mode with study targets.
2. The optimiser (part 7) encodes SPACE to unit coordinates, renders candidates in threads, ranks them with the
   model-free pre-screen, sends the best to the critic, and runs ES or CMA-ES generations until the budget, patience
   or step size ends it.
3. Diverse picks, then robust re-renders over extra seeds, choose the finals. Each final is peak-normalised to -1 dBFS
   and written with its render block (part 6), plus report.json, report.md, compare.png (part 9), audition.wav.
4. Families and `palette()` (part 1) score member sets together and match their loudness (part 4).
5. Knobs (part 5) re-render a bank sound until it lands on target numbers.
6. `bench.py` (part 10) and the blind test (part 11) measure the delivered finals against held-out recordings.

## Data assets

| asset | where | format | class | fallback |
|---|---|---|---|---|
| seed banks | `hark/forge/seeds/<recipe>.json` (part 7) | JSON: good parameter sets per recipe, with pins for delivered members | REGENERATE: our own forge runs write them | ship empty (Q4); searches then start from DEFAULTS and random starts |
| refs registry and cut WAVs | `<HARK_HOME>/refsets.json`, `out/refs/` (parts 9, 10) | JSON registry `{version, categories: {cat: {forge, bench, note}}}`; 48 kHz PCM 24-bit trims; a manifest per trim | NEEDS-DATA: licensed third-party recordings, never shipped | (1) designed mode with no refs, carried by STUDY_TARGETS; (2) users lay out `<root>/<cat>/{forge,bench}/` and call `write(scan(root))`; (3) the build script of part 10 from public items named in its plan table |
| sources list and kit manifest for the refset build | part 10 | JSON | NEEDS-DATA: they describe the source's own library and kit | use our kit (unit bank-kit-sfx) for `kit:` entries, or drop them |
| KIT_BENCH | `hark.forge.refsets` (part 9) | list of kit file stems | rebuilt: fill it from our kit manifest where a kit file shares a source with a bench ref | empty list |
| STUDY_TARGETS | `hark.forge.critic` (part 8) | table of numbers per category | DERIVE: the numbers are measurements of professional films, kept as facts | ship the table as given; re-measuring needs the study films (NEEDS-DATA) |
| pro_* study refs | part 10 | trims of launch films | NEEDS-DATA | pro_* categories get only phrasing, AudioSet, artifact, texture and diversity rows |
| K-weighting filter | `hark.forge.polish` (part 4) | 2 biquads at 48 kHz | DERIVE: from ITU-R BS.1770 (public standard) | none needed |
| BELLS ratios | `hark.forge.dsp` (part 2) | partial ratios and amplitudes per bell | DERIVE: public bell and bar physics | none needed |
| frozen tone copy | `eval/forge/frozen_tone.py` (part 10) | code, specified whole | DERIVE: freeze it at our first benchmark run | none needed |
| benchmark phrasings, confusions, AudioSet labels | `eval/forge/bench.py` (part 10) | text tables | kept as constants | none needed |
| benchmark results | `eval/forge/results_*.json`, `.md` | JSON and markdown | REGENERATE: run `bench.py` on our finals; commit only scrubbed results | none needed |
| first-version finals for the progress file | part 11 | WAV | NEEDS-DATA | skip the progress file, or use our first delivered finals as "v1" |
| blind test folder | `out/showcase/blind_v2/` (part 11) | WAVs, sheet, key, cleanliness JSON, scorer | REGENERATE: one command | none needed |
| schema LEGACY table | `hark.forge.schema` (part 6) | stand-in name -> (module, function) | not carried over: a clean build has no legacy reports | empty table |

NEEDS-DATA list: refs registry and its cut WAVs; the sources list and kit manifest of the refset build; the pro_*
study refs (launch-film trims); the study films (only to re-measure STUDY_TARGETS); the first-version finals of the
progress file.

## Dependencies on other units

| unit | what forge-core uses or gives |
|---|---|
| core-io | audio load (float, 2-D, 48 kHz, with and without the cache) and mono mix; WAV write PCM 24-bit; media listing; report emit and the common `--json` / `--out` CLI flags; the config home `HARK_HOME` and the embedding cache; `HARK_THREADS` |
| listen | the CLAP vocabulary (`embed_text`, `embed_files`, `embed_audio`, `calibrate` with `z_q`, `top_terms`, the padding mode); the realness model; compare typicality z with k neighbours; comparison features and their stats; the AudioSet tagger (`probs_file`, `top`, `prob_of`, its class index); features: K-weighting, momentary loudness curve, true peak in dBTP; find (segments with score, z, verdict, kind, cut flags, head and tail dB, peak dBFS) and rank (z, verdict, sounds most like) for the refset build |
| timeline | no direct calls found. The forge's finals are inputs to timeline placement; cue and frame facts in the round log came from the timeline tools |
| forge-core | this unit |
| recipes-impacts | recipe modules in `hark.forge.recipes` that follow the recipe contract of part 1 (impacts, UI sounds, tonal and digital sounds); their DEFAULTS, SPACE, TARGET, ALTS, REFSET, families and optional `measure` / `target_check` / `study_extra` hooks; their module names fill the knobs tables and any LEGACY entries (Q1) |
| recipes-motion | recipe modules for whooshes, sweeps, risers and downlifters that follow the same contract; the whoosh envelope that knobs drives with the one-argument run (Q2) |
| bank-kit-sfx | bank entries and their renderers that knobs re-render; `estimate_pitch(x, target_hz, span_st)`; `times(x)['peak_s']`; the kit files and kit manifest (KIT_BENCH, the refset build's `kit:` entries) |
| fit | `fit.auto.kind_of(model)` (used by part 5); fit owns morphs and the fit knobs that call into part 5 |
| surfaces | the CLI and MCP exposure of `hark forge` (including `--check`, `--mode`, `--tone`, `--refset`) and of the knobs |
| skills-and-docs | the skill text that tells agents how to run the forge, read report.md, and when to trust the benchmark and the blind test |

## Open questions

- Q1. Recipe module names. The schema LEGACY table and the knobs tables key some entries by recipe module name. Our
  recipe units (recipes-impacts, recipes-motion) choose their own names. The spec keeps category keys and an empty
  LEGACY table. The recipes units must confirm the module names before the knobs tables are filled.
- Q2. Bank renderer call shape. Knobs calls the bank renderer with one argument on the whoosh path and with two
  arguments elsewhere. The spec says the renderer must accept both (part 5). The bank-kit-sfx unit must confirm this.
- Q3. Defaults of `forge()`. The source's Python function keeps first-version defaults (no pre-screen, no seed bank),
  while the CLI uses the second-version setup. The spec keeps that split. Should our engine use the second-version
  setup everywhere?
- Q4. Seed banks. Ship them empty (recommended in part 7) or regenerate them from our own runs before release?
- Q5. The pro_* categories rest on 1-10 events from mostly one film. Are they public categories or study material
  only?
- Q6. The benchmark phrasings, confusions and plan-table queries are short texts kept as constants so that the
  numbers stay comparable. Keep them as given, or write our own (and lose comparability with the source's numbers)?
