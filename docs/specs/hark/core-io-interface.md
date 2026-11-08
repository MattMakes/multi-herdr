# hark spec: core-io, part: Interface (exact)

Unit: **core-io**. This file is part of the core-io spec. The index is `docs/specs/hark/core-io.md`.
The naming rule of the index applies: package, command and module `hark`; environment variables `HARK_*`.

---

## 2. Interface (exact)

### 2.1 Environment variables

All are read by hark. hark never sets or clears `HF_HUB_OFFLINE` or `TRANSFORMERS_OFFLINE` or `HARK_OFFLINE`.
The CLI sets `OPENBLAS_NUM_THREADS` (rule in 3.5) and `HARK_THREADS` (when `--threads` is given).

| Variable | Default | Meaning |
|---|---|---|
| `HARK_HOME` | see 3.1 | Cache root. Made absolute. Holds `hf/`, `emb/`, `decoded/`. |
| `HARK_THREADS` | `max(1, min(4, cpu_count // 2))`, with `cpu_count` 2 when unknown | torch CPU threads per process. Also the BLAS limit of the CLI. A value below 1 counts as 1. A non-integer value is an error at the first read. |
| `HARK_CLAP` | `laion/larger_clap_general` | CLAP model: a Hugging Face Hub repo id, or a local model folder. |
| `HARK_AST` | `MIT/ast-finetuned-audioset-10-10-0.4593` | AudioSet tagger: a Hub repo id, or a local model folder. |
| `HARK_CLAP_REVISION` | unset | A Hub revision (40-hex commit, branch, tag or PR ref) used instead of the pin. Empty means unset. Applies only when `HARK_CLAP` is the configured model. |
| `HARK_AST_REVISION` | unset | The same for the tagger. |
| `HARK_OFFLINE` | unset | Offline flag (rule in 3.7). |
| `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE` | unset | Same meaning as `HARK_OFFLINE` to hark. |
| `HARK_PRECISION` | `bf16` | `bf16` asks for bfloat16 weights on CPUs with native bf16 support. Any other value (for example `fp32`) forces fp32. Compared in lower case. |
| `HARK_DEBUG` | unset | Any non-empty value: an uncaught exception in a command shows its full traceback instead of the one-line error. |
| `HARK_BANK` | unset | Extra sound-bank files, separated by the OS path separator. Owned by unit bank-kit-sfx. Doctor reports it. |
| `OPENBLAS_NUM_THREADS` | set by the CLI | When the user sets it, the CLI leaves it alone. |
| `ONEDNN_MAX_CPU_ISA` | unset | Read by the bf16 probe (4.12). |

Other units read more variables (`HARK_REFS`, `HARK_REFSETS`, the critique model variable). They are not specified here.

### 2.2 Cache layout

```
<HARK_HOME>/
  hf/                                    Hugging Face hub cache (standard huggingface_hub layout)
    models--<org>--<name>/
      refs/<branch-or-tag-or-pr-ref>     text file: a 40-hex commit
      snapshots/<40-hex commit>/         the files of that revision (symlinks into blobs/ are allowed)
      blobs/
  emb/
    clap/<cache_name>--<precision>/
      text.npz                           text embeddings: key -> (512,) float32
      a_<24 hex>.npy                     one audio embedding (512,) float32
    ast/<cache_name>--<precision>/
      p_<24 hex>.npy                     one class-probability vector (527,) float32
  decoded/
    <20 hex>.npy                         one decoded file: (n,) or (n, 2) float32 at the decode rate
```

`<precision>` is `bf16` or `fp32` (what actually loaded). `<cache_name>` is defined in 4.10.

### 2.3 CLI grammar

```
hark [--threads N] <command> [command args] [--json] [--out FILE] [--threads N]
hark -h | --help
hark <command> -h
python -m hark ...            (same as hark ...)
```

Global and shared flags:

| Flag | Where | Type | Meaning |
|---|---|---|---|
| `--threads N` | before the command, and after it on every command | int | CPU threads for torch and BLAS for this process. `0` means the default. Given in both places, the one after the command wins. |
| `--json` | every command | switch | Print the full result as JSON on stdout instead of the text form. |
| `--out FILE` | every command | path | Also write the full result as JSON to FILE. Parent folders are made. Works with and without `--json`. |
| `-h`, `--help` | everywhere | switch | Help (standard argument-parser help, exit 0). |

