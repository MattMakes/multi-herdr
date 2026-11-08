# Spec: skills-and-docs (the `hark` tool skill and the `sound-sourcing` skill)

Clean-room behaviour spec. Written by a spec writer (contaminated side) from the source at commit `4ac3d2a`. A clean
implementer can rebuild both skills from this spec and the other hark specs alone. "The source" means the upstream
project. Our names: tool skill `hark` (`skills/hark/`), sourcing skill `sound-sourcing` (`skills/sound-sourcing/`).

The spec passes 60 KB, so it is split. This index holds every required section; the parts hold the detail.

| part | file | covers |
|---|---|---|
| index | `docs/specs/hark/skills-and-docs.md` | scope, coverage, every required section in summary, data assets, dependencies, open questions |
| 1 | `docs/specs/hark/skills-and-docs-hark.md` | the `hark` tool skill: triggers, surfaces, the 5 loops, LLM critique, numbers, queries, limits, game SFX |
| 2 | `docs/specs/hark/skills-and-docs-workflow.md` | the `sound-sourcing` workflow: steps, roles, every role brief, every template, usage-rights policy, craft rules, troubleshooting |
| 3 | `docs/specs/hark/skills-and-docs-scripts.md` | `analyze.py`, `rights.py`, `rights_audit.py`, `credits.py`, `arrange.py`, `variant.py`, `mux.py` |
| 4 | `docs/specs/hark/skills-and-docs-mixer.md` | the design and cue JSON, `mix.py`, `master.py`, `qa.py` |

## Coverage of the source files of this unit

| source file (role, not name) | spec location |
|---|---|
| tool skill SKILL.md | part 1 |
| sourcing skill SKILL.md | part 2, sections 1, 3; part 3 and 4 interfaces |
| reference: the process | part 2, sections 3, 5 |
| reference: the design format | part 4, sections 1.1-1.5, 2, 4.11 |
| reference: rights (licences) | part 2, section 6; part 3, section 2 |
| reference: sound design rules | part 2, section 7 |
| reference: troubleshooting | part 2, section 8; part 4, section 6 |
| script: analyze | part 3, section 1 |
| script: arrange | part 3, section 5 |
| script: audit (ours `rights_audit.py`) | part 3, section 3 |
| script: credits | part 3, section 4 |
| script: licence policy (ours `rights.py`) | part 3, section 2 |
| script: master | part 4, section 3 |
| script: mix | part 4, sections 1.4-1.5, 2 |
| script: mux (Node in the source; ours `mux.py`) | part 3, section 7 |
| script: qa | part 4, sections 1.7, 4 |
| script: variant | part 3, section 6 |
| templates: arc, audit entry (ours `rights_entry.json`), cues, design fragment, manifest entry, top-level design, sound brief | part 2, section 4 |
| role briefs: 5 critics, designer, fixer, licence audit (ours rights audit), music supervisor, sourcing | part 2, section 5 |

## Interface

Exact interfaces live in the parts. Summary of the names hark uses:

`skills/hark/`:

- `SKILL.md` (under 12288 bytes) and `references/` (detail that does not fit).
- Commands taught (each owned by another unit): `hark doctor`, `measure`, `describe`, `match`, `rank`, `find`,
  `compare`, `ab`, `realness`, `timeline`, `sheet`, `critique`, `llm`, `forge`, `sfx list|render`, `kit
  init|render|check`, `fit`, `bank add-fit`, `mcp`. Python: `from hark import api`.

`skills/sound-sourcing/`:

| path | content |
|---|---|
| `SKILL.md` | the workflow (part 2, sections 1-3), the launch-film sound map (part 2, 7.2), the tool table, the file layout. |
| `references/process.md` | roles, steps, critic lenses, thresholds and their origin, variants, rounds. |
| `references/design-format.md` | every design key (part 4, section 1). |
| `references/sound-design.md` | craft rules (part 2, section 7). |
| `references/rights.md` | usage-rights policy and per-site verification (part 2, section 6). |
| `references/troubleshooting.md` | part 2, section 8 and part 4, section 6. |
| `references/role-*.md` | 10 role briefs: `sourcing`, `rights-audit`, `music`, `designer`, `critic-sync`, `critic-edges`, `critic-masking`, `critic-tonal`, `critic-director`, `fixer` (part 2, section 5). |
| `scripts/analyze.py`, `rights.py`, `rights_audit.py`, `credits.py`, `arrange.py`, `mix.py`, `master.py`, `qa.py`, `variant.py`, `mux.py` | part 3 and part 4. |
| `templates/cues.json`, `SOUND_BRIEF.md`, `manifest_entry.json`, `rights_entry.json`, `sounddesign.json`, `arc.json`, `design_fragment.json` | part 2, section 4. |

