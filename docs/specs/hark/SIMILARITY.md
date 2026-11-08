# hark clean-room similarity gate

Ticket 26b (T-39). Measured 2026-11-08 on branch `fleet/solo-w68-hark` at the base of this task.

## Result

**212 of 389 files fail.** 177 files pass.

| Group | Files | Fail | Main cause |
|---|---:|---:|---|
| Engine Python (`skills/hark/engine/hark/**.py`) | 162 | 71 | token similarity over 30 % |
| Test Python (`skills/hark/engine/tests/**.py`) | 85 | 13 | token similarity over 30 % (stand-in DSP helpers) |
| JSON data (seeds, banks, kits, vocab, templates) | 41 | 33 | identical value runs and token similarity |
| Other code (`pyproject.toml`, `scripts/hark`) | 2 | 1 | dependency list run |
| Prose (`*.md`) | 99 | 94 | token similarity over 20 % |

Failure kinds:

- F1: 18 files have 5 or more identical consecutive non-trivial lines. 12 are JSON data files. 6 are code files: `forge/engine/dsp.py`, `forge/engine/eval/blind_trim.py`, `fit/analysis/modal.py`, `tests/recipes_impacts/forge_standins.py`, `tests/recipes_ui/conftest.py` and `pyproject.toml`.
- F2: 113 code files are over 30 % token similarity. 35 Python files are over 40 %, and 13 are over 50 %.
- F3: 94 prose files are over 20 % token similarity. No prose file shares a run of more than 2 identical lines, and the highest prose line similarity is 13.0 %. No source sentence was copied. The token overlap is shared vocabulary.
- F4: No file in scope contains the source's project or skill names. The only text hits are the English word "real-sounding".

## Method

- Source: the upstream repository at commit `4ac3d2ac2b151e1c23376f9ce7f3047a17050013`, cloned read-only to `~/.local/state/hark-cleanroom/src-gate`. No source code was run.
- Scope: every file under `skills/hark/`, `skills/sound-sourcing/` and `docs/specs/hark/`, without `__pycache__`. That is 389 files: 247 `.py`, 99 `.md`, 41 `.json`, 1 `.toml` and the launcher `skills/hark/scripts/hark`.
- Pairing: each file is compared with every source file of the same class. Code (`.py`, `.json`, `.toml`, no extension) pairs with source code and data. Prose (`.md`) pairs with source markdown. The role file in the tables is the source file with the highest token similarity, for example `forge/engine/dsp.py` with the source DSP module. 132 of 389 rows match a source file of the same basename. The others match a source file of the same role under a different name, or the nearest file when no counterpart exists. The best match is an upper bound for any hand-picked role pair, so the gate is at least as strict as role pairing.
- Token % is the multiset Jaccard of tokens, as in `similarity.py`. Code tokens are identifiers, with strings, comments and docstrings removed. JSON tokens are keys, words and numbers. Prose tokens are lower-case words.
- Line % is the `difflib` ratio of whitespace-normalised non-empty lines, the best of the 8 best token candidates.
- Max run is the longest block of consecutive identical normalised lines shared with any source file of any class. It counts only non-trivial lines: 12 or more characters, at least 1 letter, not an import line.
- Thresholds (ticket step 4): a code file fails over 30 % token similarity or with a run of 5 or more. A prose file fails over 20 % token similarity. This report applies the run rule to prose too, but no prose file reaches it.
- Source paths are written with neutral prefixes: `src-pkg/` is the source Python package, `src-skills/tool/` and `src-skills/sourcing/` are its 2 skills, and `src-docs/`, `src-eval/`, `src-tests/` and `src-root/` are the other source folders. No source text is in this file.
- The measuring scripts are scratch tools outside the repo: `~/.local/state/hark-cleanroom/gate-tools/measure.py` and `report.py`. The raw data is `~/.local/state/hark-cleanroom/gate-out/all.json`.

### unlicense `similarity.py` runs

The ticket's 4 runs of `~/.claude/skills/unlicense/scripts/similarity.py`, with tests included (`--exclude __none__`):

| Target | Source folder | Files read | Lines | Line-weighted similar | Verbatim lines |
|---|---|---:|---:|---:|---:|
| `skills/hark/engine/` (`.py`) | package | 195 | 50044 | 17.7 % | 5776 (11.5 %) |
| `skills/hark/` (`.py,.md,.json`) | skills | 238 | 63666 | 8.6 % | 3603 (5.7 %) |
| `skills/sound-sourcing/` (`.py,.md,.json`) | skills | 35 | 6255 | 21.4 % | 599 (9.6 %) |
| `docs/specs/hark/` (`.md`) | docs | 73 | 25345 | 13.3 % | 613 (2.4 %) |

`similarity.py` keys files by basename. The engine has 247 Python files, but it reads only 195, because files such as `__init__.py` overwrite each other. Its verbatim count also counts generic lines such as `return x`. For these 2 reasons, the per-file verdicts in this report come from `measure.py`, which keys by relative path and counts only non-trivial lines.

### Calibration control

The same tool measured unrelated repo files against the source: 148 Python files and 156 markdown files from other skills and `docs/specs/`.

| Class | Files (20+ lines) | Median token % | 90th percentile | Max | Over threshold |
|---|---:|---:|---:|---:|---:|
| Code | 139 | 17.0 | 22.3 | 24.1 | 0 |
| Prose | 149 | 13.3 | 19.5 | 23.6 | 11 (7 %) |

The 30 % code threshold separates unrelated code from related code. The 20 % prose threshold flags 7 % of unrelated prose. The hark prose shares the domain vocabulary and the public interface names (commands, flags, JSON fields, which the clean-room rules allow), so its median is 27.7 %. Of the 94 prose failures, 62 are over 25 %, 34 are over 30 % and 19 are over 40 %.

## Notes for the rewrite

- N1: The JSON failures are data assets. Their runs are parameter values that match the source values line for line (for example `forge/seeds/risers/sting.json` lines 1-38). The rules allow constants as facts, but a spec must not paste the data. A decision is needed: regenerate these assets with our tools, or accept them as facts.
- N2: The 6 code runs are short formula and constant blocks. The `pyproject.toml` run is the dependency list. The test runs are stand-in copies of small engine helpers. The `dsp.py`, `modal.py` and `blind_trim.py` runs keep the source expression and names, and need a rewrite.
- N3: 79 of the 113 F2 code failures share no block of 3 or more identical lines. Their token overlap comes from shared identifiers: public parameter names, constants and local names that the specs carried over. A rewrite renames non-public identifiers and changes structure.
- N4: The F3 prose failures copy no sentences. A rewrite for token reasons alone changes wording but keeps the interface names. The orchestrator can decide to keep the 20 % threshold, or to judge prose by line similarity and runs, where every prose file passes.

## Failures

### Code failures: 5 or more identical consecutive non-trivial lines

| Our file | Max run | Our lines | Source role file | Token % |
|---|---:|---|---|---:|
| `skills/hark/engine/hark/forge/seeds/risers/sting.json` | 35 | 1-38 | `src-pkg/forge/seeds/sting.json` | 27.6 |
| `skills/hark/engine/hark/forge/seeds/risers/notify.json` | 31 | 1-36 | `src-pkg/forge/seeds/notify.json` | 36.1 |
| `skills/hark/engine/hark/forge/seeds/ui/pop.json` | 23 | 579-605 | `src-pkg/forge/seeds/pop.json` | 36.4 |
| `skills/hark/engine/hark/forge/seeds/risers/riser.json` | 19 | 4-25 | `src-pkg/forge/seeds/riser.json` | 37.3 |
| `skills/hark/engine/hark/forge/seeds/risers/glitch.json` | 18 | 65-83 | `src-pkg/forge/seeds/glitch.json` | 32.3 |
| `skills/hark/engine/hark/forge/seeds/ui/keypress.json` | 14 | 9-23 | `src-pkg/forge/seeds/keypress.json` | 29.5 |
| `skills/sound-sourcing/templates/sounddesign.json` | 9 | 4-12 | `src-skills/sourcing/templates/sounddesign.json` | 48.0 |
| `skills/hark/engine/hark/forge/seeds/impacts/impact.json` | 8 | 501-508 | `src-pkg/forge/seeds/impact.json` | 52.2 |
| `skills/hark/engine/hark/data/timeline/launch_profile.json` | 7 | 1-11 | `src-pkg/data/launch_profile.json` | 32.2 |
| `skills/hark/engine/hark/forge/engine/dsp.py` | 6 | 517-523 | `src-pkg/forge/dsp.py` | 58.4 |
| `skills/hark/engine/hark/forge/engine/eval/blind_trim.py` | 6 | 21-26 | `src-eval/forge/blind_v2.py` | 30.1 |
| `skills/hark/engine/pyproject.toml` | 6 | 10-15 | `src-root/pyproject.toml` | 50.0 |
| `skills/hark/engine/tests/recipes_impacts/forge_standins.py` | 6 | 30-39 | `src-pkg/forge/dsp.py` | 53.9 |
| `skills/hark/engine/tests/recipes_ui/conftest.py` | 6 | 48-55 | `src-pkg/forge/dsp.py` | 52.2 |
| `skills/hark/engine/hark/fit/analysis/modal.py` | 5 | 224-229 | `src-pkg/forge/fit/modal.py` | 43.9 |
| `skills/hark/engine/hark/forge/seeds/ui/click.json` | 5 | 145-150 | `src-pkg/forge/seeds/click.json` | 23.6 |
| `skills/hark/engine/hark/forge/seeds/ui/popin.json` | 5 | 28-33 | `src-pkg/forge/seeds/popin.json` | 25.6 |
| `skills/hark/engine/hark/forge/seeds/ui/toggle.json` | 5 | 203-207 | `src-pkg/forge/seeds/toggle.json` | 24.9 |

### Code failures: token similarity over 30 %

Shared blocks list our line ranges with 3 or more consecutive non-trivial lines that are identical to source lines. The count of non-trivial lines is in brackets.