Exit codes of the shell:

| Code | When |
|---|---|
| 0 | Success. Also `hark` with no command (prints help). Also `hark doctor` when it finds problems (doctor reports problems in its result, not in its exit code). |
| 1 | A command raised an error. One line on stderr. |
| 2 | Bad command-line usage (argument-parser error), or the module of the requested command cannot be imported. |

A command can return its own non-zero code. Those codes are specified in its unit.

### 2.4 Command table

Order = order in `hark -h`. "Unit" names the unit that specifies the command's behaviour.
The flags listed are the command's own flags; every command also takes `--json`, `--out`, `--threads`.

| Command | Unit | One-line purpose (help text, in hark's own words) | Arguments and flags |
|---|---|---|---|
| `describe` | listen | What a sound sounds like: calibrated tags, AudioSet labels, measurements. | `path`; `--start S`; `--end S`; `--top N` (default 8) |
| `match` | listen | How well a sound fits one or more text descriptions (calibrated z and verdict). | `path`; `query...` (1 or more); `--alts A...`; `--start S`; `--end S` |
| `rank` | listen | Sort many files by how well they fit a description. | `query`; `paths...` (files, folders, globs); `--top N` (default all) |
| `find` | listen | The best moments for a description inside a long recording, as trims. | `path`; `query`; `--top N` (default 5); `--win S`; `--hop S`; `--kind auto\|event\|texture` (default auto); `--start S`; `--end S` |
| `compare` | listen | Nearest reference recordings, typicality, feature differences. | `path`; `--refs P...` (required); `-k N` (default 5); `--start S`; `--end S` |
| `ab` | listen | A/B two sounds or two trims. | `a`; `b`; `--query Q`; `--refs P...`; `--a-start`, `--a-end`, `--b-start`, `--b-end` (seconds) |
| `realness` | listen | Real recording or pro-produced audio versus code-synthesized sound. | `paths...`; `--start S`; `--end S`; `--ast` |
| `timeline` | timeline | Listen through a soundtrack or film: events, sections, loudness, beats, sync. | `file`; `--video V`; `--cues C`; `--from`/`--start S`; `--to`/`--end S`; `--detail low\|normal\|high` (default normal); `--fresh` |
| `sheet` | timeline | An annotated PNG of a soundtrack or film. | `file`; `-o PNG`; `--video V`; `--cues C`; `--from`/`--start`; `--to`/`--end`; `--detail`; `--width PX` (default 2000) |
| `critique` | timeline | Launch-video sound scorecard against a reference profile. | `file`; `--video V`; `--profile P`; `--fast` |
| `profile` | listen | Build a reference profile from the user's own films. | `refs...`; `-o OUT`; `--styles JSON`; `--no-ast` |
| `forge` | forge-core | Make sounds in code and improve them by listening. | see forge-core |
| `fit` | fit | Translate a recording into recipe code and vary it. | see fit |
| `sfx` | bank-kit-sfx | One bank sound by id, aimed at the picture; audit any effect. Sub-commands `list`, `render`, `check`. | see bank-kit-sfx |
| `kit` | bank-kit-sfx | A film's sound kit. Sub-commands `init`, `render`, `check`, `audition`. | see bank-kit-sfx |
| `gallery` | surfaces | One static HTML page of forge results. | see surfaces |
| `measure` | core-io | Objective measurements only, no models. | `files...` (1 or more: files, folders, globs); `--music`; `--light`; `--start S`; `--end S` |
| `llm` | surfaces | Optional audio-LLM critique. In hark this routes through the agy CLI (see decisions). | see surfaces |
| `mcp` | surfaces | Run hark as an MCP server over stdio. | `--warm`; `--tools` |
| `doctor` | core-io | Check the install, models, data files and CPU settings. | `--load`; `--fetch` |

### 2.5 Python API

Package root `hark`:

| Name | Kind | Resolves to |
|---|---|---|
| `hark.__version__` | str | The package version. Must equal the version in the project metadata and in the newest changelog heading. |
| `hark.describe`, `hark.match`, `hark.compare`, `hark.realness`, `hark.timeline`, `hark.sheet`, `hark.critique`, `hark.profile` | callable | The main function of the same-named unit module. `hark.describe(path)` must work whether or not the submodule was imported before. |
| `hark.rank`, `hark.find` | callable | The rank and find functions of the search module (unit listen). |
| `hark.ab` | callable | The A/B function of the compare module (unit listen). |
| `hark.measure` | callable | `features.measure` (this unit). |
| `hark.llm_critique` | callable | The critique function of the LLM module (unit surfaces). |
| any other name | - | A normal submodule import, else `AttributeError`. |

Nothing heavy is imported by `import hark`. Each name imports its module on first access.

Implementation note for the "callable module" rule: in Python, importing a submodule `hark.describe` binds the
module object to the package attribute of the same name, which hides a package-level function. hark must make
`hark.describe(...)` call the function in both cases. One way: each of those modules replaces its own module class
with a subclass whose `__call__` forwards to the module's same-named function. Any approach that passes acceptance
test AT-15 is fine.

`hark.api` (a module): one flat namespace, lazy, `__all__` is the sorted list of these names:

| Name | Target unit | Target |
|---|---|---|
| `describe`, `match`, `rank`, `find`, `compare`, `ab`, `realness`, `realness_many`, `profile` | listen | the same-named functions |
| `timeline`, `sheet`, `critique` | timeline | the same-named functions |
| `measure` | core-io | `features.measure` |
| `doctor` | core-io | `doctor.check` |
| `llm_critique` | surfaces | the LLM critique function |
| `sfx_list`, `sfx_render`, `sfx_check` | bank-kit-sfx | the sfx module functions |
| `kit_init`, `kit_render`, `kit_check`, `kit_audition` | bank-kit-sfx | the kit module functions |

Any other attribute raises `AttributeError`.

Core functions of this unit (signatures exact; defaults exact):

```
hark.audio
  SR = 48000
  class AudioError(RuntimeError)
  load(path, sr=48000, mono=False, start=None, end=None, cache=True) -> float32 ndarray (n, 2) or (n,)
  to_mono(x) -> ndarray (n,)
  resample(x, sr_from, sr_to) -> float32 ndarray
  duration(path) -> float seconds
  probe(path) -> dict
  windows(x, win, hop, sr=48000, pad_last=False) -> list[(t0_seconds, segment)]
  list_media(paths, recursive=True, exts=MEDIA_EXTS) -> list[str]
  check_paths(paths, what='paths') -> None, or raises AudioError
  file_id(path) -> str (20 hex)
  array_id(x) -> str (20 hex)
  replace_quietly(tmp, dst, tries=5) -> bool
  db(v, floor=1e-12) -> float

hark.config
  HOME, HF_CACHE, EMB_CACHE, DEC_CACHE, DATA_DIR, PKG_DIR
  CLAP_MODEL, AST_MODEL, CLAP_REVISION, AST_REVISION, PRECISION
  PINS (table in 5.5), OFFLINE_FLAGS = ('HARK_OFFLINE', 'HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE')
  offline() -> name of the first set flag, or None
  threads() -> int >= 1
  ensure_dirs() -> None

hark.features
  BANDS (table in 5.2)
  measure(src, music=False, start=None, end=None, light=False) -> dict
  envelope_times(m) -> dict
  smooth_env(m, win_ms) -> ndarray (one value per ms)
  frame_rms(m, win) -> ndarray
  true_peak_dbtp(x) -> float
  loudness_curve(x, win=0.4, hop=0.1) -> (t_mid ndarray, lufs ndarray)
  integrated_lufs(x) -> float
  lra(x) -> float or None
  onsets(m, sr=48000, delta=0.07) -> list[float]
  key_estimate(m, sr=48000) -> list[{key, r}] (3 items)
  brightness_hz(m, sr=48000) -> int or None

hark.report
  to_json(result, indent=1) -> str
  render_text(result) -> str
  emit(result, args, text=None) -> result
  error_line(cmd, msg) -> None (prints to stderr)
  fmt_t(t) -> str

hark.models.hub
  resolve(model_id, revision=None) -> Spec
  fetch(spec) -> Spec
  ensure(model_id, revision=None) -> Spec
  slug(model_id) -> str, is_folder(model_id) -> bool, cache_name(model_id, revision=None) -> str
  model_dir(model_id) -> str, snapshot_dir(model_id, revision) -> str or None

hark.models.clap
  get(model_id=None) -> Clap (process singleton)
  Clap.embed_text(texts, batch=64) -> (n, 512) float32
  Clap.embed_audio(arrays, pad='repeatpad', batch=8, per_window=False) -> (n, 512) or list[(k_i, 512)]
  Clap.embed_file(path, start=None, end=None, pad='repeatpad') -> (512,)
  Clap.embed_files(paths, pad='repeatpad', batch=8) -> (n, 512)
  Clap attributes: model_id, spec, revision, weights, precision, dir, logit_scale
  native_bf16(torch) -> bool

hark.models.ast
  get(model_id=None) -> AST (process singleton)
  AST.probs(arrays, reduce='max', batch=8) -> (n, 527) float32
  AST.probs_file(path, start=None, end=None, reduce='max') -> (527,)
  AST.top(p, k=10, min_p=0.0) -> list[(label, p rounded to 3)]
  AST.top_file(path, k=10, start=None, end=None)
  AST.prob_of(p, label) -> float
  AST attributes: model_id, spec, revision, weights, precision, labels (527 str), index (label -> int), dir

hark.doctor
  check(load=False, fetch_missing=False) -> dict
  models() -> dict, fetch() -> dict, bank_info() -> dict
```

`Spec` (from `models.hub`) is an immutable record:

| Field | Type | Meaning |
|---|---|---|
| `model_id` | str | Hub repo id or local folder path as given. |
| `revision` | str | The revision hark wants: the pin, an override, `main`, or `local` for a folder. |
| `state` | str | `cached`, `legacy`, `incomplete` or `missing`. |
| `snapshot` | str or None | The complete local folder to load from (state cached or legacy), else None. |
| `weights` | str | The weights file name that loads, or that a download would get. |
| `missing` | tuple of str | Files of the wanted revision not in the cache. |
| `pinned` | bool | The wanted revision is hark's pin. |
| `folder` | bool | `model_id` names a local folder. |
| `local` (derived) | bool | `snapshot is not None`. |
| `sha` (derived) | str or None | Folder name of the snapshot (its commit) when one loads and it is not a local folder; else None. |
| `label` (derived) | str | Human name: `the local model folder <id>` for a folder; else `<id>@<rev>` where rev is cut to 7 characters when it is a 40-hex commit. |
| `cache_tag`, `cache_name` (derived) | str | See 4.10. |

### 2.6 `hark measure` result (JSON fields)

Every field is optional except `duration`. Short sounds skip what needs length (5.3).
Rounding is part of the interface.

| Field | Unit, rounding | Meaning |
|---|---|---|
| `file` | str | Path as given (CLI only). |
| `summary` | str | Plain-language line (3.10) (CLI only). |
| `duration` | s, 3 dp | Samples / 48000 of the analysed range. |
| `error` | str | `too short to measure` when under 5 ms; then no other field. |
| `peak_dbfs` | dBFS, 1 dp | Sample peak over both channels. |
| `true_peak_dbtp` | dBTP, 1 dp | Peak of the 4x oversampled signal. |
| `rms_dbfs` | dBFS, 1 dp | RMS over all samples of both channels. |
| `lufs` | LUFS, 1 dp | BS.1770 integrated loudness. From 0.4 s. |
| `short_term_max_lufs` | LUFS, 1 dp | Maximum of 3 s blocks, hop 0.5 s. From 3 s. |
| `lra_lu` | LU, 1 dp | Loudness range. From 6 s, and only when enough blocks pass the gates. |
| `clipped_samples` | int | Samples with absolute value at or above 0.999, counted over both channels. |
| `dc` | 5 dp | Mean of the mono mix. |
| `noise_floor_dbfs` | dBFS, 1 dp | 10th percentile of 20 ms frame RMS. |
| `dynamic_range_db` | dB, 1 dp | Loudest 20 ms frame RMS in dB minus the rounded noise floor. |
| `digital_silence_frac` | 0..1, 4 dp | Fraction of mono samples with absolute value under 1e-6. |
| `side_to_mid_db` | dB, 1 dp, or null | Side RMS over mid RMS. Null when the side is under 1e-7 RMS. |
| `correlation` | -1..1, 3 dp | Pearson correlation of L and R. Absent (not null) when either channel is constant. |
| `attack_at` | s, 3 dp | Start of the first event. |
| `peak_at` | s, 3 dp | Time of the overall envelope peak. |
| `attack_ms` | ms (whole), or null | 10% to 90% rise of the first event. |
| `decay_ms` | ms (whole), or null | Peak to 30 dB down that stays down 20 ms. Null when never reached. |
| `centroid_hz` | Hz, int | Power-weighted spectral centroid of the whole file. |
| `rolloff95_hz` | Hz, int | Frequency below which 95% of power lies. |
| `bands_db` | object of 7 numbers, dB 1 dp | Power share per band (5.2), as 10 log10 of the fraction. |
| `bandwidth_hz` | Hz, int, or null | Upper edge of the highest third-octave band within 60 dB of the loudest band. |
| `flatness` | 0..1, 4 dp | Spectral flatness (geometric over arithmetic mean). |
| `brightness_hz` | Hz, int, or null | Median magnitude-weighted centroid over active STFT frames. |
| `tonal_peak_ratio_db` | dB, 1 dp | Strongest spectral bin over the median bin. |
| `harmonic_ratio` | 0..1, 3 dp | Harmonic energy share from HPSS on the first 30 s. Not with `--light`. |
| `f0_hz` | Hz, 1 dp | Median pitch. Only when `harmonic_ratio >= 0.8`. Not with `--light`. |
| `onsets` | list of s, 3 dp | First 200 onset times. Not with `--light`. |
| `onset_count` | int | All onsets. Not with `--light`. |
| `onsets_per_s` | 2 dp | `onset_count / duration`. Not with `--light`. |
| `tempo` | BPM, 1 dp | With `--music`, from 5 s, not with `--light`. |
| `beats` | list of s, 3 dp | Same condition as `tempo`. |
| `beat_ibi_std_ms` | ms, 1 dp | Std of beat intervals. Only when more than 4 beats. |
| `key` | list of 3 `{key, r}` | Best 3 keys, `key` like `C major` or `F# minor`, `r` 3 dp. Same condition as `tempo`. |

Multi-file result: `{"summary": "<N> files measured.", "files": [<one result per file>]}`.
A single file prints its own result object.

### 2.7 `hark doctor` result (JSON fields)

| Field | Meaning |
|---|---|
| `python` | Python version string (for example `3.12.4`). |
| `hark_home` | The cache root. |
| `threads` | `config.threads()`. |
| `precision` | `HARK_PRECISION` as read (lower case), not what loads. |
| `clap_model`, `ast_model` | Configured model ids. |
| `packages` | Object: import name -> installed version, or `ok` when found but without metadata, or `MISSING (<ExceptionType>)`. |
| `ffmpeg`, `ffprobe` | Path on PATH, or a `MISSING ...` string. |
| `torch_cuda_used` | Always `false`. hark never uses a GPU. |
| `offline` | Name of the offline flag that is set, or null. |
| `fetched` | Only with `--fetch`: object model id -> what happened (3.9). |
| `models` | Object model id -> `{revision, cached, loads, weights, snapshot, missing}`. `cached` holds the state word; `loads` holds `Spec.sha`. |
| `models_cached` | Object model id -> bool (a snapshot loads). |
| `agy` | hark-specific: path of the `agy` executable, or null (replaces the source's LLM key check; see 3.9 and Q3). |
| `data_files` | Object data file name -> exists (bool). The list is in 5.6. |
| `bank` | Sound-bank summary (3.9) or `{error}`. |
| `problems` | List of strings. Empty means healthy. |
| `notes` | List of strings. |
| `summary` | Plain-language line (3.9). |
| with `--load`: `clap_load_s` (1 dp), `clap_one_window_s` (2 dp), `clap_precision`, `clap_revision`, `clap_weights`, `ast_load_s` (1 dp), `ast_one_window_s` (2 dp), `ast_revision`, `ast_weights`, `native_bf16` | What loaded and how long it took. |