Film-project layout the skill uses (paths relative to the film root):

```
audio/SOUND_BRIEF.md                  the contract
audio/assets/<category>/              downloads: <source>-<id>_<slug>.<ext>; never commit or publish them
audio/assets/manifests/<category>.json  one entry per recording
audio/assets/manifests/rights.json    the usage-rights audit (+ rights.md, rights.prev.json)
audio/assets/manifests/music_choice.json  the music decision record
audio/sounddesign.json                top level: include, buses, master
audio/design/<category>.json          a designer's fragment (one each)
out/cues.json                         cue events exported from the picture
out/audio/soundtrack.wav, premaster.wav, stems/, cache/
out/qa/                               mix_report.json, master.json, adds.json, cues_sounded.json, qa.json
out/audio-review/                     working folders of each category and critic: dumps, variants, backups
CREDITS.md, out/CREDITS_description.txt
```

Renames from the source's interface, applied everywhere in this spec: the licence audit is a usage-rights audit
(`rights_audit.py`, `rights.json`); manifest fields `rights` and `rights_url`; audit fields `rights_seen`,
`rights_confirmed`, `rights_in_manifest`, `rights`, `rights_status`; the manifest's listening block is `hark`;
`credits.py --rights`. Machine-read field and flag names are otherwise kept as the parts list them.

## Behaviour

- The tool skill teaches the no-claim-without-a-reading rule, the cost model (CLI vs MCP vs one script), 5 loops
  (choose an asset; review a mix or film; A/B a change; forge a sound in code; render a film kit from the bank), an
  optional consent-gated LLM critique, how much to trust each number, how to write queries, and the limits. Part 1.
- The sourcing skill runs 11 steps (cue list, brief, parallel sourcing, rights audit, music edit, parallel design
  fragments, integrate, QA and listening, 5 parallel read-only critics, 1 fixer, master + credits + mux) with strict
  file ownership per role. Part 2.
- The scripts are deterministic. The same design renders bit-identically, a solo render plays the same sounds as the
  full mix when rules are named, and every QA finding names the placement that caused it. Parts 3 and 4.

## Algorithms

Steps and math are in the parts:

- Mixer DSP: decode cache, sample chain order, RBJ biquads, constant-power pan, resampling pitch, varispeed glide,
  align points, slicing, CRC-hashed variation, loop crossfade, music pre-roll and stretch: part 4, section 2.
- Master chain: high-pass, mono low end, RMS compressor, glue, true-peak lookahead limiter, loudness passes, edge
  fades, BS.1770 meter, LRA, gate, side-chain ducker: part 4, section 3.
- QA: layer rebuild and event grouping, 14 kHz click detector and ownership, edge judging, splice checks, 1/3-octave
  audibility margins, 2-6 kHz harshness, sync tests with offset explanations, density and machine-gun detection,
  loudness gates, coverage classes: part 4, section 4.
- Music: beat-grid fit, transient snap by spectral flux, downbeat phase scoring, bar features, drop candidates,
  dynamic-programming arrangement against an arc: part 3, section 5.
- Rights classification, the audit's checks and merge of agent fields, credit-line completion, format-preserving JSON
  patching, mux duration handling: part 3.

## Constants

Tables: part 4, section 5 (mixer, master, QA thresholds); part 3, sections 1.2 (analyze checks, key profiles), 2.1
(rights table), 5.2 (arrange); part 2, section 7 (launch-film norms) and 6.1 (allowed and forbidden terms). The most
used numbers:

| item | value |
|---|---|
| sample rate | 48000 Hz |
| master | -14 LUFS, ceiling 0.77 linear, gate +-0.5 LU and <= -1.5 dBTP, about -1 dBTP after AAC |
| QA loudness fail | +-0.5 LU, true peak > -1.0 dBTP, tail not silent |
| sync norms | impact -45..+75 ms, apex +-45 ms, riser end -250..+30 ms (checked -300..+60) |
| audibility | read >= 6 clear, 3-6 ok, 0-3 barely, < 0 masked |
| harshness | 2-6 kHz 5 ms RMS > 6 dB over the music's loudest |
| density | about 5 designed effects per 10 s |
| LRA | 5-9 LU target, warn outside 4-10 |
| hark verdicts | z >= 3 strong, 2.5-3 plausible, 2-2.5 weak, < 2 no match; realness clip lo 0.17 / hi 0.77, file lo 0.40 / hi 0.77 |

## Data assets