| Our file | Token % | Line % | Verbatim lines | Shared blocks (our lines) | Source role file |
|---|---:|---:|---:|---|---|
| `skills/hark/engine/hark/forge/bank/data/pluck.json` | 90.0 | 50.0 | 2 / 7 | none | `src-pkg/forge/bank/pluck.json` |
| `skills/hark/engine/hark/forge/bank/data/forge04.json` | 78.0 | 7.9 | 2 / 59 | none | `src-pkg/forge/bank/forge04.json` |
| `skills/hark/engine/hark/__main__.py` | 75.0 | 50.0 | 1 / 3 | none | `src-pkg/__main__.py` |
| `skills/sound-sourcing/templates/arc.json` | 67.5 | 71.4 | 3 / 5 | 3-5 (3) | `src-skills/sourcing/templates/arc.json` |
| `skills/hark/engine/hark/forge/kits/launch.json` | 67.4 | 6.4 | 6 / 163 | none | `src-pkg/forge/kits/launch.json` |
| `skills/hark/engine/hark/forge/bank/data/forge_v2.json` | 63.8 | 3.5 | 2 / 220 | none | `src-pkg/forge/bank/forge_v2.json` |
| `skills/hark/engine/hark/forge/kits/minimal.json` | 60.8 | 11.7 | 9 / 90 | none | `src-pkg/forge/kits/minimal.json` |
| `skills/hark/engine/hark/forge/engine/dsp.py` | 58.4 | 17.7 | 74 / 474 | 262-264 (3), 341-344 (3), 446-449 (3), 465-467 (3), 517-523 (6), 672-676 (5) | `src-pkg/forge/dsp.py` |
| `skills/hark/engine/hark/forge/engine/eval/frozen_tone.py` | 57.4 | 11.6 | 14 / 124 | 59-63 (3) | `src-eval/forge/frozen_tone.py` |
| `skills/hark/engine/hark/forge/engine/tone.py` | 57.2 | 8.1 | 34 / 325 | 23-25 (3), 425-427 (3) | `src-pkg/forge/tone.py` |
| `skills/hark/engine/hark/forge/recipes/tonal.py` | 54.5 | 11.8 | 19 / 197 | none | `src-pkg/forge/recipes/_tonal.py` |
| `skills/hark/engine/tests/recipes_impacts/forge_standins.py` | 53.9 | 12.9 | 85 / 348 | 30-39 (6), 97-103 (5), 199-207 (5) | `src-pkg/forge/dsp.py` |
| `skills/hark/engine/hark/data/listen/vocab.json` | 53.6 | 15.0 | 19 / 332 | none | `src-pkg/data/vocab.json` |
| `skills/hark/engine/tests/recipes_risers/standin_engine.py` | 52.7 | 11.4 | 68 / 306 | 20-24 (3), 159-162 (3), 201-204 (3) | `src-pkg/forge/dsp.py` |
| `skills/hark/engine/hark/forge/engine/viz.py` | 52.4 | 11.3 | 14 / 105 | none | `src-pkg/forge/viz.py` |
| `skills/hark/engine/hark/forge/engine/schema.py` | 52.3 | 12.2 | 31 / 281 | none | `src-pkg/forge/schema.py` |
| `skills/hark/engine/hark/forge/seeds/impacts/impact.json` | 52.2 | 20.0 | 500 / 796 | 22-28 (6), 30-32 (3), 36-42 (6), 44-46 (3), 47-49 (3), 51-53 (3), 61-63 (3), 76-78 (3), 102-108 (7), 110-112 (3), 116-122 (6), 124-126 (3), 127-129 (3), 131-133 (3), 141-143 (3), 156-158 (3), 182-188 (7), 190-192 (3), 196-202 (6), 204-206 (3), 207-209 (3), 211-213 (3), 221-223 (3), 236-238 (3), 262-268 (6), 270-272 (3), 276-281 (6), 284-286 (3), 287-289 (3), 291-293 (3), 301-303 (3), 316-318 (3), 342-348 (6), 350-352 (3), 356-362 (6), 364-366 (3), 367-369 (3), 371-373 (3), 381-383 (3), 396-398 (3), 422-428 (6), 430-432 (3), 436-442 (6), 444-446 (3), 447-449 (3), 451-453 (3), 461-463 (3), 476-478 (3), 501-508 (8), 514-519 (6), 524-526 (3), 527-529 (3), 541-543 (3), 556-558 (3), 581-588 (8), 594-599 (6), 604-606 (3), 607-609 (3), 621-623 (3), 636-638 (3), 661-668 (8), 674-679 (6), 684-686 (3), 687-689 (3), 701-703 (3), 716-718 (3), 741-748 (8), 754-759 (6), 764-766 (3), 767-769 (3), 781-783 (3), 796-798 (3), 803-808 (3), 821-828 (8), 836-839 (4), 844-846 (3), 847-849 (3), 861-863 (3), 876-878 (3) | `src-pkg/forge/seeds/impact.json` |
| `skills/hark/engine/tests/recipes_ui/conftest.py` | 52.2 | 14.0 | 105 / 376 | 48-55 (6), 352-356 (3) | `src-pkg/forge/dsp.py` |
| `skills/hark/engine/hark/forge/engine/search.py` | 52.1 | 7.9 | 50 / 759 | none | `src-pkg/forge/search.py` |
| `skills/hark/engine/hark/fit/analysis/sinusoidal.py` | 51.3 | 2.4 | 52 / 1006 | none | `src-pkg/forge/fit/sines.py` |
| `skills/hark/engine/hark/forge/kits/calm.json` | 50.5 | 12.2 | 8 / 92 | 70-72 (3) | `src-pkg/forge/kits/calm.json` |
| `skills/hark/engine/hark/forge/engine/fx.py` | 50.3 | 7.2 | 67 / 729 | 725-727 (3), 909-911 (3) | `src-pkg/forge/fx.py` |
| `skills/hark/engine/pyproject.toml` | 50.0 | 40.0 | 18 / 33 | 10-15 (6) | `src-root/pyproject.toml` |
| `skills/hark/engine/hark/forge/engine/polish.py` | 49.5 | 8.2 | 56 / 637 | 587-589 (3) | `src-pkg/forge/polish.py` |
| `skills/hark/engine/hark/fit/pipeline/quality.py` | 48.7 | 8.2 | 35 / 362 | 98-100 (3), 223-225 (3) | `src-pkg/forge/fit/quality.py` |
| `skills/hark/engine/hark/forge/recipes/risers/sting.py` | 48.5 | 3.7 | 44 / 679 | 342-344 (3) | `src-pkg/forge/recipes/sting.py` |
| `skills/hark/engine/hark/models/hub.py` | 48.1 | 10.7 | 20 / 147 | none | `src-pkg/models/hub.py` |
| `skills/sound-sourcing/templates/sounddesign.json` | 48.0 | 68.2 | 11 / 17 | 4-13 (9) | `src-skills/sourcing/templates/sounddesign.json` |
| `skills/hark/engine/hark/forge/recipes/ui/keypress.py` | 47.5 | 5.2 | 17 / 294 | none | `src-pkg/forge/recipes/keypress.py` |
| `skills/sound-sourcing/scripts/mix.py` | 47.0 | 3.1 | 31 / 842 | none | `src-skills/sourcing/scripts/mix.py` |
| `skills/hark/engine/hark/forge/kits/crisp.json` | 46.9 | 11.7 | 10 / 90 | none | `src-pkg/forge/kits/crisp.json` |
| `skills/hark/engine/hark/fit/analysis/spectral.py` | 46.6 | 2.1 | 46 / 1086 | none | `src-pkg/forge/fit/spectral.py` |
| `skills/hark/engine/hark/forge/recipes/ui/typing.py` | 46.5 | 7.9 | 26 / 249 | none | `src-pkg/forge/recipes/typing.py` |
| `skills/hark/engine/hark/fit/analysis/gesture.py` | 46.2 | 6.5 | 10 / 250 | none | `src-pkg/forge/fit/gesture.py` |
| `skills/hark/engine/hark/fit/analysis/space.py` | 45.5 | 6.7 | 22 / 350 | none | `src-pkg/forge/fit/space.py` |
| `skills/hark/engine/hark/models/ast.py` | 45.5 | 9.2 | 4 / 84 | none | `src-pkg/models/ast.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/pluck.py` | 45.4 | 6.8 | 19 / 182 | none | `src-pkg/forge/recipes/pluck.py` |
| `skills/hark/engine/hark/forge/engine/critic.py` | 45.0 | 2.8 | 33 / 451 | none | `src-pkg/forge/critic.py` |
| `skills/hark/engine/hark/listen/eval/kind_queries.json` | 44.3 | 7.6 | 5 / 110 | none | `src-eval/semantics/kind_queries.json` |
| `skills/hark/engine/hark/forge/recipes/motion/shimmer.py` | 44.2 | 7.0 | 22 / 359 | none | `src-pkg/forge/recipes/shimmer.py` |
| `skills/hark/engine/hark/fit/analysis/modal.py` | 43.9 | 3.7 | 43 / 709 | 224-229 (5) | `src-pkg/forge/fit/modal.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/impact.py` | 43.4 | 4.7 | 59 / 901 | none | `src-pkg/forge/recipes/impact.py` |
| `skills/sound-sourcing/templates/design_fragment.json` | 43.1 | 34.6 | 9 / 45 | 3-6 (3), 55-57 (3) | `src-skills/sourcing/templates/design_fragment.json` |
| `skills/hark/engine/hark/forge/seeds/impacts/subdrop.json` | 42.9 | 38.5 | 163 / 229 | 8-11 (4), 19-23 (3), 26-28 (3), 38-41 (4), 49-53 (3), 56-58 (3), 68-71 (4), 79-83 (3), 86-88 (3), 98-101 (4), 109-113 (4), 116-118 (3), 128-131 (4), 139-143 (4), 146-148 (3), 158-161 (4), 169-173 (4), 176-178 (3), 188-191 (4), 199-203 (4), 206-208 (3), 218-221 (4), 229-233 (4), 236-238 (3), 248-251 (4), 259-263 (4), 266-268 (3), 278-281 (4), 289-293 (4), 296-298 (3) | `src-pkg/forge/seeds/subdrop.json` |
| `skills/sound-sourcing/scripts/master.py` | 42.9 | 5.0 | 20 / 301 | 35-37 (3) | `src-skills/sourcing/scripts/master.py` |
| `skills/hark/engine/hark/timeline/video.py` | 42.8 | 11.2 | 9 / 117 | none | `src-pkg/video.py` |
| `skills/sound-sourcing/scripts/qa.py` | 42.8 | 1.9 | 48 / 1029 | none | `src-skills/sourcing/scripts/qa.py` |
| `skills/hark/engine/hark/forge/recipes/risers/notify.py` | 42.1 | 5.6 | 33 / 473 | 557-560 (3) | `src-pkg/forge/recipes/notify.py` |
| `skills/hark/engine/hark/forge/bank/kit.py` | 41.3 | 4.3 | 27 / 702 | none | `src-pkg/forge/kit.py` |
| `skills/hark/engine/hark/timeline/sheet.py` | 41.3 | 3.8 | 23 / 410 | 162-165 (3) | `src-pkg/sheet.py` |
| `skills/hark/engine/hark/listen/vocabulary.py` | 39.8 | 5.5 | 20 / 482 | 152-155 (3) | `src-pkg/vocab.py` |
| `skills/sound-sourcing/scripts/arrange.py` | 39.8 | 4.8 | 27 / 450 | none | `src-skills/sourcing/scripts/arrange.py` |
| `skills/sound-sourcing/scripts/rights_audit.py` | 39.7 | 2.0 | 20 / 564 | none | `src-skills/sourcing/scripts/audit.py` |
| `skills/sound-sourcing/templates/cues.json` | 39.7 | 25.9 | 1 / 19 | none | `src-skills/sourcing/templates/cues.json` |
| `skills/hark/engine/hark/forge/recipes/ui/pop.py` | 39.5 | 3.1 | 24 / 590 | none | `src-pkg/forge/recipes/pop.py` |
| `skills/hark/engine/hark/forge/recipes/risers/darkriser.py` | 39.3 | 5.8 | 7 / 95 | none | `src-pkg/forge/recipes/darkriser.py` |
| `skills/hark/engine/hark/forge/seeds/ui/typing.json` | 39.3 | 15.0 | 134 / 398 | 25-28 (3), 31-33 (3), 60-63 (3), 66-68 (3), 95-98 (3), 101-103 (3), 130-133 (3), 136-138 (3), 165-168 (3), 171-173 (3), 200-203 (3), 206-208 (3), 235-238 (4), 250-252 (3), 279-282 (4), 294-296 (3), 323-326 (4), 338-340 (3), 367-370 (4), 382-384 (3), 411-414 (4), 426-428 (3), 455-458 (4), 470-472 (3) | `src-pkg/forge/seeds/typing.json` |
| `skills/hark/engine/hark/forge/engine/loop.py` | 39.1 | 3.1 | 28 / 538 | none | `src-pkg/forge/__init__.py` |
| `skills/hark/engine/hark/models/clap.py` | 39.1 | 5.8 | 7 / 130 | none | `src-pkg/models/clap.py` |
| `skills/hark/engine/hark/forge/seeds/risers/downlifter.json` | 38.8 | 25.9 | 106 / 206 | 38-40 (3), 80-82 (3), 122-124 (3), 164-166 (3), 183-185 (3), 206-208 (3), 248-250 (3) | `src-pkg/forge/seeds/downlifter.json` |
| `skills/hark/engine/hark/listen/match.py` | 38.7 | 4.4 | 4 / 110 | none | `src-pkg/match.py` |
| `skills/sound-sourcing/templates/manifest_entry.json` | 38.7 | 16.1 | 2 / 17 | none | `src-skills/sourcing/templates/manifest_entry.json` |
| `skills/hark/engine/hark/listen/search.py` | 38.5 | 3.6 | 26 / 514 | none | `src-pkg/search.py` |
| `skills/hark/engine/hark/listen/eval/queries.json` | 38.4 | 18.0 | 30 / 261 | none | `src-eval/semantics/queries.json` |
| `skills/hark/engine/hark/listen/profile.py` | 38.4 | 4.3 | 25 / 543 | none | `src-pkg/profile.py` |
| `skills/hark/engine/hark/forge/seeds/motion/shimmer.json` | 37.7 | 10.7 | 470 / 586 | none | `src-pkg/forge/seeds/shimmer.json` |
| `skills/hark/engine/hark/forge/seeds/risers/riser.json` | 37.3 | 23.9 | 416 / 486 | 4-25 (19), 28-34 (7), 36-41 (6), 50-52 (3), 57-67 (7), 67-69 (3), 69-79 (11), 83-88 (6), 90-95 (5), 112-114 (3), 119-124 (3), 126-131 (5), 134-136 (3), 138-141 (4), 152-157 (5), 174-176 (3), 188-194 (7), 199-201 (3), 206-212 (7), 214-216 (3), 228-230 (3), 290-292 (3), 297-319 (19), 322-328 (7), 330-335 (5), 344-346 (3), 351-373 (19), 376-382 (7), 384-389 (5), 398-400 (3), 405-427 (19), 430-436 (7), 438-443 (5), 452-454 (3), 459-481 (19), 484-490 (7), 492-497 (5), 506-508 (3), 513-535 (19), 538-544 (7), 546-551 (5), 560-562 (3) | `src-pkg/forge/seeds/riser.json` |
| `skills/hark/engine/hark/forge/recipes/risers/revcymbal.py` | 37.2 | 5.4 | 7 / 114 | none | `src-pkg/forge/recipes/revcymbal.py` |
| `skills/hark/engine/hark/forge/recipes/ui/_layers.py` | 37.2 | 3.2 | 11 / 195 | none | `src-pkg/forge/recipes/click.py` |
| `skills/sound-sourcing/scripts/credits.py` | 37.2 | 4.7 | 11 / 278 | none | `src-skills/sourcing/scripts/credits.py` |
| `skills/hark/engine/hark/fit/pipeline/codegen.py` | 37.1 | 7.6 | 15 / 226 | 377-380 (3) | `src-pkg/forge/fit/codegen.py` |
| `skills/hark/engine/tests/timeline/test_timeline_analysis.py` | 37.1 | 3.8 | 9 / 216 | none | `src-tests/test_timeline.py` |
| `skills/hark/engine/hark/forge/engine/eval/scrub_paths.py` | 37.0 | 11.0 | 6 / 85 | none | `src-eval/forge/scrub_paths.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/cuthit.py` | 37.0 | 6.9 | 15 / 202 | none | `src-pkg/forge/recipes/cuthit.py` |
| `skills/hark/engine/hark/fit/pipeline/cli.py` | 36.6 | 2.9 | 6 / 228 | none | `src-pkg/forge/fit/__init__.py` |
| `skills/hark/engine/hark/listen/realness.py` | 36.5 | 3.8 | 26 / 606 | none | `src-pkg/realness.py` |
| `skills/hark/engine/hark/forge/bank/sfx.py` | 36.4 | 3.3 | 10 / 366 | none | `src-pkg/forge/sfx.py` |
| `skills/hark/engine/hark/forge/recipes/risers/glitch.py` | 36.4 | 2.4 | 15 / 377 | none | `src-pkg/forge/recipes/glitch.py` |
| `skills/hark/engine/hark/forge/seeds/ui/pop.json` | 36.4 | 41.9 | 819 / 941 | 1-10 (7), 15-21 (7), 23-38 (14), 40-44 (5), 48-51 (4), 53-62 (10), 65-67 (3), 74-83 (10), 85-89 (4), 92-94 (3), 96-106 (7), 111-117 (7), 119-134 (14), 136-140 (5), 144-147 (4), 149-158 (10), 161-166 (6), 170-185 (15), 188-190 (3), 195-202 (5), 207-213 (7), 215-230 (14), 232-236 (5), 240-243 (4), 245-262 (18), 266-275 (10), 277-281 (4), 284-286 (3), 291-298 (5), 303-309 (7), 311-326 (14), 328-332 (5), 336-339 (4), 341-350 (10), 353-358 (6), 362-372 (11), 374-377 (3), 380-382 (3), 387-394 (5), 399-405 (7), 407-422 (14), 424-428 (5), 432-435 (4), 437-446 (10), 449-454 (6), 458-473 (15), 476-478 (3), 483-490 (5), 495-501 (7), 503-518 (14), 520-524 (5), 528-531 (4), 533-542 (10), 547-550 (4), 554-569 (15), 572-574 (3), 579-605 (23), 611-614 (3), 625-642 (18), 644-646 (3), 650-652 (3), 655-665 (10), 668-670 (3), 674-701 (23), 721-738 (18), 740-742 (3), 746-748 (3), 751-761 (10), 764-766 (3), 771-797 (23), 817-834 (18), 836-838 (3), 842-844 (3), 847-857 (10), 860-862 (3), 867-893 (23), 912-930 (19), 932-934 (3), 938-940 (3), 943-953 (10), 956-958 (3), 963-989 (23), 994-998 (4), 1009-1026 (18), 1028-1030 (3), 1034-1036 (3), 1039-1049 (10), 1052-1054 (3) | `src-pkg/forge/seeds/pop.json` |
| `skills/hark/engine/hark/listen/compare.py` | 36.2 | 3.2 | 11 / 330 | none | `src-pkg/compare.py` |
| `skills/hark/engine/hark/forge/seeds/risers/notify.json` | 36.1 | 45.7 | 580 / 778 | 1-36 (31), 38-47 (10), 48-50 (3), 74-76 (3), 78-114 (31), 116-125 (10), 126-128 (3), 152-154 (3), 156-192 (31), 194-203 (10), 204-206 (3), 230-232 (3), 234-270 (31), 272-281 (10), 282-284 (3), 308-310 (3), 312-348 (31), 350-359 (10), 360-362 (3), 386-388 (3), 390-396 (3), 393-426 (30), 428-437 (10), 438-440 (3), 464-466 (3), 470-504 (30), 506-515 (10), 516-518 (3), 542-544 (3), 549-582 (30), 584-593 (10), 594-596 (3), 620-622 (3), 627-660 (30), 662-671 (10), 672-674 (3), 698-700 (3), 705-738 (30), 740-749 (10), 750-752 (3), 776-778 (3), 783-816 (30), 818-827 (10), 828-830 (3), 854-856 (3) | `src-pkg/forge/seeds/notify.json` |
| `skills/hark/engine/tests/fit_pipeline/conftest.py` | 36.1 | 1.8 | 64 / 663 | none | `src-pkg/forge/fit/modal.py` |
| `skills/hark/engine/tests/listen/standins.py` | 35.9 | 8.5 | 83 / 412 | 105-111 (3), 116-120 (3), 208-212 (3), 272-274 (3) | `src-pkg/features.py` |
| `skills/hark/engine/hark/forge/recipes/risers/downlifter.py` | 35.8 | 4.7 | 6 / 139 | none | `src-pkg/forge/recipes/downlifter.py` |
| `skills/hark/engine/hark/core/audio.py` | 35.7 | 4.2 | 13 / 279 | none | `src-pkg/audio.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/mallet.py` | 35.4 | 7.0 | 17 / 252 | none | `src-pkg/forge/recipes/mallet.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/subdrop.py` | 35.2 | 6.4 | 17 / 156 | none | `src-pkg/forge/recipes/subdrop.py` |
| `skills/hark/engine/hark/forge/recipes/risers/riser.py` | 35.2 | 4.1 | 26 / 474 | none | `src-pkg/forge/recipes/riser.py` |
| `skills/hark/engine/hark/forge/engine/eval/bench.py` | 34.8 | 7.2 | 30 / 508 | 285-287 (3) | `src-eval/forge/bench.py` |
| `skills/hark/engine/tests/listen/test_realness.py` | 34.2 | 3.3 | 7 / 167 | none | `src-tests/test_realness.py` |
| `skills/hark/engine/hark/forge/recipes/motion/zip.py` | 33.7 | 3.2 | 4 / 201 | none | `src-pkg/forge/recipes/zip.py` |
| `skills/hark/engine/hark/forge/seeds/motion/zip.json` | 33.6 | 12.6 | 88 / 190 | 19-21 (3), 62-64 (3), 71-73 (3) | `src-pkg/forge/seeds/zip.json` |
| `skills/hark/engine/tests/recipes_ui/test_ui_keys.py` | 33.2 | 2.6 | 2 / 127 | none | `src-tests/test_forge_recipes_digital.py` |
| `skills/hark/engine/hark/surfaces/mcp_server.py` | 33.1 | 3.2 | 18 / 622 | none | `src-pkg/mcp_server.py` |
| `skills/hark/engine/hark/forge/recipes/ui/shutter.py` | 32.8 | 3.8 | 2 / 120 | none | `src-pkg/forge/recipes/shutter.py` |
| `skills/hark/engine/hark/timeline/critique.py` | 32.8 | 2.2 | 14 / 688 | none | `src-pkg/critique.py` |
| `skills/hark/engine/hark/surfaces/llm.py` | 32.6 | 4.6 | 21 / 502 | none | `src-pkg/llm.py` |
| `skills/hark/engine/hark/forge/seeds/risers/glitch.json` | 32.3 | 43.6 | 251 / 298 | 38-40 (3), 45-59 (11), 65-83 (18), 88-90 (3), 92-109 (13), 115-133 (18), 138-140 (3), 142-159 (14), 165-183 (18), 188-190 (3), 192-209 (13), 215-233 (18), 238-240 (3), 242-259 (14), 265-283 (18), 288-290 (3), 292-309 (13), 315-333 (18), 338-340 (3) | `src-pkg/forge/seeds/glitch.json` |
| `skills/hark/engine/hark/data/timeline/launch_profile.json` | 32.2 | 41.4 | 175 / 216 | 1-11 (7), 13-20 (5), 22-29 (4), 31-38 (4), 40-47 (5), 49-56 (5), 58-65 (6), 67-74 (6), 76-83 (4), 85-92 (5), 94-101 (5), 103-110 (5), 112-119 (5), 121-128 (4), 139-146 (5), 148-155 (6), 157-164 (4), 166-173 (4), 175-182 (4), 184-191 (4), 193-200 (6), 202-209 (6), 211-218 (6), 220-227 (6), 229-236 (6), 238-245 (6), 247-254 (6), 256-263 (6), 265-272 (6), 274-281 (6), 283-290 (6), 292-299 (6) | `src-pkg/data/launch_profile.json` |
| `skills/hark/engine/hark/forge/seeds/motion/swipe.json` | 32.2 | 7.2 | 45 / 341 | none | `src-pkg/forge/seeds/swipe.json` |
| `skills/hark/engine/hark/forge/recipes/motion/hover.py` | 31.7 | 4.8 | 10 / 197 | none | `src-pkg/forge/recipes/click.py` |
| `skills/hark/engine/tests/fit_analysis/test_kinds.py` | 31.7 | 3.6 | 16 / 244 | none | `src-tests/test_fit_gesture.py` |
| `skills/hark/engine/tests/timeline/conftest.py` | 31.6 | 3.7 | 61 / 428 | none | `src-pkg/profile.py` |
| `skills/hark/engine/hark/doctor.py` | 31.4 | 5.7 | 6 / 215 | none | `src-pkg/doctor.py` |
| `skills/hark/engine/tests/fit_pipeline/test_engine.py` | 31.3 | 3.1 | 14 / 236 | none | `src-tests/test_fit_engine.py` |
| `skills/hark/engine/hark/timeline/__init__.py` | 31.2 | 13.6 | 1 / 25 | none | `src-pkg/_callable.py` |
| `skills/hark/engine/hark/forge/recipes/impacts/thump.py` | 31.1 | 10.5 | 5 / 82 | none | `src-pkg/forge/recipes/thump.py` |
| `skills/hark/engine/hark/fit/analysis/_kernels.py` | 31.0 | 2.0 | 34 / 434 | none | `src-pkg/forge/dsp.py` |
| `skills/hark/engine/tests/timeline/test_critique.py` | 30.9 | 3.3 | 6 / 184 | none | `src-tests/test_launch.py` |
| `skills/hark/engine/hark/fit/pipeline/_character.py` | 30.7 | 2.7 | 10 / 216 | none | `src-pkg/forge/fit/quality.py` |
| `skills/hark/engine/hark/forge/seeds/motion/swish.json` | 30.6 | 7.4 | 45 / 337 | none | `src-pkg/forge/seeds/swish.json` |
| `skills/hark/engine/tests/forge/test_polish.py` | 30.2 | 4.2 | 6 / 117 | none | `src-tests/test_forge_critic.py` |
| `skills/hark/engine/hark/forge/engine/eval/blind_trim.py` | 30.1 | 3.0 | 28 / 293 | 21-26 (6), 324-326 (3) | `src-pkg/features.py` |

