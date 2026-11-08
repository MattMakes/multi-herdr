# hark spec: core audio I/O, features, CLI shell, models and doctor

Unit: **core-io**. Status: behaviour spec, written on the contaminated side from the source at the pinned commit.
A clean implementer builds this unit from this file alone.

Naming rule for this spec:

- The package, the command and the Python module are `hark`.
- Every environment variable of the source is renamed with the prefix `HARK_` (for example `HARK_HOME`,
  `HARK_THREADS`). The third-party variables `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE`, `OPENBLAS_NUM_THREADS` and
  `ONEDNN_MAX_CPU_ISA` keep their names, because other libraries read them.
- Error lines start with `hark <command>: error:`.
- "The source" means the upstream project. Where this spec says "the source does X; hark does Y", hark follows Y.

## Contents

The spec passed 60 KB, so it is split. This index holds sections 1, 6, 7, 10 and 11. The parts hold the rest.
Read all 5 files; together they are the complete spec of this unit.

| Section | File |
|---|---|
| 1. Scope | this file |
| 2. Interface (exact) | `docs/specs/hark/core-io-interface.md` |
| 3. Behaviour | `docs/specs/hark/core-io-behaviour.md` |
| 4. Algorithms | `docs/specs/hark/core-io-algorithms.md` |
| 5. Constants | `docs/specs/hark/core-io-algorithms.md` |
| 6. Optional install | this file |
| 7. Data assets | this file |
| 8. Errors and edge cases | `docs/specs/hark/core-io-tests.md` |
| 9. Acceptance tests | `docs/specs/hark/core-io-tests.md` |
| 10. Dependencies on other units | this file |
| 11. Open questions | this file |

Cross-references such as "4.9" or "AT-15" name a section number or a test id. They are the same in every file.

---

## 1. Scope

This unit owns these parts of hark:

| Part | What it does |
|---|---|
| audio | Decode any media file to float32 at 48 kHz through ffmpeg, cache the decode, probe a file, cut windows, expand paths to media lists, check paths, dB helper. |
| config | Cache root and sub-folders, model ids, model pins, revision overrides, offline flags, CPU thread limit, precision choice. |
| features | The model-free measurements of a sound (`hark measure`): levels, loudness (BS.1770), envelope, spectrum, tonality, pitch, onsets, tempo, beats, key. |
| report | The output convention of every command: summary first, `--json`, `--out`, one-line errors, time formatting. |
| cli | The `hark` command: global flags, lazy command dispatch, BLAS thread hold, error handling, exit codes. |
| package root | `hark.__version__`, lazy top-level functions (`hark.describe(...)` and others), `python -m hark`. |
| api | One flat namespace `hark.api` with every public function of every unit. |
| models.hub | The pinned model files in hark's own Hugging Face cache: resolve, fetch, ensure. Imports nothing heavy. |
| models.clap | The CLAP text-audio embedding model on CPU, with text and audio embedding caches. |
| models.ast | The AudioSet tagger (Audio Spectrogram Transformer) on CPU: 527 class probabilities, with a cache. |
| doctor | `hark doctor`: install check, model cache state, optional model load timing, optional fetch. |
| packaging | The project metadata: Python version, dependencies, extras, console script, package data. |

The command modules of other units (describe, timeline, forge, and so on) plug into the CLI shell of this unit.
Their own flags and outputs are specified in their units. Section 2.4 lists every command and its flags so that the
shell and the help text are complete.

---

## 6. Optional install

Decision (operator): the ML listening layer is optional. Three tiers:

| Tier | Packages | Purpose |
|---|---|---|
| base | numpy, scipy, soundfile, soxr, pillow, matplotlib; ffmpeg and ffprobe on PATH | decode, measure (light), render, audit, images |
| analysis | librosa | onsets, HPSS, pitch, beats, chroma (Q4: base or own tier) |
| ML | torch (CPU wheel), transformers (it brings huggingface_hub) | CLAP and AST |
| mcp | mcp | the MCP server |