| asset | format | origin in the source | mark | how hark gets it |
|---|---|---|---|---|
| templates (cues, brief, manifest entry, rights entry, top-level design, arc, design fragment) | JSON, Markdown | hand-written examples with placeholder data | REGENERATE | write fresh from the field tables of part 2, section 4. |
| role briefs (10) | Markdown with placeholders | hand-written prompts | REGENERATE | write fresh from part 2, section 5. |
| Krumhansl-Kessler key profiles | 2 x 12 numbers | published music-cognition data | DERIVE | the table in part 3, 1.2. |
| BS.1770 K-weighting coefficients at 48 kHz, gating constants | 2 biquads, -70 / -10 / -0.691 | ITU-R BS.1770 | DERIVE | from the standard (or pyloudnorm). |
| RBJ biquad formulas | formulas | the Audio EQ Cookbook | DERIVE | public. |
| rights policy table | patterns, names, URLs | licence texts of CC, Mixkit, Pixabay | DERIVE | part 3, 2.1 and part 2, 6.1. |
| launch-film norms quoted by the craft rules (5 effects per 10 s, 85 % of cuts silent, click levels, sync window, LRA) | numbers | measured by a sister project on professional launch films | DERIVE | used as stated constants; the launch profile itself belongs to the `timeline` unit. |
| the bit-identical golden render (60 s film, 83 recordings, 264 placements, a fixed SHA-1) | WAV hash | a private shipped film and its licensed recordings | NEEDS-DATA | fallback: a golden render of the template design over generated test fixtures, hashed in the test suite; DSP changes bump a version and the hash. |
| "59 % of licensed real recordings have edit or fade padding" | a statistic | the source's private licensed set | NEEDS-DATA | fallback: state the qualitative rule (zeros prove nothing alone) without the number, or measure our own CC0 set when one exists. |
| query evaluation: job words vs sound words (9 pops; 195 effects, 152 -> 176 passes) | statistics | the source's private evaluation | NEEDS-DATA | fallback: keep the rule, drop the numbers, or re-measure with the `listen` unit's evaluation. |
| forge first test (independent judge, 8-13 dB off) | an anecdote | the source's demo | NEEDS-DATA | fallback: keep the Goodhart warnings; report hark's own forge test once one is run. |
| downbeat accuracy (2 hand-labelled tracks: 0.93 correct, 0.15 half a bar off) | 2 cases | the source's private music | NEEDS-DATA | fallback: test on generated click tracks with known downbeats (part 3, S11); keep the LOW-confidence rule. |
| the reference-mix QA findings (7 clicks, 5 of 153 masked, ...) | statistics | the private shipped film | NEEDS-DATA | fallback: describe as the expected scale, without claiming hark measured it. |
| bank family names, kit presets, fitted CC0 sounds | bank data | the source's bank | see `bank-kit-sfx` and `fit` specs | not owned here. |

## Errors and edge cases

Detailed per script: part 3 (each section) and part 4, section 6. Cross-cutting rules:

- Every script exits 0 on success; QA and audits exit 1 on a fail or blocker and 2 on missing inputs; the mixer and
  master exit 1 on a failed gate.
- No partial writes on a bad operation (`variant.py`), on a missing recording (`mix.py`), or on an existing backup.
- A downloaded file is never executed or unpacked. Fetches are sequential, polite and in memory. Scripts run with
  `python -I`.
- Source quirks hark must not copy: the analyzer's mislabelled decimation for files over 400 s (part 3, 1.2); an
  ambience window shorter than the 1.5 s crossfade (part 4, section 6); `classify` reading the bare word "zero" as
  CC0 (part 3, 2.1).

## Acceptance tests

Full lists: part 1 section 12 (H1-H6), part 2 section 9 (W1-W6), part 3 section 8 (S1-S17), part 4 section 7
(M1-M11, Q1-Q10). A core set of 12 that ticket 22 must pass first:

| # | input | expected properties |
|---|---|---|
| A1 | `mix.py` twice on one design | bit-identical outputs (M2). |
| A2 | full mix vs `--only <fragment>` with named rules | identical placements for that fragment (M3). |
| A3 | 2 fragments defining one name | error naming fragment and name; nothing written (M4). |
| A4 | `master.py` on a -30 LUFS test signal | -14 +-0.5 LUFS, <= -1.5 dBTP, last 480 samples zero, exit 0 (M10). |
| A5 | an sfx placement starting on a non-zero sample, no fade | QA click at an edge in the master; status fail; exit 1 (Q1). |
| A6 | an effect 10 dB under the bed | audibility `masked`, covered by music/amb (Q3). |
| A7 | a rule offset of 0.2 s on an impact | sync lists it as deliberate with that rule (Q4). |
| A8 | `classify` on the 11 texts of S1 | the keys and statuses of S1. |
| A9 | an in-scope CC BY entry with no page reading | rights audit blocker; exit 1; `rights.json` written (S2). |
| A10 | credits for a CC BY effect with a link-less requested form | the line gains the page link and the licence link (S7). |
| A11 | `arrange.py` on a generated 120 BPM click track with accented downbeats | bpm 120 +-0.05, the accented phase, `stretch: none` (S11). |
| A12 | `variant.py --out` with 1 rule `set` on a hand-aligned fragment | a 1-line diff; the real design untouched (S13). |

## Dependencies on other units

| unit | what this unit needs from it |
|---|---|
| core-io | the CLI shell and conventions (summary first, `--json`, `--out`, one-line errors, exit 1), `--threads` and `HARK_THREADS`, `doctor`, the optional ML install prompt, `measure` (LUFS, dBTP, LRA, floor, bandwidth, bands; reads mp4 audio). |
| listen | `describe`, `match` (`--alts`, z, verdicts, `p_vs_alts`), `rank`, `find` (`--kind`, segment kinds, head/tail, cut marks), `compare --refs` (typicality z), `ab` (trims, `--query`, `--refs`), `realness` (`--start --end`, length-dependent thresholds, evidence list); `api.realness` returning `p_real` and `verdict` for QA `--realness`. |
| timeline | `timeline` (`--cues`, `--video`, `--detail`, sync offset and chance, `ends` events aligned by end), `sheet` (`-o`, `--from --to`, cue colours), `critique` (`--fast`, verdict, score, actions, metrics with status low/high, the 7-film launch profile); `api.timeline` returning `summary` and `text`, `api.sheet(path, png, video=, cues=, tl=)`, `api.critique(path, video=, use_ast=, use_realness=)`. |
| forge-core | `forge` and its flags (`--refs --budget --reps --tone --keep --robust --dir`), the recipe contract, the DSP kit, `report.json` (tone words, robust, alt_zs, reference scores). |
| recipes-impacts | the impact and UI recipes a forge loop starts from. |
| recipes-motion | the motion, riser, glitch and notify recipes. |
| bank-kit-sfx | `sfx list` (`--kind`, `--category`), `sfx render` (`--pitch --dur --apex`), `kit init --preset`, `kit render`, `kit check`, `kit.lock.json`, the kit helpers (render, bed level, gain, place, spacing), the family names. |
| fit | `fit --takes`, `bank add-fit`, the 15 fitted CC0 sounds and their source credits. |
| surfaces | the MCP server and its tool names, `full=true`, `--warm`; the LLM critique through `agy -p` with its consent flag, mono 64 kbps upload, `--with-timeline`, `--prompt`. |
| skills-and-docs | this unit. |

## Open questions

| id | question | recommendation |
|---|---|---|
| Q1 | Does `agy -p` hear audio (probe ticket 11)? Part 1 section 9 depends on it. | If not, the skill drops the LLM section and says the second opinion is unavailable. |
| Q2 | Keep the source's script file names (`analyze.py`, `mix.py`, `qa.py`, ...) or rename? They are generic words, and the policy and audit scripts are already renamed. | Keep the generic names. They read clearly and the similarity gate measures text, not names. |
| Q3 | Who writes hark's long docs that the source's skills point to (the launch-film profile write-up, an integration guide, the bank's "add your own fit" guide, the validation write-up)? None is a source file of this unit. | The owning units (`timeline`, `bank-kit-sfx`, `fit`, `listen`) put that content into their own `references/` files; the skill links to them. |
| Q4 | The source's mux script is Node. Ticket 22 asks for Python 3.11+ scripts. | Python `mux.py` with the behaviour of part 3, section 7. |
| Q5 | The source's mixer converts mono to stereo with ffmpeg's own conversion (3 dB down per channel), while its analyzer duplicates at unity. Keep the difference? | Use unity duplication in both, and fix the golden hash to that. `norm_peak_db` hides the difference in most designs. |
| Q6 | The game SFX workflow is new (part 1, section 11). Loudness target and loop-point rules for game assets are not in the source. | Ticket 22 sets them with the `godot-sound-design` skill; a per-asset peak or loudness normalisation instead of -14 LUFS integrated. |
| Q7 | The `--self-test` that ticket 22 adds to each script is not in the source. | Each self-test runs that script's acceptance tests from parts 3 and 4 on generated fixtures, with no network. |