### Prose failures: token similarity over 20 %

| Our file | Token % | Line % | Max run | Source role file |
|---|---:|---:|---:|---|
| `skills/sound-sourcing/references/sound-design.md` | 64.5 | 1.2 | 0 | `src-skills/sourcing/references/sound-design.md` |
| `skills/sound-sourcing/references/role-designer.md` | 61.8 | 5.3 | 0 | `src-skills/sourcing/templates/prompts/designer.md` |
| `docs/specs/hark/skills-and-docs-hark.md` | 57.2 | 1.0 | 0 | `src-skills/tool/SKILL.md` |
| `skills/sound-sourcing/references/role-music.md` | 56.2 | 5.0 | 0 | `src-skills/sourcing/templates/prompts/music_supervisor.md` |
| `skills/sound-sourcing/references/role-critic-masking.md` | 52.8 | 3.4 | 0 | `src-skills/sourcing/templates/prompts/critic_masking.md` |
| `skills/sound-sourcing/references/rights.md` | 52.6 | 4.0 | 0 | `src-skills/sourcing/references/licences.md` |
| `skills/sound-sourcing/references/role-rights-audit.md` | 52.3 | 5.6 | 0 | `src-skills/sourcing/templates/prompts/licence_audit.md` |
| `skills/sound-sourcing/references/role-critic-sync.md` | 52.2 | 3.4 | 0 | `src-skills/sourcing/templates/prompts/critic_sync.md` |
| `skills/sound-sourcing/references/role-critic-director.md` | 51.3 | 3.4 | 0 | `src-skills/sourcing/templates/prompts/critic_director.md` |
| `skills/sound-sourcing/references/troubleshooting.md` | 50.1 | 13.0 | 1 | `src-skills/sourcing/references/troubleshooting.md` |
| `skills/sound-sourcing/references/role-critic-tonal.md` | 50.0 | 3.1 | 0 | `src-skills/sourcing/templates/prompts/critic_tonal.md` |
| `docs/specs/hark/bank-kit-sfx-kit.md` | 49.7 | 1.8 | 1 | `src-docs/SFX.md` |
| `skills/sound-sourcing/references/role-fixer.md` | 49.2 | 5.2 | 0 | `src-skills/sourcing/templates/prompts/fixer.md` |
| `skills/sound-sourcing/references/design-format.md` | 47.6 | 5.6 | 2 | `src-skills/sourcing/references/design-format.md` |
| `skills/sound-sourcing/references/role-sourcing.md` | 47.2 | 5.1 | 0 | `src-skills/sourcing/templates/prompts/sourcing.md` |
| `docs/specs/hark/bank-kit-sfx-design.md` | 46.6 | 1.0 | 0 | `src-docs/motion-sfx-techniques.md` |
| `skills/sound-sourcing/templates/SOUND_BRIEF.md` | 46.5 | 4.7 | 1 | `src-skills/sourcing/templates/SOUND_BRIEF.md` |
| `skills/sound-sourcing/references/role-critic-edges.md` | 46.0 | 3.5 | 0 | `src-skills/sourcing/templates/prompts/critic_edges.md` |
| `skills/sound-sourcing/SKILL.md` | 44.7 | 7.5 | 1 | `src-skills/sourcing/SKILL.md` |
| `docs/specs/hark/surfaces-limits.md` | 39.5 | 4.5 | 2 | `src-skills/tool/SKILL.md` |
| `skills/sound-sourcing/references/process.md` | 38.1 | 4.9 | 1 | `src-skills/sourcing/SKILL.md` |
| `docs/specs/hark/fit.md` | 38.0 | 1.9 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/bank-kit-sfx-sfx.md` | 37.9 | 1.1 | 1 | `src-docs/SFX.md` |
| `docs/specs/hark/bank-kit-sfx-bank.md` | 36.9 | 1.1 | 1 | `src-docs/SFX.md` |
| `docs/specs/hark/bank-kit-sfx-catalog.md` | 36.6 | 0.4 | 0 | `src-docs/SFX.md` |
| `docs/specs/hark/fit-auto.md` | 34.3 | 1.0 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/bank-kit-sfx.md` | 33.9 | 2.0 | 0 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/skills-and-docs.md` | 32.0 | 2.8 | 1 | `src-skills/sourcing/references/process.md` |
| `docs/specs/hark/surfaces-mcp.md` | 32.0 | 1.0 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/fit-sines.md` | 30.8 | 0.8 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/forge-core-loop.md` | 30.7 | 1.3 | 1 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/forge-core-bench.md` | 30.6 | 0.8 | 0 | `src-docs/SFX.md` |
| `skills/hark/references/listening.md` | 30.3 | 3.0 | 1 | `src-docs/LIMITS.md` |
| `docs/specs/hark/skills-and-docs-workflow.md` | 30.2 | 1.2 | 1 | `src-docs/SFX.md` |
| `docs/specs/hark/timeline.md` | 30.0 | 3.4 | 0 | `src-eval/timeline/RESULTS.md` |
| `docs/specs/hark/listen-profile.md` | 29.9 | 0.7 | 0 | `src-docs/launch-video-sound.md` |
| `docs/specs/hark/listen.md` | 29.8 | 1.5 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/core-io-behaviour.md` | 29.3 | 0.6 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/listen-eval.md` | 29.2 | 1.1 | 0 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/recipes-motion.md` | 28.9 | 1.8 | 0 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/timeline-critique.md` | 28.9 | 0.7 | 1 | `src-docs/launch-video-sound.md` |
| `docs/specs/hark/surfaces-gallery.md` | 28.6 | 1.0 | 0 | `src-docs/SFX.md` |
| `docs/specs/hark/fit-codegen.md` | 28.5 | 1.2 | 0 | `src-root/CHANGELOG.md` |
| `skills/hark/SKILL.md` | 28.2 | 2.2 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/fit-modal.md` | 28.1 | 1.0 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/forge-core-blind.md` | 28.1 | 0.5 | 0 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/skills-and-docs-mixer.md` | 28.1 | 2.9 | 1 | `src-docs/SFX.md` |
| `docs/specs/hark/surfaces-critique.md` | 28.1 | 1.5 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/listen-realness.md` | 27.8 | 1.3 | 0 | `src-eval/realness/RESULTS.md` |
| `docs/specs/hark/surfaces.md` | 27.7 | 3.7 | 2 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/timeline-analysis.md` | 27.7 | 0.8 | 0 | `src-docs/SFX.md` |
| `docs/specs/hark/skills-and-docs-scripts.md` | 27.5 | 3.1 | 1 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/recipes-impacts.md` | 27.1 | 0.0 | 0 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/listen-find.md` | 27.0 | 0.8 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/recipes-impacts-hits.md` | 26.5 | 0.0 | 0 | `src-docs/SFX.md` |
| `docs/specs/hark/fit-render.md` | 26.0 | 1.1 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/timeline-sheet-video.md` | 26.0 | 1.4 | 0 | `src-eval/timeline/RESULTS.md` |
| `docs/specs/hark/listen-calibration.md` | 25.9 | 1.1 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/forge-core.md` | 25.7 | 0.9 | 0 | `src-docs/FORGE_V2.md` |
| `docs/specs/hark/ARCHITECTURE.md` | 25.4 | 1.9 | 1 | `src-root/CHANGELOG.md` |
| `docs/specs/hark/listen-describe.md` | 25.4 | 0.4 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/recipes-motion-sweep.md` | 25.4 | 0.5 | 0 | `src-docs/motion-sfx-techniques.md` |
| `docs/specs/hark/fit-spectral.md` | 24.7 | 0.7 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/timeline-analysis-tests.md` | 24.3 | 2.2 | 0 | `src-eval/timeline/RESULTS.md` |
| `docs/specs/hark/forge-core-polish.md` | 24.1 | 0.7 | 0 | `src-docs/motion-sfx-study.md` |
| `docs/specs/hark/recipes-motion-glitch.md` | 24.1 | 0.8 | 0 | `src-docs/motion-sfx-study.md` |
| `docs/specs/hark/forge-core-search.md` | 24.0 | 0.6 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/core-io-algorithms.md` | 23.8 | 1.3 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/forge-core-knobs.md` | 23.8 | 1.0 | 0 | `src-docs/SFX.md` |
| `docs/specs/hark/recipes-impacts-pop.md` | 23.8 | 0.0 | 0 | `src-docs/motion-sfx-techniques.md` |
| `skills/hark/references/kit.md` | 23.8 | 0.0 | 0 | `src-docs/LIMITS.md` |
| `docs/specs/hark/core-io-interface.md` | 23.7 | 3.0 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/fit-quality.md` | 23.7 | 1.8 | 0 | `src-docs/FIT.md` |
| `docs/specs/hark/recipes-impacts-keys.md` | 23.5 | 0.0 | 0 | `src-docs/motion-sfx-study.md` |
| `docs/specs/hark/recipes-impacts-ui.md` | 23.4 | 0.0 | 0 | `src-docs/motion-sfx-techniques.md` |
| `docs/specs/hark/recipes-motion-pass.md` | 23.1 | 0.7 | 0 | `src-docs/FIT.md` |
| `skills/hark/references/review.md` | 23.0 | 2.6 | 1 | `src-skills/sourcing/templates/prompts/critic_director.md` |
| `docs/specs/hark/core-io.md` | 22.9 | 2.1 | 0 | `src-docs/DESIGN.md` |
| `docs/specs/hark/listen-compare.md` | 22.7 | 0.9 | 0 | `src-skills/tool/SKILL.md` |
| `docs/specs/hark/forge-core-critic.md` | 22.6 | 0.5 | 0 | `src-docs/FORGE_V2.md` |
| `docs/specs/hark/forge-core-refsets-viz.md` | 22.6 | 3.6 | 0 | `src-eval/forge/RESULTS.md` |
| `docs/specs/hark/recipes-impacts-tuned.md` | 22.6 | 0.0 | 0 | `src-docs/motion-sfx-study.md` |
| `skills/hark/references/game-sfx.md` | 22.6 | 1.8 | 0 | `src-skills/sourcing/references/troubleshooting.md` |
| `docs/specs/hark/recipes-impacts-seeds.md` | 22.4 | 0.0 | 0 | `src-docs/FORGE_V2.md` |
| `docs/specs/hark/recipes-impacts-impact.md` | 22.1 | 0.0 | 0 | `src-docs/motion-sfx-techniques.md` |
| `docs/specs/hark/forge-core-dsp.md` | 22.0 | 0.8 | 0 | `src-docs/motion-sfx-techniques.md` |
| `skills/hark/references/forge.md` | 21.9 | 0.0 | 0 | `src-docs/LIMITS.md` |
| `docs/specs/hark/fit-space.md` | 21.2 | 0.7 | 0 | `src-docs/launch-video-sound.md` |
| `docs/specs/hark/recipes-motion-tonal.md` | 21.1 | 1.0 | 1 | `src-docs/SFX.md` |
| `skills/hark/references/setup.md` | 20.9 | 5.6 | 0 | `src-docs/DESIGN.md` |
| `docs/specs/hark/core-io-tests.md` | 20.7 | 1.7 | 0 | `src-skills/sourcing/references/design-format.md` |
| `docs/specs/hark/fit-gesture.md` | 20.4 | 1.2 | 0 | `src-skills/sourcing/references/sound-design.md` |
| `docs/specs/hark/forge-core-schema.md` | 20.2 | 2.1 | 1 | `src-skills/sourcing/references/design-format.md` |
| `docs/specs/hark/forge-core-tone.md` | 20.1 | 0.6 | 0 | `src-docs/launch-video-sound.md` |

## Full table

Every measured file, in path order. Result `FAIL:tok` is over the token threshold. `FAIL:run` has an identical run of 5 or more non-trivial lines.

| Our file | Class | Lines | Token % | Line % | Max run | Run lines | Source role file | Result |
|---|---|---:|---:|---:|---:|---|---|---|
| `docs/specs/hark/ARCHITECTURE.md` | prose | 780 | 25.4 | 1.9 | 1 | 569-569 | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/agy-audio-probe.md` | prose | 71 | 19.5 | 6.3 | 0 | - | `src-docs/LIMITS.md` | pass |
| `docs/specs/hark/bank-kit-sfx-bank.md` | prose | 557 | 36.9 | 1.1 | 1 | 70-70 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/bank-kit-sfx-catalog.md` | prose | 530 | 36.6 | 0.4 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/bank-kit-sfx-design.md` | prose | 330 | 46.6 | 1.0 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/bank-kit-sfx-kit.md` | prose | 640 | 49.7 | 1.8 | 1 | 44-44 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/bank-kit-sfx-sfx.md` | prose | 451 | 37.9 | 1.1 | 1 | 78-78 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/bank-kit-sfx.md` | prose | 349 | 33.9 | 2.0 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/core-io-algorithms.md` | prose | 408 | 23.8 | 1.3 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/core-io-behaviour.md` | prose | 305 | 29.3 | 0.6 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/core-io-interface.md` | prose | 266 | 23.7 | 3.0 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/core-io-tests.md` | prose | 135 | 20.7 | 1.7 | 0 | - | `src-skills/sourcing/references/design-format.md` | FAIL:tok |
| `docs/specs/hark/core-io.md` | prose | 139 | 22.9 | 2.1 | 0 | - | `src-docs/DESIGN.md` | FAIL:tok |
| `docs/specs/hark/fit-auto.md` | prose | 302 | 34.3 | 1.0 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit-codegen.md` | prose | 315 | 28.5 | 1.2 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/fit-gesture.md` | prose | 140 | 20.4 | 1.2 | 0 | - | `src-skills/sourcing/references/sound-design.md` | FAIL:tok |
| `docs/specs/hark/fit-modal.md` | prose | 340 | 28.1 | 1.0 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit-quality.md` | prose | 233 | 23.7 | 1.8 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit-render.md` | prose | 263 | 26.0 | 1.1 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit-sines.md` | prose | 451 | 30.8 | 0.8 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit-space.md` | prose | 187 | 21.2 | 0.7 | 0 | - | `src-docs/launch-video-sound.md` | FAIL:tok |
| `docs/specs/hark/fit-spectral.md` | prose | 300 | 24.7 | 0.7 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/fit.md` | prose | 243 | 38.0 | 1.9 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/forge-core-bench.md` | prose | 503 | 30.6 | 0.8 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/forge-core-blind.md` | prose | 296 | 28.1 | 0.5 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/forge-core-critic.md` | prose | 217 | 22.6 | 0.5 | 0 | - | `src-docs/FORGE_V2.md` | FAIL:tok |
| `docs/specs/hark/forge-core-dsp.md` | prose | 557 | 22.0 | 0.8 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/forge-core-knobs.md` | prose | 308 | 23.8 | 1.0 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/forge-core-loop.md` | prose | 434 | 30.7 | 1.3 | 1 | 139-139 | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/forge-core-polish.md` | prose | 330 | 24.1 | 0.7 | 0 | - | `src-docs/motion-sfx-study.md` | FAIL:tok |
| `docs/specs/hark/forge-core-refsets-viz.md` | prose | 114 | 22.6 | 3.6 | 0 | - | `src-eval/forge/RESULTS.md` | FAIL:tok |
| `docs/specs/hark/forge-core-schema.md` | prose | 141 | 20.2 | 2.1 | 1 | 35-35 | `src-skills/sourcing/references/design-format.md` | FAIL:tok |
| `docs/specs/hark/forge-core-search.md` | prose | 292 | 24.0 | 0.6 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/forge-core-tone.md` | prose | 184 | 20.1 | 0.6 | 0 | - | `src-docs/launch-video-sound.md` | FAIL:tok |
| `docs/specs/hark/forge-core.md` | prose | 106 | 25.7 | 0.9 | 0 | - | `src-docs/FORGE_V2.md` | FAIL:tok |
| `docs/specs/hark/listen-calibration.md` | prose | 350 | 25.9 | 1.1 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/listen-compare.md` | prose | 264 | 22.7 | 0.9 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/listen-describe.md` | prose | 335 | 25.4 | 0.4 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/listen-eval.md` | prose | 559 | 29.2 | 1.1 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/listen-find.md` | prose | 376 | 27.0 | 0.8 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/listen-profile.md` | prose | 352 | 29.9 | 0.7 | 0 | - | `src-docs/launch-video-sound.md` | FAIL:tok |
| `docs/specs/hark/listen-realness.md` | prose | 564 | 27.8 | 1.3 | 0 | - | `src-eval/realness/RESULTS.md` | FAIL:tok |
| `docs/specs/hark/listen.md` | prose | 339 | 29.8 | 1.5 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-hits.md` | prose | 585 | 26.5 | 0.0 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-impact.md` | prose | 551 | 22.1 | 0.0 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-keys.md` | prose | 409 | 23.5 | 0.0 | 0 | - | `src-docs/motion-sfx-study.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-pop.md` | prose | 607 | 23.8 | 0.0 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-seeds.md` | prose | 196 | 22.4 | 0.0 | 0 | - | `src-docs/FORGE_V2.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-tonal.md` | prose | 120 | 20.0 | 0.0 | 0 | - | `src-skills/sourcing/references/design-format.md` | pass |
| `docs/specs/hark/recipes-impacts-tuned.md` | prose | 471 | 22.6 | 0.0 | 0 | - | `src-docs/motion-sfx-study.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts-ui.md` | prose | 475 | 23.4 | 0.0 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/recipes-impacts.md` | prose | 237 | 27.1 | 0.0 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/recipes-motion-glitch.md` | prose | 387 | 24.1 | 0.8 | 0 | - | `src-docs/motion-sfx-study.md` | FAIL:tok |
| `docs/specs/hark/recipes-motion-pass.md` | prose | 577 | 23.1 | 0.7 | 0 | - | `src-docs/FIT.md` | FAIL:tok |
| `docs/specs/hark/recipes-motion-sting.md` | prose | 460 | 17.5 | 1.0 | 0 | - | `src-docs/motion-sfx-study.md` | pass |
| `docs/specs/hark/recipes-motion-sweep.md` | prose | 874 | 25.4 | 0.5 | 0 | - | `src-docs/motion-sfx-techniques.md` | FAIL:tok |
| `docs/specs/hark/recipes-motion-tonal.md` | prose | 704 | 21.1 | 1.0 | 1 | 41-41 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/recipes-motion.md` | prose | 291 | 28.9 | 1.8 | 0 | - | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/skills-and-docs-hark.md` | prose | 278 | 57.2 | 1.0 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/skills-and-docs-mixer.md` | prose | 749 | 28.1 | 2.9 | 1 | 62-62 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/skills-and-docs-scripts.md` | prose | 476 | 27.5 | 3.1 | 1 | 287-287 | `src-root/CHANGELOG.md` | FAIL:tok |
| `docs/specs/hark/skills-and-docs-workflow.md` | prose | 578 | 30.2 | 1.2 | 1 | 674-674 | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/skills-and-docs.md` | prose | 176 | 32.0 | 2.8 | 1 | 80-80 | `src-skills/sourcing/references/process.md` | FAIL:tok |
| `docs/specs/hark/surfaces-critique.md` | prose | 283 | 28.1 | 1.5 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/surfaces-gallery.md` | prose | 530 | 28.6 | 1.0 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/surfaces-limits.md` | prose | 250 | 39.5 | 4.5 | 2 | 197-198 | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/surfaces-mcp.md` | prose | 330 | 32.0 | 1.0 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/surfaces.md` | prose | 355 | 27.7 | 3.7 | 2 | 290-291 | `src-skills/tool/SKILL.md` | FAIL:tok |
| `docs/specs/hark/timeline-analysis-tests.md` | prose | 43 | 24.3 | 2.2 | 0 | - | `src-eval/timeline/RESULTS.md` | FAIL:tok |
| `docs/specs/hark/timeline-analysis.md` | prose | 702 | 27.7 | 0.8 | 0 | - | `src-docs/SFX.md` | FAIL:tok |
| `docs/specs/hark/timeline-critique.md` | prose | 476 | 28.9 | 0.7 | 1 | 57-57 | `src-docs/launch-video-sound.md` | FAIL:tok |
| `docs/specs/hark/timeline-sheet-video.md` | prose | 224 | 26.0 | 1.4 | 0 | - | `src-eval/timeline/RESULTS.md` | FAIL:tok |
| `docs/specs/hark/timeline.md` | prose | 213 | 30.0 | 3.4 | 0 | - | `src-eval/timeline/RESULTS.md` | FAIL:tok |
| `skills/hark/SKILL.md` | prose | 97 | 28.2 | 2.2 | 0 | - | `src-skills/tool/SKILL.md` | FAIL:tok |
| `skills/hark/engine/.static-analysisrc.json` | code | 21 | 6.9 | 14.3 | 1 | 11-12 | `src-skills/sourcing/templates/sounddesign.json` | pass |
| `skills/hark/engine/CHANGELOG.md` | prose | 4 | 3.4 | 0.0 | 0 | - | `src-skills/sourcing/templates/prompts/critic_edges.md` | pass |
| `skills/hark/engine/README.md` | prose | 60 | 18.3 | 4.0 | 0 | - | `src-eval/README.md` | pass |
| `skills/hark/engine/hark/__init__.py` | code | 27 | 19.1 | 4.1 | 0 | - | `src-pkg/__init__.py` | pass |
| `skills/hark/engine/hark/__main__.py` | code | 5 | 75.0 | 50.0 | 1 | 8-8 | `src-pkg/__main__.py` | FAIL:tok |
| `skills/hark/engine/hark/cli.py` | code | 233 | 17.1 | 5.2 | 1 | 324-324 | `src-pkg/cli.py` | pass |
| `skills/hark/engine/hark/core/__init__.py` | code | 1 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/core/api.py` | code | 41 | 18.4 | 8.5 | 0 | - | `src-pkg/__init__.py` | pass |
| `skills/hark/engine/hark/core/audio.py` | code | 315 | 35.7 | 4.2 | 1 | 34-34 | `src-pkg/audio.py` | FAIL:tok |
| `skills/hark/engine/hark/core/config.py` | code | 105 | 19.9 | 3.4 | 0 | - | `src-pkg/config.py` | pass |
| `skills/hark/engine/hark/core/features.py` | code | 424 | 15.0 | 2.6 | 1 | 93-93 | `src-pkg/features.py` | pass |
| `skills/hark/engine/hark/core/report.py` | code | 111 | 28.2 | 6.6 | 1 | 122-122 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/core/safe.py` | code | 20 | 9.3 | 6.3 | 0 | - | `src-pkg/_callable.py` | pass |
| `skills/hark/engine/hark/data/listen/realness_model.json` | code | 19 | 6.0 | 2.4 | 3 | 6-8 | `src-pkg/data/realness_model.json` | pass |
| `skills/hark/engine/hark/data/listen/vocab.json` | code | 366 | 53.6 | 15.0 | 2 | 10-11 | `src-pkg/data/vocab.json` | FAIL:tok |
| `skills/hark/engine/hark/data/timeline/launch_profile.json` | code | 367 | 32.2 | 41.4 | 7 | 1-11 | `src-pkg/data/launch_profile.json` | FAIL:tok+run |
| `skills/hark/engine/hark/doctor.py` | code | 255 | 31.4 | 5.7 | 1 | 168-168 | `src-pkg/doctor.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/__init__.py` | code | 25 | 18.5 | 10.9 | 0 | - | `src-pkg/__init__.py` | pass |
| `skills/hark/engine/hark/fit/analysis/__init__.py` | code | 18 | 12.2 | 12.5 | 0 | - | `src-pkg/__init__.py` | pass |
| `skills/hark/engine/hark/fit/analysis/_common.py` | code | 178 | 23.5 | 3.2 | 2 | 106-107 | `src-pkg/forge/fit/gesture.py` | pass |
| `skills/hark/engine/hark/fit/analysis/_kernels.py` | code | 497 | 31.0 | 2.0 | 2 | 23-24 | `src-pkg/forge/dsp.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/analysis/gesture.py` | code | 288 | 46.2 | 6.5 | 2 | 162-163 | `src-pkg/forge/fit/gesture.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/analysis/modal.py` | code | 821 | 43.9 | 3.7 | 5 | 224-229 | `src-pkg/forge/fit/modal.py` | FAIL:tok+run |
| `skills/hark/engine/hark/fit/analysis/sinusoidal.py` | code | 1101 | 51.3 | 2.4 | 2 | 101-102 | `src-pkg/forge/fit/sines.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/analysis/space.py` | code | 389 | 45.5 | 6.7 | 2 | 240-241 | `src-pkg/forge/fit/space.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/analysis/spectral.py` | code | 1202 | 46.6 | 2.1 | 2 | 43-44 | `src-pkg/forge/fit/spectral.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/pipeline/__init__.py` | code | 1 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/_character.py` | code | 253 | 30.7 | 2.7 | 1 | 42-42 | `src-pkg/forge/fit/quality.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/pipeline/_layers.py` | code | 99 | 19.3 | 1.8 | 1 | 34-34 | `src-tests/test_fit_choice.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/_model.py` | code | 99 | 25.1 | 4.7 | 2 | 135-136 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/_morph.py` | code | 559 | 25.4 | 1.4 | 2 | 105-106 | `src-pkg/forge/fit/auto.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/_post.py` | code | 252 | 27.4 | 2.8 | 1 | 27-27 | `src-pkg/forge/dsp.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/_postfit.py` | code | 212 | 23.8 | 2.5 | 1 | 47-47 | `src-pkg/forge/fit/quality.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/auto.py` | code | 286 | 19.5 | 2.0 | 1 | 80-80 | `src-pkg/forge/fit/gesture.py` | pass |
| `skills/hark/engine/hark/fit/pipeline/cli.py` | code | 273 | 36.6 | 2.9 | 2 | 239-240 | `src-pkg/forge/fit/__init__.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/pipeline/codegen.py` | code | 280 | 37.1 | 7.6 | 3 | 377-379 | `src-pkg/forge/fit/codegen.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/pipeline/quality.py` | code | 409 | 48.7 | 8.2 | 3 | 98-100 | `src-pkg/forge/fit/quality.py` | FAIL:tok |
| `skills/hark/engine/hark/fit/pipeline/render.py` | code | 353 | 23.1 | 1.8 | 1 | 191-191 | `src-pkg/forge/fit/gesture.py` | pass |
| `skills/hark/engine/hark/forge/__init__.py` | code | 21 | 25.5 | 9.1 | 0 | - | `src-pkg/__init__.py` | pass |
| `skills/hark/engine/hark/forge/bank/__init__.py` | code | 5 | 0.0 | 0.6 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/bank/__main__.py` | code | 76 | 18.1 | 6.8 | 1 | 101-101 | `src-pkg/cli.py` | pass |
| `skills/hark/engine/hark/forge/bank/_build.py` | code | 608 | 24.3 | 2.2 | 1 | 98-98 | `src-pkg/forge/bank.py` | pass |
| `skills/hark/engine/hark/forge/bank/_measures.py` | code | 198 | 25.6 | 3.9 | 1 | 35-35 | `src-pkg/features.py` | pass |
| `skills/hark/engine/hark/forge/bank/bank.py` | code | 347 | 23.2 | 2.5 | 1 | 46-46 | `src-pkg/forge/schema.py` | pass |
| `skills/hark/engine/hark/forge/bank/catalog.json` | code | 354 | 27.2 | 1.9 | 1 | 2-2 | `src-pkg/forge/bank/forge04.json` | pass |
| `skills/hark/engine/hark/forge/bank/data/forge04.json` | code | 62 | 78.0 | 7.9 | 1 | 2-2 | `src-pkg/forge/bank/forge04.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/bank/data/forge_v2.json` | code | 223 | 63.8 | 3.5 | 1 | 2-2 | `src-pkg/forge/bank/forge_v2.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/bank/data/pluck.json` | code | 10 | 90.0 | 50.0 | 1 | 2-2 | `src-pkg/forge/bank/pluck.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/bank/kit.py` | code | 816 | 41.3 | 4.3 | 2 | 317-319 | `src-pkg/forge/kit.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/bank/sfx.py` | code | 414 | 36.4 | 3.3 | 2 | 33-34 | `src-pkg/forge/sfx.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/__init__.py` | code | 0 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/engine/args.py` | code | 26 | 10.6 | 4.1 | 0 | - | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/forge/engine/cli.py` | code | 127 | 17.7 | 4.1 | 1 | 127-127 | `src-pkg/forge/fit/__init__.py` | pass |
| `skills/hark/engine/hark/forge/engine/critic.py` | code | 511 | 45.0 | 2.8 | 2 | 557-558 | `src-pkg/forge/critic.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/dsp.py` | code | 528 | 58.4 | 17.7 | 6 | 517-523 | `src-pkg/forge/dsp.py` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/engine/eval/__init__.py` | code | 3 | 0.0 | 0.6 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/engine/eval/bench.py` | code | 589 | 34.8 | 7.2 | 3 | 285-287 | `src-eval/forge/bench.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/eval/blind_progress.py` | code | 207 | 23.2 | 2.8 | 1 | 44-44 | `src-pkg/audio.py` | pass |
| `skills/hark/engine/hark/forge/engine/eval/blind_sheet.py` | code | 86 | 15.3 | 6.3 | 1 | 56-56 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/forge/engine/eval/blind_trim.py` | code | 319 | 30.1 | 3.0 | 6 | 21-26 | `src-pkg/features.py` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/engine/eval/blind_v2.py` | code | 405 | 22.4 | 3.1 | 2 | 96-97 | `src-pkg/forge/tone.py` | pass |
| `skills/hark/engine/hark/forge/engine/eval/build_refsets.py` | code | 373 | 28.2 | 3.7 | 1 | 153-153 | `src-eval/forge/build_refsets.py` | pass |
| `skills/hark/engine/hark/forge/engine/eval/frozen_tone.py` | code | 142 | 57.4 | 11.6 | 3 | 59-61 | `src-eval/forge/frozen_tone.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/eval/scrub_paths.py` | code | 106 | 37.0 | 11.0 | 2 | 62-63 | `src-eval/forge/scrub_paths.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/fx.py` | code | 822 | 50.3 | 7.2 | 3 | 725-727 | `src-pkg/forge/fx.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/knobs.py` | code | 758 | 22.9 | 1.7 | 3 | 878-880 | `src-pkg/forge/search.py` | pass |
| `skills/hark/engine/hark/forge/engine/loop.py` | code | 614 | 39.1 | 3.1 | 1 | 79-79 | `src-pkg/forge/__init__.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/loop_report.py` | code | 175 | 22.3 | 3.1 | 1 | 32-32 | `src-pkg/audio.py` | pass |
| `skills/hark/engine/hark/forge/engine/polish.py` | code | 730 | 49.5 | 8.2 | 3 | 587-589 | `src-pkg/forge/polish.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/refsets.py` | code | 145 | 29.8 | 9.3 | 2 | 93-94 | `src-pkg/forge/refsets.py` | pass |
| `skills/hark/engine/hark/forge/engine/schema.py` | code | 337 | 52.3 | 12.2 | 2 | 294-295 | `src-pkg/forge/schema.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/search.py` | code | 860 | 52.1 | 7.9 | 2 | 122-123 | `src-pkg/forge/search.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/tone.py` | code | 367 | 57.2 | 8.1 | 3 | 23-25 | `src-pkg/forge/tone.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/engine/viz.py` | code | 122 | 52.4 | 11.3 | 2 | 74-75 | `src-pkg/forge/viz.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/kits/calm.json` | code | 161 | 50.5 | 12.2 | 3 | 70-72 | `src-pkg/forge/kits/calm.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/kits/crisp.json` | code | 154 | 46.9 | 11.7 | 2 | 1-4 | `src-pkg/forge/kits/crisp.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/kits/launch.json` | code | 268 | 67.4 | 6.4 | 2 | 1-4 | `src-pkg/forge/kits/launch.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/kits/minimal.json` | code | 154 | 60.8 | 11.7 | 2 | 1-4 | `src-pkg/forge/kits/minimal.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/__init__.py` | code | 72 | 15.3 | 4.2 | 0 | - | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/forge/recipes/impacts/__init__.py` | code | 0 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/recipes/impacts/cuthit.py` | code | 237 | 37.0 | 6.9 | 2 | 142-143 | `src-pkg/forge/recipes/cuthit.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/impacts/drip.py` | code | 144 | 29.2 | 3.2 | 1 | 132-132 | `src-pkg/forge/recipes/drip.py` | pass |
| `skills/hark/engine/hark/forge/recipes/impacts/impact.py` | code | 998 | 43.4 | 4.7 | 2 | 73-74 | `src-pkg/forge/recipes/impact.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/impacts/knock.py` | code | 185 | 27.4 | 4.5 | 1 | 140-140 | `src-pkg/forge/recipes/knock.py` | pass |
| `skills/hark/engine/hark/forge/recipes/impacts/mallet.py` | code | 280 | 35.4 | 7.0 | 2 | 198-199 | `src-pkg/forge/recipes/mallet.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/impacts/pluck.py` | code | 206 | 45.4 | 6.8 | 2 | 110-111 | `src-pkg/forge/recipes/pluck.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/impacts/subdrop.py` | code | 185 | 35.2 | 6.4 | 2 | 221-222 | `src-pkg/forge/recipes/subdrop.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/impacts/thump.py` | code | 103 | 31.1 | 10.5 | 1 | 48-48 | `src-pkg/forge/recipes/thump.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/motion/__init__.py` | code | 0 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/_seeds.py` | code | 77 | 14.5 | 6.8 | 1 | 100-100 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/_util.py` | code | 104 | 23.8 | 3.9 | 1 | 27-27 | `src-pkg/forge/recipes/_tonal.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/_whoosh_v1.py` | code | 142 | 23.3 | 2.7 | 1 | 163-163 | `src-pkg/forge/recipes/typing.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/brightswish.py` | code | 114 | 27.0 | 8.8 | 1 | 77-78 | `src-pkg/forge/recipes/brightswish.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/hover.py` | code | 219 | 31.7 | 4.8 | 2 | 100-101 | `src-pkg/forge/recipes/click.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/motion/shimmer.py` | code | 401 | 44.2 | 7.0 | 2 | 356-357 | `src-pkg/forge/recipes/shimmer.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/motion/swipe.py` | code | 70 | 23.5 | 3.4 | 0 | - | `src-pkg/forge/recipes/swish.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/swish.py` | code | 69 | 23.5 | 3.4 | 0 | - | `src-pkg/forge/recipes/swish.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/whoosh.py` | code | 432 | 29.9 | 3.1 | 2 | 494-495 | `src-pkg/forge/recipes/whoosh.py` | pass |
| `skills/hark/engine/hark/forge/recipes/motion/zip.py` | code | 235 | 33.7 | 3.2 | 1 | 132-132 | `src-pkg/forge/recipes/zip.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/__init__.py` | code | 0 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/recipes/risers/_common.py` | code | 108 | 23.3 | 3.1 | 1 | 35-35 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/hark/forge/recipes/risers/_glitchkit.py` | code | 79 | 20.4 | 2.9 | 1 | 44-44 | `src-pkg/forge/recipes/burstglitch.py` | pass |
| `skills/hark/engine/hark/forge/recipes/risers/_seedbank.py` | code | 85 | 16.2 | 5.1 | 1 | 26-26 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/forge/recipes/risers/burstglitch.py` | code | 137 | 29.1 | 6.0 | 1 | 81-81 | `src-pkg/forge/recipes/burstglitch.py` | pass |
| `skills/hark/engine/hark/forge/recipes/risers/darkriser.py` | code | 109 | 39.3 | 5.8 | 2 | 60-61 | `src-pkg/forge/recipes/darkriser.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/downlifter.py` | code | 167 | 35.8 | 4.7 | 1 | 117-117 | `src-pkg/forge/recipes/downlifter.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/glitch.py` | code | 407 | 36.4 | 2.4 | 2 | 447-448 | `src-pkg/forge/recipes/glitch.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/notify.py` | code | 545 | 42.1 | 5.6 | 3 | 557-560 | `src-pkg/forge/recipes/notify.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/revcymbal.py` | code | 131 | 37.2 | 5.4 | 1 | 54-54 | `src-pkg/forge/recipes/revcymbal.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/riser.py` | code | 538 | 35.2 | 4.1 | 2 | 302-303 | `src-pkg/forge/recipes/riser.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/risers/sting.py` | code | 756 | 48.5 | 3.7 | 3 | 342-344 | `src-pkg/forge/recipes/sting.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/tonal.py` | code | 224 | 54.5 | 11.8 | 2 | 125-127 | `src-pkg/forge/recipes/_tonal.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/__init__.py` | code | 0 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/forge/recipes/ui/_layers.py` | code | 220 | 37.2 | 3.2 | 1 | 25-25 | `src-pkg/forge/recipes/click.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/click.py` | code | 127 | 15.4 | 4.0 | 1 | 62-62 | `src-pkg/forge/recipes/toggle.py` | pass |
| `skills/hark/engine/hark/forge/recipes/ui/keypress.py` | code | 315 | 47.5 | 5.2 | 2 | 377-378 | `src-pkg/forge/recipes/keypress.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/pop.py` | code | 649 | 39.5 | 3.1 | 2 | 80-81 | `src-pkg/forge/recipes/pop.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/popin.py` | code | 223 | 29.5 | 3.6 | 1 | 48-48 | `src-pkg/forge/recipes/popin.py` | pass |
| `skills/hark/engine/hark/forge/recipes/ui/shutter.py` | code | 135 | 32.8 | 3.8 | 1 | 154-154 | `src-pkg/forge/recipes/shutter.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/toggle.py` | code | 137 | 22.3 | 3.5 | 1 | 59-59 | `src-pkg/forge/recipes/toggle.py` | pass |
| `skills/hark/engine/hark/forge/recipes/ui/typing.py` | code | 272 | 46.5 | 7.9 | 2 | 300-302 | `src-pkg/forge/recipes/typing.py` | FAIL:tok |
| `skills/hark/engine/hark/forge/recipes/ui/ui_pop.py` | code | 129 | 27.5 | 3.0 | 1 | 119-119 | `src-pkg/forge/recipes/drip.py` | pass |
| `skills/hark/engine/hark/forge/seeds/impacts/impact.json` | code | 885 | 52.2 | 20.0 | 8 | 501-508 | `src-pkg/forge/seeds/impact.json` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/seeds/impacts/subdrop.json` | code | 305 | 42.9 | 38.5 | 4 | 8-11 | `src-pkg/forge/seeds/subdrop.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/motion/hover.json` | code | 375 | 28.7 | 13.7 | 3 | 240-242 | `src-pkg/forge/seeds/hover.json` | pass |
| `skills/hark/engine/hark/forge/seeds/motion/shimmer.json` | code | 687 | 37.7 | 10.7 | 2 | 5-6 | `src-pkg/forge/seeds/shimmer.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/motion/swipe.json` | code | 370 | 32.2 | 7.2 | 2 | 5-6 | `src-pkg/forge/seeds/swipe.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/motion/swish.json` | code | 370 | 30.6 | 7.4 | 2 | 5-6 | `src-pkg/forge/seeds/swish.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/motion/whoosh.json` | code | 1319 | 25.6 | 8.0 | 2 | 5-6 | `src-pkg/forge/seeds/whoosh.json` | pass |
| `skills/hark/engine/hark/forge/seeds/motion/zip.json` | code | 220 | 33.6 | 12.6 | 3 | 19-21 | `src-pkg/forge/seeds/zip.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/risers/downlifter.json` | code | 257 | 38.8 | 25.9 | 3 | 38-40 | `src-pkg/forge/seeds/downlifter.json` | FAIL:tok |
| `skills/hark/engine/hark/forge/seeds/risers/glitch.json` | code | 347 | 32.3 | 43.6 | 18 | 65-83 | `src-pkg/forge/seeds/glitch.json` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/seeds/risers/notify.json` | code | 863 | 36.1 | 45.7 | 31 | 1-36 | `src-pkg/forge/seeds/notify.json` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/seeds/risers/riser.json` | code | 569 | 37.3 | 23.9 | 19 | 4-25 | `src-pkg/forge/seeds/riser.json` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/seeds/risers/sting.json` | code | 456 | 27.6 | 45.2 | 35 | 1-38 | `src-pkg/forge/seeds/sting.json` | FAIL:run |
| `skills/hark/engine/hark/forge/seeds/ui/click.json` | code | 405 | 23.6 | 14.4 | 5 | 145-150 | `src-pkg/forge/seeds/toggle.json` | FAIL:run |
| `skills/hark/engine/hark/forge/seeds/ui/keypress.json` | code | 389 | 29.5 | 12.7 | 14 | 9-23 | `src-pkg/forge/seeds/keypress.json` | FAIL:run |
| `skills/hark/engine/hark/forge/seeds/ui/pop.json` | code | 1061 | 36.4 | 41.9 | 23 | 579-605 | `src-pkg/forge/seeds/pop.json` | FAIL:tok+run |
| `skills/hark/engine/hark/forge/seeds/ui/popin.json` | code | 710 | 25.6 | 12.8 | 5 | 28-33 | `src-pkg/forge/seeds/popin.json` | FAIL:run |
| `skills/hark/engine/hark/forge/seeds/ui/toggle.json` | code | 235 | 24.9 | 11.5 | 5 | 203-207 | `src-pkg/forge/seeds/toggle.json` | FAIL:run |
| `skills/hark/engine/hark/forge/seeds/ui/typing.json` | code | 479 | 39.3 | 15.0 | 4 | 235-238 | `src-pkg/forge/seeds/typing.json` | FAIL:tok |
| `skills/hark/engine/hark/listen/__init__.py` | code | 1 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/listen/compare.py` | code | 367 | 36.2 | 3.2 | 1 | 30-30 | `src-pkg/compare.py` | FAIL:tok |
| `skills/hark/engine/hark/listen/describe.py` | code | 326 | 27.1 | 3.2 | 1 | 22-22 | `src-pkg/describe.py` | pass |
| `skills/hark/engine/hark/listen/eval/__init__.py` | code | 1 | 0.0 | 0.0 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/listen/eval/dataset.py` | code | 108 | 19.0 | 5.6 | 0 | - | `src-eval/forge/scrub_paths.py` | pass |
| `skills/hark/engine/hark/listen/eval/find_checks.py` | code | 153 | 16.8 | 3.6 | 1 | 51-51 | `src-pkg/match.py` | pass |
| `skills/hark/engine/hark/listen/eval/heldout_labels.json` | code | 5 | 4.8 | 33.3 | 0 | - | `src-eval/semantics/heldout_labels.json` | pass |
| `skills/hark/engine/hark/listen/eval/kind_queries.json` | code | 125 | 44.3 | 7.6 | 1 | 23-23 | `src-eval/semantics/kind_queries.json` | FAIL:tok |
| `skills/hark/engine/hark/listen/eval/metrics.py` | code | 294 | 23.0 | 2.1 | 1 | 86-86 | `src-pkg/vocab.py` | pass |
| `skills/hark/engine/hark/listen/eval/precision.py` | code | 83 | 19.0 | 12.2 | 1 | 59-59 | `src-eval/realness/extract.py` | pass |
| `skills/hark/engine/hark/listen/eval/queries.json` | code | 590 | 38.4 | 18.0 | 1 | 3-3 | `src-eval/semantics/queries.json` | FAIL:tok |
| `skills/hark/engine/hark/listen/eval/real_effects.py` | code | 123 | 25.2 | 6.9 | 1 | 150-150 | `src-eval/semantics/real_effects.py` | pass |
| `skills/hark/engine/hark/listen/eval/run_eval.py` | code | 336 | 25.7 | 3.7 | 1 | 40-40 | `src-pkg/vocab.py` | pass |
| `skills/hark/engine/hark/listen/eval/scenes.py` | code | 108 | 22.8 | 3.8 | 1 | 40-40 | `src-eval/timeline/mixes.py` | pass |
| `skills/hark/engine/hark/listen/match.py` | code | 131 | 38.7 | 4.4 | 1 | 56-56 | `src-pkg/match.py` | FAIL:tok |
| `skills/hark/engine/hark/listen/profile.py` | code | 620 | 38.4 | 4.3 | 1 | 36-36 | `src-pkg/profile.py` | FAIL:tok |
| `skills/hark/engine/hark/listen/realness.py` | code | 676 | 36.5 | 3.8 | 2 | 150-151 | `src-pkg/realness.py` | FAIL:tok |
| `skills/hark/engine/hark/listen/realness_train.py` | code | 277 | 26.8 | 2.7 | 1 | 26-26 | `src-eval/realness/common.py` | pass |
| `skills/hark/engine/hark/listen/search.py` | code | 565 | 38.5 | 3.6 | 2 | 316-317 | `src-pkg/search.py` | FAIL:tok |
| `skills/hark/engine/hark/listen/vocabulary.py` | code | 544 | 39.8 | 5.5 | 3 | 152-155 | `src-pkg/vocab.py` | FAIL:tok |
| `skills/hark/engine/hark/models/__init__.py` | code | 50 | 11.8 | 3.3 | 1 | 23-23 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/models/_common.py` | code | 129 | 25.4 | 8.8 | 1 | 42-42 | `src-pkg/models/ast.py` | pass |
| `skills/hark/engine/hark/models/ast.py` | code | 97 | 45.5 | 9.2 | 1 | 52-52 | `src-pkg/models/ast.py` | FAIL:tok |
| `skills/hark/engine/hark/models/clap.py` | code | 154 | 39.1 | 5.8 | 1 | 15-15 | `src-pkg/models/clap.py` | FAIL:tok |
| `skills/hark/engine/hark/models/hub.py` | code | 173 | 48.1 | 10.7 | 2 | 152-153 | `src-pkg/models/hub.py` | FAIL:tok |
| `skills/hark/engine/hark/surfaces/__init__.py` | code | 3 | 0.0 | 0.6 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/hark/surfaces/_flow.py` | code | 43 | 13.3 | 5.1 | 1 | 17-17 | `src-pkg/report.py` | pass |
| `skills/hark/engine/hark/surfaces/gallery.py` | code | 423 | 21.8 | 1.7 | 1 | 230-230 | `src-pkg/forge/schema.py` | pass |
| `skills/hark/engine/hark/surfaces/gallery_inputs.py` | code | 417 | 22.4 | 2.7 | 2 | 77-78 | `src-pkg/forge/kit.py` | pass |
| `skills/hark/engine/hark/surfaces/gallery_page.py` | code | 524 | 21.2 | 2.2 | 2 | 486-487 | `src-pkg/vocab.py` | pass |
| `skills/hark/engine/hark/surfaces/gallery_sound.py` | code | 193 | 25.6 | 3.6 | 1 | 17-17 | `src-pkg/features.py` | pass |
| `skills/hark/engine/hark/surfaces/llm.py` | code | 572 | 32.6 | 4.6 | 2 | 211-212 | `src-pkg/llm.py` | FAIL:tok |
| `skills/hark/engine/hark/surfaces/mcp_server.py` | code | 701 | 33.1 | 3.2 | 1 | 35-35 | `src-pkg/mcp_server.py` | FAIL:tok |
| `skills/hark/engine/hark/timeline/__init__.py` | code | 33 | 31.2 | 13.6 | 1 | 29-29 | `src-pkg/_callable.py` | FAIL:tok |
| `skills/hark/engine/hark/timeline/_beats.py` | code | 126 | 25.2 | 1.7 | 1 | 20-20 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/hark/timeline/_dsp.py` | code | 91 | 22.6 | 3.0 | 1 | 68-68 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/hark/timeline/_events.py` | code | 236 | 27.2 | 2.1 | 1 | 18-18 | `src-pkg/forge/tone.py` | pass |
| `skills/hark/engine/hark/timeline/_loudness.py` | code | 90 | 22.8 | 2.7 | 1 | 31-31 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/hark/timeline/_sections.py` | code | 272 | 24.0 | 2.0 | 2 | 37-38 | `src-pkg/forge/tone.py` | pass |
| `skills/hark/engine/hark/timeline/_sync.py` | code | 197 | 22.4 | 3.0 | 1 | 25-25 | `src-pkg/vocab.py` | pass |
| `skills/hark/engine/hark/timeline/_tags.py` | code | 199 | 24.3 | 3.9 | 2 | 84-85 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/hark/timeline/_text.py` | code | 225 | 22.6 | 2.0 | 1 | 123-123 | `src-pkg/describe.py` | pass |
| `skills/hark/engine/hark/timeline/critique.py` | code | 780 | 32.8 | 2.2 | 1 | 89-89 | `src-pkg/critique.py` | FAIL:tok |
| `skills/hark/engine/hark/timeline/sheet.py` | code | 465 | 41.3 | 3.8 | 3 | 162-165 | `src-pkg/sheet.py` | FAIL:tok |
| `skills/hark/engine/hark/timeline/timeline.py` | code | 299 | 25.4 | 3.6 | 2 | 117-118 | `src-pkg/vocab.py` | pass |
| `skills/hark/engine/hark/timeline/video.py` | code | 149 | 42.8 | 11.2 | 1 | 44-44 | `src-pkg/video.py` | FAIL:tok |
| `skills/hark/engine/pyproject.toml` | code | 37 | 50.0 | 40.0 | 6 | 10-15 | `src-root/pyproject.toml` | FAIL:tok+run |
| `skills/hark/engine/tests/bank/conftest.py` | code | 507 | 24.1 | 4.2 | 2 | 370-372 | `src-pkg/forge/schema.py` | pass |
| `skills/hark/engine/tests/bank/test_bank.py` | code | 340 | 27.1 | 2.5 | 1 | 18-18 | `src-tests/test_bank.py` | pass |
| `skills/hark/engine/tests/bank/test_kit.py` | code | 288 | 23.8 | 1.7 | 1 | 31-31 | `src-tests/test_sfx_kit.py` | pass |
| `skills/hark/engine/tests/bank/test_sfx.py` | code | 192 | 23.5 | 3.8 | 2 | 214-215 | `src-tests/test_realness.py` | pass |
| `skills/hark/engine/tests/conftest.py` | code | 115 | 18.5 | 9.3 | 1 | 37-37 | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/core/conftest.py` | code | 82 | 12.9 | 8.5 | 1 | 22-22 | `src-pkg/report.py` | pass |
| `skills/hark/engine/tests/core/test_api.py` | code | 68 | 11.2 | 6.0 | 0 | - | `src-eval/forge/pro/run_timeline.py` | pass |
| `skills/hark/engine/tests/core/test_audio.py` | code | 207 | 24.4 | 3.1 | 1 | 18-18 | `src-tests/test_core.py` | pass |
| `skills/hark/engine/tests/core/test_cli.py` | code | 256 | 17.9 | 4.1 | 2 | 53-56 | `src-tests/test_core.py` | pass |
| `skills/hark/engine/tests/core/test_config.py` | code | 77 | 11.6 | 1.0 | 1 | 26-26 | `src-pkg/report.py` | pass |
| `skills/hark/engine/tests/core/test_doctor.py` | code | 148 | 16.3 | 3.5 | 1 | 18-18 | `src-pkg/doctor.py` | pass |
| `skills/hark/engine/tests/core/test_features.py` | code | 152 | 22.7 | 2.6 | 0 | - | `src-tests/test_realness.py` | pass |
| `skills/hark/engine/tests/core/test_hub.py` | code | 137 | 15.3 | 1.5 | 1 | 138-138 | `src-pkg/models/hub.py` | pass |
| `skills/hark/engine/tests/core/test_launcher.py` | code | 47 | 12.4 | 5.5 | 0 | - | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/engine/tests/core/test_models.py` | code | 174 | 21.3 | 2.8 | 1 | 19-19 | `src-tests/test_core.py` | pass |
| `skills/hark/engine/tests/core/test_report.py` | code | 87 | 14.9 | 5.0 | 0 | - | `src-pkg/report.py` | pass |
| `skills/hark/engine/tests/fit_analysis/conftest.py` | code | 128 | 21.9 | 4.8 | 2 | 69-70 | `src-tests/test_fit_sines.py` | pass |
| `skills/hark/engine/tests/fit_analysis/test_fit_acceptance.py` | code | 207 | 23.6 | 3.6 | 2 | 23-24 | `src-tests/test_fit_takes.py` | pass |
| `skills/hark/engine/tests/fit_analysis/test_kinds.py` | code | 257 | 31.7 | 3.6 | 2 | 320-321 | `src-tests/test_fit_gesture.py` | FAIL:tok |
| `skills/hark/engine/tests/fit_pipeline/conftest.py` | code | 749 | 36.1 | 1.8 | 2 | 640-641 | `src-pkg/forge/fit/modal.py` | FAIL:tok |
| `skills/hark/engine/tests/fit_pipeline/test_codegen.py` | code | 228 | 26.2 | 3.9 | 1 | 37-37 | `src-tests/test_fit_engine.py` | pass |
| `skills/hark/engine/tests/fit_pipeline/test_engine.py` | code | 259 | 31.3 | 3.1 | 2 | 105-106 | `src-tests/test_fit_engine.py` | FAIL:tok |
| `skills/hark/engine/tests/fit_pipeline/test_quality.py` | code | 107 | 21.2 | 3.4 | 2 | 61-62 | `src-tests/test_fit_spectral.py` | pass |
| `skills/hark/engine/tests/forge/conftest.py` | code | 237 | 26.1 | 5.0 | 2 | 227-228 | `src-eval/forge/frozen_tone.py` | pass |
| `skills/hark/engine/tests/forge/test_bench.py` | code | 234 | 28.2 | 4.2 | 1 | 19-19 | `src-tests/test_forge_refsets.py` | pass |
| `skills/hark/engine/tests/forge/test_blind.py` | code | 172 | 26.5 | 4.5 | 2 | 23-24 | `src-tests/test_core.py` | pass |
| `skills/hark/engine/tests/forge/test_critic.py` | code | 86 | 22.6 | 6.0 | 1 | 16-16 | `src-tests/test_forge_integration.py` | pass |
| `skills/hark/engine/tests/forge/test_dsp_fx.py` | code | 166 | 28.9 | 2.0 | 1 | 23-23 | `src-tests/test_forge_dsp.py` | pass |
| `skills/hark/engine/tests/forge/test_knobs.py` | code | 100 | 6.1 | 2.9 | 2 | 18-19 | `src-pkg/forge/knobs.py` | pass |
| `skills/hark/engine/tests/forge/test_loop.py` | code | 237 | 26.4 | 3.6 | 1 | 90-90 | `src-tests/test_forge_rerender.py` | pass |
| `skills/hark/engine/tests/forge/test_polish.py` | code | 128 | 30.2 | 4.2 | 2 | 159-160 | `src-tests/test_forge_critic.py` | FAIL:tok |
| `skills/hark/engine/tests/forge/test_refsets_viz.py` | code | 69 | 20.2 | 5.7 | 1 | 31-31 | `src-tests/test_forge_refsets.py` | pass |
| `skills/hark/engine/tests/forge/test_registry_tonal.py` | code | 93 | 21.2 | 4.5 | 1 | 53-53 | `src-tests/test_fit_choice.py` | pass |
| `skills/hark/engine/tests/forge/test_schema.py` | code | 74 | 22.9 | 5.9 | 1 | 83-83 | `src-tests/test_forge_rerender.py` | pass |
| `skills/hark/engine/tests/forge/test_search.py` | code | 105 | 29.1 | 4.2 | 1 | 96-96 | `src-tests/test_forge_core_r5.py` | pass |
| `skills/hark/engine/tests/forge/test_tone.py` | code | 83 | 23.8 | 4.1 | 1 | 43-43 | `src-tests/test_fit_choice.py` | pass |
| `skills/hark/engine/tests/forge/tiny_recipe.py` | code | 36 | 19.3 | 11.1 | 2 | 30-31 | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/listen/conftest.py` | code | 200 | 26.0 | 5.4 | 3 | 144-147 | `src-pkg/models/ast.py` | pass |
| `skills/hark/engine/tests/listen/standins.py` | code | 498 | 35.9 | 8.5 | 3 | 105-109 | `src-pkg/features.py` | FAIL:tok |
| `skills/hark/engine/tests/listen/test_calibration.py` | code | 213 | 29.1 | 2.5 | 1 | 96-96 | `src-tests/test_semantics.py` | pass |
| `skills/hark/engine/tests/listen/test_compare.py` | code | 171 | 25.1 | 3.1 | 1 | 29-29 | `src-tests/test_realness.py` | pass |
| `skills/hark/engine/tests/listen/test_describe.py` | code | 253 | 28.4 | 2.5 | 1 | 25-25 | `src-tests/test_semantics.py` | pass |
| `skills/hark/engine/tests/listen/test_eval.py` | code | 301 | 25.3 | 2.7 | 1 | 131-131 | `src-tests/test_semantics.py` | pass |
| `skills/hark/engine/tests/listen/test_find.py` | code | 216 | 29.8 | 2.7 | 1 | 26-26 | `src-tests/test_semantics.py` | pass |
| `skills/hark/engine/tests/listen/test_profile.py` | code | 161 | 27.8 | 3.9 | 2 | 29-30 | `src-tests/test_launch.py` | pass |
| `skills/hark/engine/tests/listen/test_realness.py` | code | 178 | 34.2 | 3.3 | 1 | 18-18 | `src-tests/test_realness.py` | FAIL:tok |
| `skills/hark/engine/tests/recipes_impacts/conftest.py` | code | 75 | 14.1 | 10.8 | 2 | 25-26 | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/recipes_impacts/forge_standins.py` | code | 412 | 53.9 | 12.9 | 6 | 30-39 | `src-pkg/forge/dsp.py` | FAIL:tok+run |
| `skills/hark/engine/tests/recipes_impacts/test_contract.py` | code | 79 | 21.8 | 4.4 | 1 | 24-24 | `src-tests/test_forge_recipes_cut_noise.py` | pass |
| `skills/hark/engine/tests/recipes_impacts/test_hits.py` | code | 158 | 24.2 | 3.3 | 1 | 42-42 | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_impacts/test_impact.py` | code | 79 | 19.0 | 3.5 | 0 | - | `src-tests/test_forge_recipes_sweep.py` | pass |
| `skills/hark/engine/tests/recipes_impacts/test_seeds.py` | code | 56 | 19.1 | 3.1 | 0 | - | `src-tests/test_forge_core_r5.py` | pass |
| `skills/hark/engine/tests/recipes_impacts/test_tuned.py` | code | 174 | 22.1 | 3.1 | 1 | 63-63 | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_motion/conftest.py` | code | 58 | 20.7 | 14.9 | 1 | 37-37 | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_contract.py` | code | 52 | 17.7 | 4.5 | 1 | 14-14 | `src-tests/test_forge_recipes_sweep.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_hover.py` | code | 49 | 15.4 | 4.7 | 0 | - | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_pass.py` | code | 65 | 22.9 | 3.1 | 1 | 13-13 | `src-tests/test_forge_recipes_sweep.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_seeds.py` | code | 51 | 18.8 | 4.6 | 0 | - | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_shimmer.py` | code | 49 | 16.9 | 7.1 | 0 | - | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/recipes_motion/test_zip.py` | code | 35 | 15.9 | 5.6 | 0 | - | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/recipes_risers/conftest.py` | code | 74 | 13.4 | 7.3 | 2 | 25-26 | `src-tests/conftest.py` | pass |
| `skills/hark/engine/tests/recipes_risers/standin_engine.py` | code | 351 | 52.7 | 11.4 | 3 | 20-24 | `src-pkg/forge/dsp.py` | FAIL:tok |
| `skills/hark/engine/tests/recipes_risers/test_contract.py` | code | 94 | 26.3 | 3.3 | 1 | 13-13 | `src-tests/test_forge_recipes_sweep.py` | pass |
| `skills/hark/engine/tests/recipes_risers/test_glitch.py` | code | 70 | 23.3 | 2.0 | 0 | - | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_risers/test_notify.py` | code | 110 | 22.0 | 1.6 | 1 | 25-25 | `src-tests/test_forge_recipes_sweep.py` | pass |
| `skills/hark/engine/tests/recipes_risers/test_seeds.py` | code | 53 | 20.4 | 3.9 | 1 | 58-58 | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/recipes_risers/test_sting.py` | code | 82 | 23.7 | 2.7 | 1 | 39-39 | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_risers/test_sweeps.py` | code | 130 | 26.1 | 2.2 | 1 | 12-12 | `src-tests/test_forge_recipes_cut_noise.py` | pass |
| `skills/hark/engine/tests/recipes_ui/conftest.py` | code | 445 | 52.2 | 14.0 | 6 | 48-55 | `src-pkg/forge/dsp.py` | FAIL:tok+run |
| `skills/hark/engine/tests/recipes_ui/test_ui_click_toggle.py` | code | 157 | 26.6 | 2.7 | 1 | 23-23 | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_ui/test_ui_contract.py` | code | 129 | 21.2 | 3.6 | 1 | 42-42 | `src-tests/test_forge_recipes_cut_noise.py` | pass |
| `skills/hark/engine/tests/recipes_ui/test_ui_keys.py` | code | 136 | 33.2 | 2.6 | 1 | 83-83 | `src-tests/test_forge_recipes_digital.py` | FAIL:tok |
| `skills/hark/engine/tests/recipes_ui/test_ui_pop.py` | code | 170 | 26.0 | 2.3 | 1 | 23-23 | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/recipes_ui/test_ui_seeds.py` | code | 100 | 22.7 | 2.4 | 1 | 59-59 | `src-tests/test_forge_core_r5.py` | pass |
| `skills/hark/engine/tests/recipes_ui/test_ui_tonal_recipes.py` | code | 94 | 20.0 | 4.1 | 0 | - | `src-tests/test_forge_recipes_digital.py` | pass |
| `skills/hark/engine/tests/surfaces/conftest.py` | code | 222 | 24.4 | 5.1 | 1 | 37-37 | `src-pkg/audio.py` | pass |
| `skills/hark/engine/tests/surfaces/test_gallery.py` | code | 325 | 27.2 | 3.0 | 1 | 41-41 | `src-tests/test_gallery.py` | pass |
| `skills/hark/engine/tests/surfaces/test_limits.py` | code | 100 | 17.4 | 3.9 | 1 | 46-46 | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/surfaces/test_llm.py` | code | 247 | 19.1 | 2.8 | 1 | 36-36 | `src-tests/test_interfaces.py` | pass |
| `skills/hark/engine/tests/surfaces/test_mcp.py` | code | 370 | 23.9 | 2.0 | 1 | 37-37 | `src-tests/test_interfaces.py` | pass |
| `skills/hark/engine/tests/timeline/conftest.py` | code | 493 | 31.6 | 3.7 | 2 | 108-109 | `src-pkg/profile.py` | FAIL:tok |
| `skills/hark/engine/tests/timeline/test_critique.py` | code | 223 | 30.9 | 3.3 | 1 | 26-26 | `src-tests/test_launch.py` | FAIL:tok |
| `skills/hark/engine/tests/timeline/test_sheet.py` | code | 86 | 17.4 | 4.5 | 1 | 13-13 | `src-tests/test_forge_round2_fixes.py` | pass |
| `skills/hark/engine/tests/timeline/test_timeline_analysis.py` | code | 249 | 37.1 | 3.8 | 1 | 22-22 | `src-tests/test_timeline.py` | FAIL:tok |
| `skills/hark/engine/tests/timeline/test_video.py` | code | 65 | 20.6 | 9.6 | 2 | 35-36 | `src-tests/test_video_scene_errors.py` | pass |
| `skills/hark/references/forge.md` | prose | 51 | 21.9 | 0.0 | 0 | - | `src-docs/LIMITS.md` | FAIL:tok |
| `skills/hark/references/game-sfx.md` | prose | 60 | 22.6 | 1.8 | 0 | - | `src-skills/sourcing/references/troubleshooting.md` | FAIL:tok |
| `skills/hark/references/kit.md` | prose | 53 | 23.8 | 0.0 | 0 | - | `src-docs/LIMITS.md` | FAIL:tok |
| `skills/hark/references/listening.md` | prose | 78 | 30.3 | 3.0 | 1 | 5-5 | `src-docs/LIMITS.md` | FAIL:tok |
| `skills/hark/references/review.md` | prose | 30 | 23.0 | 2.6 | 1 | 5-5 | `src-skills/sourcing/templates/prompts/critic_director.md` | FAIL:tok |
| `skills/hark/references/setup.md` | prose | 60 | 20.9 | 5.6 | 0 | - | `src-docs/DESIGN.md` | FAIL:tok |
| `skills/hark/scripts/hark` | code | 39 | 6.1 | 0.0 | 0 | - | `src-eval/timeline/tempo_tune.py` | pass |
| `skills/sound-sourcing/SKILL.md` | prose | 119 | 44.7 | 7.5 | 1 | 26-26 | `src-skills/sourcing/SKILL.md` | FAIL:tok |
| `skills/sound-sourcing/references/design-format.md` | prose | 152 | 47.6 | 5.6 | 2 | 78-79 | `src-skills/sourcing/references/design-format.md` | FAIL:tok |
| `skills/sound-sourcing/references/process.md` | prose | 123 | 38.1 | 4.9 | 1 | 90-90 | `src-skills/sourcing/SKILL.md` | FAIL:tok |
| `skills/sound-sourcing/references/rights.md` | prose | 74 | 52.6 | 4.0 | 0 | - | `src-skills/sourcing/references/licences.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-critic-director.md` | prose | 32 | 51.3 | 3.4 | 0 | - | `src-skills/sourcing/templates/prompts/critic_director.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-critic-edges.md` | prose | 30 | 46.0 | 3.5 | 0 | - | `src-skills/sourcing/templates/prompts/critic_edges.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-critic-masking.md` | prose | 32 | 52.8 | 3.4 | 0 | - | `src-skills/sourcing/templates/prompts/critic_masking.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-critic-sync.md` | prose | 31 | 52.2 | 3.4 | 0 | - | `src-skills/sourcing/templates/prompts/critic_sync.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-critic-tonal.md` | prose | 37 | 50.0 | 3.1 | 0 | - | `src-skills/sourcing/templates/prompts/critic_tonal.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-designer.md` | prose | 37 | 61.8 | 5.3 | 0 | - | `src-skills/sourcing/templates/prompts/designer.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-fixer.md` | prose | 38 | 49.2 | 5.2 | 0 | - | `src-skills/sourcing/templates/prompts/fixer.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-music.md` | prose | 41 | 56.2 | 5.0 | 0 | - | `src-skills/sourcing/templates/prompts/music_supervisor.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-rights-audit.md` | prose | 32 | 52.3 | 5.6 | 0 | - | `src-skills/sourcing/templates/prompts/licence_audit.md` | FAIL:tok |
| `skills/sound-sourcing/references/role-sourcing.md` | prose | 40 | 47.2 | 5.1 | 0 | - | `src-skills/sourcing/templates/prompts/sourcing.md` | FAIL:tok |
| `skills/sound-sourcing/references/sound-design.md` | prose | 102 | 64.5 | 1.2 | 0 | - | `src-skills/sourcing/references/sound-design.md` | FAIL:tok |
| `skills/sound-sourcing/references/troubleshooting.md` | prose | 54 | 50.1 | 13.0 | 1 | 1-1 | `src-skills/sourcing/references/troubleshooting.md` | FAIL:tok |
| `skills/sound-sourcing/scripts/.static-analysisrc.json` | code | 25 | 8.8 | 12.8 | 1 | 13-14 | `src-skills/sourcing/templates/sounddesign.json` | pass |
| `skills/sound-sourcing/scripts/analyze.py` | code | 310 | 19.5 | 3.6 | 1 | 1-1 | `src-skills/sourcing/scripts/analyze.py` | pass |
| `skills/sound-sourcing/scripts/arrange.py` | code | 511 | 39.8 | 4.8 | 2 | 96-97 | `src-skills/sourcing/scripts/arrange.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/credits.py` | code | 313 | 37.2 | 4.7 | 1 | 1-1 | `src-skills/sourcing/scripts/credits.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/fixtures.py` | code | 114 | 24.3 | 5.6 | 1 | 17-17 | `src-eval/timeline/mixes.py` | pass |
| `skills/sound-sourcing/scripts/master.py` | code | 347 | 42.9 | 5.0 | 3 | 35-37 | `src-skills/sourcing/scripts/master.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/mix.py` | code | 947 | 47.0 | 3.1 | 2 | 135-136 | `src-skills/sourcing/scripts/mix.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/mux.py` | code | 203 | 26.6 | 4.9 | 1 | 1-1 | `src-pkg/audio.py` | pass |
| `skills/sound-sourcing/scripts/qa.py` | code | 1122 | 42.8 | 1.9 | 2 | 56-57 | `src-skills/sourcing/scripts/qa.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/rights.py` | code | 193 | 29.6 | 5.9 | 1 | 133-133 | `src-skills/sourcing/scripts/licences.py` | pass |
| `skills/sound-sourcing/scripts/rights_audit.py` | code | 621 | 39.7 | 2.0 | 1 | 1-1 | `src-skills/sourcing/scripts/audit.py` | FAIL:tok |
| `skills/sound-sourcing/scripts/variant.py` | code | 527 | 22.7 | 3.4 | 1 | 1-1 | `src-skills/sourcing/scripts/variant.py` | pass |
| `skills/sound-sourcing/templates/SOUND_BRIEF.md` | prose | 60 | 46.5 | 4.7 | 1 | 23-23 | `src-skills/sourcing/templates/SOUND_BRIEF.md` | FAIL:tok |
| `skills/sound-sourcing/templates/arc.json` | code | 7 | 67.5 | 71.4 | 3 | 3-5 | `src-skills/sourcing/templates/arc.json` | FAIL:tok |
| `skills/sound-sourcing/templates/cues.json` | code | 25 | 39.7 | 25.9 | 1 | 3-5 | `src-skills/sourcing/templates/cues.json` | FAIL:tok |
| `skills/sound-sourcing/templates/design_fragment.json` | code | 60 | 43.1 | 34.6 | 3 | 3-6 | `src-skills/sourcing/templates/design_fragment.json` | FAIL:tok |
| `skills/sound-sourcing/templates/manifest_entry.json` | code | 20 | 38.7 | 16.1 | 1 | 4-4 | `src-skills/sourcing/templates/manifest_entry.json` | FAIL:tok |
| `skills/sound-sourcing/templates/rights_entry.json` | code | 17 | 28.5 | 16.7 | 0 | - | `src-skills/sourcing/templates/audit_entry.json` | pass |
| `skills/sound-sourcing/templates/sounddesign.json` | code | 22 | 48.0 | 68.2 | 9 | 4-12 | `src-skills/sourcing/templates/sounddesign.json` | FAIL:tok+run |