Which commands need what (from the source's imports):

| Command and mode | base | librosa | torch + transformers |
|---|---|---|---|
| `measure --light` | yes | no | no |
| `measure` (default, `--music`) | yes | yes | no |
| `sfx list / render / check` | yes | per unit bank-kit-sfx | no |
| `kit init / render / check / audition` | yes | per unit bank-kit-sfx | no |
| `forge --list`, `forge --render`, `forge --check` | yes | per unit forge-core | no |
| `forge <recipe>` (search with the critic) | yes | yes | yes (CLAP; AST for held-out tags) |
| `fit` default | yes | per unit fit | no |
| `fit --clap`, `fit --describe`, `fit --forge` | yes | yes | yes |
| `gallery` | yes | no | no |
| `critique --fast` | yes | yes | no (skips AudioSet and realness) |
| `critique` | yes | yes | yes (AST; CLAP through realness) |
| `profile --no-ast` | yes | yes | no |
| `profile` | yes | yes | yes (AST) |
| `describe`, `match`, `rank`, `find`, `compare`, `ab`, `realness` | yes | yes | yes |
| `timeline`, `sheet` (every detail level tags events with CLAP) | yes | yes | yes |
| `llm` (agy route) | yes | no | only with its timeline option |
| `mcp` | yes + mcp | per tool | per tool; `--warm` loads both models |
| `doctor` | yes | no | no |
| `doctor --load` | yes | no | yes |
| `doctor --fetch` | yes | no | huggingface_hub only (comes with transformers) |

Behaviour when an ML command runs without the ML tier (operator decision, hark-specific):

1. Detect it before loading: torch or transformers cannot be found (import spec missing).
2. When stdin and stdout are a TTY: ask `y/N` whether to install the ML extras now, showing the exact command. On
   `y`, run it, then continue the command. On anything else, exit 1 with the error line.
3. Not a TTY: do not prompt. Exit 1 with one error line that contains the exact install command (torch from the CPU
   wheel index, then transformers). The architect names the command for hark's venv.
4. Model-free commands never import torch, transformers or huggingface_hub. `import hark` and `hark -h` never do.

---

## 7. Data assets

| Asset | Format | Origin | Mark | How a clean implementer gets it |
|---|---|---|---|---|
| CLAP model files at the pin | Hugging Face snapshot: JSON configs, tokenizer files, `model.safetensors` | Public Hub model `laion/larger_clap_general` | DERIVE | Download at runtime by the pinned revision (5.5). Never commit it. |
| AST model files at the pin | Hugging Face snapshot: `config.json`, `preprocessor_config.json`, `model.safetensors` | Public Hub model `MIT/ast-finetuned-audioset-10-10-0.4593` | DERIVE | Same. The 527 AudioSet labels come from the model's own config (id-to-label map); hark does not ship a label list. |
| K-weighting coefficients | 2 biquads | ITU-R BS.1770 (public standard) | DERIVE | Table 5.4. |
| Krumhansl-Kessler key profiles | 2 x 12 numbers | Published music-cognition data | DERIVE | Table 5.4. |
| Third-octave edges, band table | numbers | Definition | DERIVE | Formula 4.16, table 5.2. |
| Precision evidence (bf16 vs fp32 cosine 0.9993, 1.4x) | measurement JSON in the source's eval folder | Source eval run | REGENERATE | Not needed at runtime. To re-check: embed a fixed set of clips in both precisions on a native-bf16 CPU and compare cosines. |
| Doctor data files (vocab, vocab stats, realness model, launch profile) | JSON in package data | Other units | see units listen and timeline | Doctor only checks that they exist. |
| Sound bank files | JSON | Unit bank-kit-sfx | see that unit | Doctor only summarizes them. |

NEEDS-DATA assets in this unit: none.

---

## 10. Dependencies on other units

| Unit | What this unit gives it | What this unit takes from it |
|---|---|---|
| listen | `audio.load` (mono for model input), `list_media`, `check_paths`, `file_id`, `windows`; `features.measure` and `envelope_times`; `clap.get`, `ast.get`; `hub.cache_name`; the report helpers and the expected-error CLI wrapper (3.5) | Command modules `describe`, `match`, `search` (rank, find), `compare` (compare, ab), `realness`, `profile` with their parser registration; the data files vocab, vocab stats, realness model (doctor checks them). |
| timeline | The same audio, features, models, report and `fmt_t` | Command modules `timeline`, `sheet`, `critique`; the launch profile data file. |
| forge-core | `audio`, `features` (envelopes, spectra), models (critic), report | Command module `forge`. |
| recipes-impacts, recipes-motion | Indirect (through forge-core) | Nothing. |
| bank-kit-sfx | `audio`, `features`, report | Command modules `sfx` and `kit`; the bank loader that doctor summarizes (version, entries, entry types, which file each entry came from, the bank folder, `HARK_BANK`). |
| fit | `audio`, `features`, models (only with `--clap`, `--describe`, `--forge`) | Command module `fit`. |
| surfaces | report, CLI common flags, models (MCP warm start), the BLAS hold (the MCP server runs through the CLI) | Command modules `gallery`, `llm`, `mcp`; the LLM critique function for `hark.llm_critique`. |
| skills-and-docs | The command table, install steps, environment variables, doctor output | Nothing. |

Every command module exposes one registration function that adds its parser (or parsers) to the CLI's sub-command
set and binds a handler. Every command parser adds the shared flags `--json`, `--out`, `--threads` through one
helper of the CLI.

---

## 11. Open questions

- Q1: Legacy CLAP snapshot. The source loads an old pickle-only CLAP cache from before it pinned safetensors. hark has
  no such caches. Recommendation: drop the legacy state and the pickle path for hark's own cache; keep pickle loading
  only for a user's local model folder. The architect decides.
- Q2: Doctor health with the ML tier absent. The source treats a missing torch or transformers as a problem. With the
  ML tier optional, recommendation: report it as a note that names the install command, not a problem; `--load` then
  reports the problem.
- Q3: LLM check in doctor. The source reports whether its LLM API key variables are present. hark routes the critique
  through `agy -p` and must never read API keys. Recommendation: report the `agy` path (or null) and, when probe ticket
  P1 passes, whether agy is logged in. Unit surfaces owns the agy contract.
- Q4: librosa, pyloudnorm, scikit-learn. librosa is needed for full `measure`, `critique`, `profile` and the listen and
  timeline units; it pulls numba and llvmlite. pyloudnorm and scikit-learn are declared by the source but not imported
  by the package. Recommendation: librosa in base; drop pyloudnorm and scikit-learn unless another unit spec names
  them.
- Q5: `measure` on an array ignores `start` and `end` (they are only validated). Recommendation: hark applies the range
  to arrays too. This is a behaviour change that no source test covers.
- Q6: (withdrawn.)
- Q7: Cache root in hark's layout. The source puts the cache inside a source checkout when the package's parent folder
  holds a `pyproject.toml`. hark's engine lives at `skills/hark/engine/hark/`. Recommendation: no in-repo cache; always
  `HARK_HOME`, else `~/.cache/hark`, so skill copies and symlinks never write into the repo.
- Q8: Version. The source is at 0.4.1. hark starts its own version line. Recommendation: 0.1.0, with AT-14 binding the
  package, the metadata and the changelog.
- Q9: Command detection skips the value of a global `--threads` (3.5 step 2). This is an improvement over the source
  and changes only speed. Confirm.
