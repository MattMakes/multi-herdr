# hark architecture: module map, shared contracts, venv and test plan

Status: fixed by the architect ticket T-12 on 2026-11-07. Side: clean.

This file fixes every name and contract that 2 or more implementers share. 13 implementers build hark in parallel
from it. Read it completely before you read your unit spec.

## 0. How to read this file

1. This file wins over a unit spec for module paths, file paths, function names, signatures, exit codes and data
   locations. The unit spec wins for behaviour, algorithms, constants and result fields.
2. A unit spec names a module by its source-style path (for example `hark.audio`, `hark.forge.dsp`,
   `hark.forge.fit.modal`). Section 2 maps every such name to its hark path. Use the hark path in code.
3. Where a unit spec says "the name is free" or "the architect decides", this file decides. Section 14 lists the
   decisions on open questions of the specs.
4. If you need a change to a contract in this file, send a QUESTION to the orchestrator and end your turn. Do not
   change another unit's paths.
5. "Spec" below means `docs/specs/hark/<unit>.md` and its parts.

## 1. Layout and ownership

Engine root: `skills/hark/engine/` (it holds `pyproject.toml`, `hark/`, `tests/`). Launcher:
`skills/hark/scripts/hark`.

```
skills/hark/engine/
  pyproject.toml                      T-12
  CHANGELOG.md                        T-13 (see 14.1, D-CHANGELOG)
  hark/
    __init__.py                       T-12: version + lazy API hook (section 7)
    __main__.py  cli.py  doctor.py    T-13
    core/        audio.py config.py features.py report.py api.py          T-13
    models/      __init__.py (require_ml) hub.py clap.py ast.py            T-13
    data/listen/     vocab.json vocab_stats.json realness_model.json        T-14
    data/timeline/   launch_profile.json                                    T-15
    listen/      __init__.py vocabulary.py describe.py match.py search.py compare.py
                 realness.py profile.py eval/                               T-14
    timeline/    __init__.py timeline.py sheet.py video.py critique.py     T-15
    forge/       __init__.py                                               T-16
      engine/    __init__.py dsp.py fx.py tone.py polish.py knobs.py schema.py search.py
                 critic.py refsets.py viz.py loop.py cli.py                T-16
      recipes/   __init__.py (registry) tonal.py                           T-16
      recipes/ui/        click keypress typing toggle shutter ui_pop popin pop       17a
      recipes/impacts/   impact cuthit thump knock mallet subdrop drip pluck         17b
      recipes/motion/    whoosh swish swipe brightswish zip hover shimmer            18a
      recipes/risers/    riser darkriser downlifter revcymbal glitch burstglitch
                         notify sting                                                18b
      seeds/<ui|impacts|motion|risers>/<recipe>.json   the owner of that recipe folder
      bank/      __init__.py bank.py sfx.py kit.py __main__.py data/       T-19
      kits/      calm.json crisp.json minimal.json launch.json             T-19
    fit/         __init__.py                                               20b
      analysis/  __init__.py sinusoidal.py modal.py spectral.py gesture.py space.py   20a
      pipeline/  __init__.py auto.py render.py quality.py codegen.py cli.py           20b
    surfaces/    __init__.py gallery.py mcp_server.py llm.py               T-21
  tests/
    conftest.py                       T-12
    core/ listen/ timeline/ forge/ recipes_ui/ recipes_impacts/ recipes_motion/
    recipes_risers/ bank/ fit_analysis/ fit_pipeline/ surfaces/      one per owner above
skills/hark/scripts/hark               T-13
```

Every subpackage folder has an `__init__.py`. Its owner writes it.

Each recipe module is `hark/forge/recipes/<folder>/<name>.py`. Each recipe folder has an empty `__init__.py`, written
by the folder owner.

## 2. Name map from the specs

| spec name | hark module |
|---|---|
| `hark.audio`, `hark.config`, `hark.features`, `hark.report`, `hark.api` | `hark.core.audio`, `hark.core.config`, `hark.core.features`, `hark.core.report`, `hark.core.api` |
| `hark.models.hub`, `.clap`, `.ast`, `hark.doctor`, `hark.cli` | unchanged |
| listen `vocab` module | `hark.listen.vocabulary` |
| listen `describe`, `match`, `compare`, `realness`, `profile` | `hark.listen.<same>` |
| listen "search module" (`rank`, `find`) | `hark.listen.search` |
| `python -m hark.vocab --build-stats` | `python -m hark.listen.vocabulary --build-stats` |
| listen `eval/semantics/run_eval.py` and siblings | `hark/listen/eval/run_eval.py`, `real_effects.py`, `precision.py`, label JSON beside them; run as `python -m hark.listen.eval.run_eval` |
| `hark.timeline` functions, `hark.sheet`, `hark.video`, `hark.critique` | `hark.timeline.timeline`, `hark.timeline.sheet`, `hark.timeline.video`, `hark.timeline.critique` |
| `hark.forge` (package entry: `forge`, `palette`, ...) | implemented in `hark.forge.engine.loop`, re-exported lazily by `hark.forge` (5.3) |
| `hark.forge.dsp`, `.fx`, `.tone`, `.polish`, `.knobs`, `.schema`, `.search`, `.critic`, `.refsets`, `.viz` | `hark.forge.engine.<same>` |
| `hark.forge.recipes._tonal` (forge-core part 1) = recipes-impacts part tonal T1-T15 | `hark.forge.recipes.tonal` (one module, section 8.6) |
| `hark.forge.recipes.<name>` | `hark.forge.recipes.<folder>.<name>`, found by the registry (8.1) |
| `hark/forge/seeds/<name>.json` | `hark/forge/seeds/<folder>/<name>.json` |
| forge-core `eval/forge/*.py` | `hark/forge/engine/eval/` (`bench.py`, `frozen_tone.py`, `build_refsets.py`, `scrub_paths.py`, `blind_v2.py`); T-16; run as `python -m hark.forge.engine.eval.bench` |
| `python -m hark.forge.schema` | `python -m hark.forge.engine.schema` |
| `hark.forge.bank`, `hark.forge.sfx`, `hark.forge.kit` | `hark.forge.bank.bank`, `hark.forge.bank.sfx`, `hark.forge.bank.kit` |
| bank maintenance CLI | `python -m hark.forge.bank` (`hark/forge/bank/__main__.py`) |
| bank files `forge_v2.json`, `forge04.json`, `fit.json`, `pluck.json` | `hark/forge/bank/data/` |
| kit presets | `hark/forge/kits/` |
| `hark.forge.fit.modal`, `.sines`, `.spectral`, `.gesture`, `.space` | `hark.fit.analysis.modal`, `.sinusoidal`, `.spectral`, `.gesture`, `.space` |
| `hark.forge.fit.auto` (fitting side, fit-auto.md) | `hark.fit.pipeline.auto` |
| `hark.forge.fit.auto` (render side, fit-render.md) | `hark.fit.pipeline.render`; `auto` re-exports its public names (9.3) |
| `hark.forge.fit.quality`, `.codegen` | `hark.fit.pipeline.quality`, `hark.fit.pipeline.codegen` |
| `hark.forge.fit` (package API) | `hark.fit` |
| surfaces gallery, MCP module, LLM module | `hark.surfaces.gallery`, `hark.surfaces.mcp_server`, `hark.surfaces.llm` |

## 3. Import rules

### 3.1 Layers

A module may import, at module level, only modules of its own layer or of a lower layer, and only along the edges
of 3.2. Lower number = lower layer.

| layer | modules |
|---|---|
| 0 | `hark` (package root), `hark.core.config`, `hark.models` (the `__init__` only) |
| 1 | `hark.core.audio`, `hark.core.report` |
| 2 | `hark.core.features`, `hark.models.hub` |
| 3 | `hark.models.clap`, `hark.models.ast` |
| 4 | `hark.timeline.video`, `hark.listen.vocabulary` |
| 5 | `hark.listen.describe`, `match`, `search`, `compare`, `realness`, `profile` |
| 6 | `hark.timeline.timeline`, `hark.timeline.sheet`, `hark.timeline.critique` |
| 7 | `hark.forge.engine.dsp`, `fx`, `tone`, `polish`, `schema`, `refsets`, `viz`; `hark.forge.recipes` (registry), `hark.forge.recipes.tonal` |
| 8 | recipe modules `hark.forge.recipes.<folder>.<name>` |
| 9 | `hark.forge.engine.critic`, `search`, `knobs`, `loop`, `cli` |
| 10 | `hark.fit.analysis.*` |
| 11 | `hark.fit.pipeline.*` |
| 12 | `hark.forge.bank.bank`, `sfx`, `kit` |
| 13 | `hark.surfaces.*`, `hark.doctor`, `hark.core.api` |
| 14 | `hark.cli`, `hark.__main__` |

### 3.2 Allowed module-level edges

- core and models: as layered above. `hark.core.features` imports `hark.core.audio`. `hark.models.clap` and
  `hark.models.ast` import `hark.models.hub`, `hark.core.config`, `hark.core.audio`. They import torch and
  transformers only inside functions, after `require_ml` (section 5).
- listen modules import core, models, `hark.listen.vocabulary`, and each other in this order only:
  `compare` -> nothing else of listen except `vocabulary`; `realness` -> `vocabulary`; `profile` -> `realness`,
  `hark.timeline.video`; `describe`, `match`, `search` -> `vocabulary`, `compare`.
- timeline modules import core, models, listen, `hark.timeline.video`. `sheet` and `critique` import
  `hark.timeline.timeline`. `critique` imports `hark.listen.profile` and `hark.listen.realness`.
- forge engine modules of layer 7 import core and each other (`fx` -> `dsp`; `tone`, `polish` -> `dsp`;
  `schema` -> `hark.forge.recipes`; `viz` -> `tone`). They never import listen or models at module level.
- recipe modules import `numpy`, `scipy`, `hark.forge.engine.dsp`, `hark.forge.engine.fx`,
  `hark.forge.recipes.tonal`, and only the other recipe modules named in 8.5.
- layer-9 forge modules import layers 0-8. `critic` imports listen (vocabulary, compare, realness) and models only
  inside functions or behind `require_ml` (the critic is an ML user).
- fit analysis modules import core and the layer-7 forge engine modules. They import each other only along these
  edges: `gesture` -> `sinusoidal`, `modal`, `spectral`; `modal` -> `sinusoidal` (the stereo side model `side_t`);
  `space` -> nothing of fit.
- fit pipeline modules import fit analysis, core, the forge engine and the recipe registry. `quality` loads CLAP and
  `describe` only inside functions.
- bank modules import core, the forge engine, the recipe registry, `hark.fit` (render of fit entries) and each
  other: `sfx` -> `bank`; `kit` -> `bank`, `sfx`.
- surfaces modules import anything below them.
- `hark.cli` imports only the standard library and `hark.core.config` at module level. It imports command modules
  with `importlib` (6.2).

### 3.3 Function-level back-edges

These 4 calls go upward. Each one is an import inside the function body that needs it. No other upward import is
allowed.

| caller | imports inside the function | why |
|---|---|---|
| `hark.forge.engine.knobs` | `hark.forge.bank.bank.estimate_pitch`, `hark.forge.bank.bank.times` | pitch and apex knobs measure a render with the bank's measures |
| `hark.forge.engine.knobs` | `hark.fit.pipeline.auto.kind_of` | the knob tables of a fit entry depend on its model kind |
| `hark.listen.realness` training (`train_model`) | `hark.forge.recipes`, `hark.forge.engine.search.render` | forge renders are the synthesized negatives of the training fallback |
| `hark.core.api`, `hark` root, `hark.timeline` package | the target modules of their lazy tables | lazy API (section 7) |

### 3.4 Package `__init__` rule

A package `__init__.py` imports nothing of hark and nothing heavy at module level. A package that exposes an API
(`hark`, `hark.forge`, `hark.fit`, `hark.timeline`, `hark.forge.recipes`) uses a module-level `__getattr__(name)`
that imports the target on first access. So `import hark.forge.engine.dsp` never loads the critic, and
`import hark.fit.analysis.modal` never loads the fit pipeline.

### 3.5 Heavy and optional imports

| package | rule |
|---|---|
| numpy, scipy, soundfile, soxr | base. Import at module level only in modules that need them. `hark`, `hark.cli`, `hark.core.config` never import them at module level. |
| librosa | base, but slow to import (numba). Import it inside the functions that need it. `measure --light`, `sfx`, `kit`, `forge --render`, `forge --check`, `forge --list` and the recipe renders never import it. |
| matplotlib, PIL | base. Import inside the image functions (`sheet`, `viz`, `gallery`, fit pictures). Use the `Agg` backend. |
| torch, transformers, huggingface_hub | ML extra. Import only inside `hark.models.clap`, `hark.models.ast` and `hark.models.hub` functions, after `require_ml`. `hark.models.hub.fetch` needs huggingface_hub: it calls `require_ml("model download")`. |
| mcp | `mcp` extra. Import only inside `hark.surfaces.mcp_server`. A missing `mcp` is an `MLMissing`-style error with its own command (5.4). |

## 4. Shared types and conventions

### 4.1 Audio buffer

- Type: `numpy.ndarray`. Axis 0 is time. Shape `(n,)` is mono. Shape `(n, 2)` is stereo, column 0 left, column 1
  right. No other channel count crosses a module boundary. A decoder that meets more channels folds them per
  core-io.
- Sample rate: `hark.core.audio.SR = 48000`. Every buffer that crosses a module boundary is at 48000 Hz unless the
  function has an explicit `sr` argument.
- dtype: decoders (`hark.core.audio.load`, `resample`) return `float32`. Recipe renders, fit renders and DSP
  helpers return `float64` unless their spec says otherwise (`hark.fit.analysis.spectral.render` returns float32).
  Every consumer accepts both dtypes.
- Scale: full scale is 1.0. A sample at or above 0.999 in absolute value counts as clipped.
- A function that needs mono and gets stereo uses `hark.core.audio.to_mono`. A consumer that needs stereo and gets
  mono duplicates the channel (`hark.forge.engine.dsp.stereo_of` or `numpy.column_stack`).
- Type alias, for type hints only: `hark.core.audio.Audio = numpy.ndarray`.

### 4.2 Result object

- Every command function and every API function returns a plain `dict[str, Any]` with the key `summary` (a
  plain-language string). An optional key `text` holds the full text output.
- Values are JSON-serialisable after `hark.core.report.clean(result)`: numpy scalars and arrays become Python
  numbers and lists, NaN and infinity become null, `pathlib.Path` becomes `str`, other objects become `str`.
- Times are seconds (3 decimals), levels dBFS / LUFS / dBTP (1 decimal), cosines 3 decimals, z 2 decimals in JSON.
  Each unit spec gives its own rounding.
- A CLI handler builds the result, then calls `hark.core.report.emit(result, args, text=None)` and returns an exit
  code (int or None for 0).

### 4.3 Exceptions

| exception | module | base | meaning |
|---|---|---|---|
| `AudioError` | `hark.core.audio` | `RuntimeError` | decode, probe or path problem |
| `MLMissing` | `hark.models` | `RuntimeError` | an optional extra is missing (section 5) |
| `KnobError` | `hark.forge.engine.knobs` | `ValueError` | a knob the sound cannot take |
| `NotReproducible` | `hark.forge.engine.schema` | `ValueError` | a render block that cannot re-render |
| `UnknownRecipe` | `hark.forge.recipes` | `KeyError` | no recipe of that name |
| `BankError` | `hark.forge.bank.bank` | `ValueError` | bank problem |
| `KitError` | `hark.forge.bank.kit` | `ValueError` | kit problem |
| `ProfileError` | `hark.listen.profile` | `ValueError` | bad profile input (the "input error type" that timeline imports) |
| `ProviderError` | `hark.surfaces.llm` | `RuntimeError` | agy call failed |

Messages are one line. The CLI prints `hark <command>: error: <ExceptionType>: <message>` (core-io 3.5 step 10). The
listen expected-error path prints `hark <command>: error: <message>` (core-io 3.5).

### 4.4 Exit codes

| code | when |
|---|---|
| 0 | success; `hark` with no command; `hark doctor` (also with problems) |
| 1 | a command raised; a check or audit failed (`sfx check`, `kit check`, `bank check`, `forge --check`); `hark llm` result not ok (also no consent); `realness` with an error row |
| 2 | bad command line (argparse); a command module cannot be imported ("not built yet"); `hark sfx` or `hark kit` without a sub-command; `hark forge --render` without a name or params; `hark fit` with no file and no mode; `hark llm` when the agy verdict is DOES-NOT-HEAR |
| 3 | an install is needed and was not done: the ML extras or `mcp` are missing and stdin/stdout is not a TTY, or the user declined the prompt; the launcher finds no venv and does not create it |

This table amends core-io AT-40: the expected exit code is 3, not 1.

### 4.5 Environment variables

| variable | owner | meaning |
|---|---|---|
| `HARK_HOME` | core | cache root; default `~/.cache/hark`; never inside the repo |
| `HARK_THREADS` | core | thread count (core-io 2.1) |
| `HARK_CLAP`, `HARK_AST`, `HARK_CLAP_REVISION`, `HARK_AST_REVISION`, `HARK_PRECISION`, `HARK_OFFLINE` | core | models (core-io 2.1) |
| `HARK_DEBUG` | core | full tracebacks |
| `HARK_PYTHON` | core (launcher) | the python the launcher runs; default `~/.local/state/hark/venv/bin/python` |
| `HARK_REFS` | forge | reference root (default `refs`) |
| `HARK_REFSETS` | forge | refs registry path (default `<HARK_HOME>/refsets.json`) |
| `HARK_REFS_PRO` | forge | pro-study trims folder of the results scrubber |
| `HARK_BANK` | bank | extra bank files |
| `HARK_LLM_MODEL` | surfaces | agy model id override |
| `HARK_EVAL_ASSETS`, `HARK_EVAL_REFS`, `HARK_EVAL_SFX_LIBRARY` | listen eval | eval data roots |
| `HARK_LIVE` | tests | `1` enables tests marked `live` |

No hark code reads, sets or passes `ANTHROPIC_API_KEY` or any other vendor API key.

## 5. Optional ML rule

### 5.1 Interface (`hark.models`, T-13)

```python
class MLMissing(RuntimeError):
    feature: str               # what needed it, for example "describe" or "CLAP"
    missing: tuple[str, ...]   # import names that are absent, for example ("torch",)
    command: str               # the exact install command (5.2)

ML_MODULES: tuple[str, ...] = ("torch", "transformers")

def ml_available() -> bool: ...
def missing_ml() -> tuple[str, ...]: ...          # importlib.util.find_spec; imports nothing
def install_command(extra: str = "ml") -> str: ...
def require_ml(feature: str) -> None: ...          # raises MLMissing when missing_ml() is not empty
def require_extra(extra: str, feature: str) -> None: ...   # "ml" or "mcp"
```

`hark/models/__init__.py` imports only the standard library.

### 5.2 Install commands

`<py>` is `sys.executable` (in the hark venv: `/Users/mascott/.local/state/hark/venv/bin/python`), quoted for the
shell.

| extra | command |
|---|---|
| `ml` | `<py> -m pip install --index-url https://download.pytorch.org/whl/cpu "torch>=2.6" && <py> -m pip install "transformers>=4.40"` |
| `mcp` | `<py> -m pip install "mcp>=2"` |

### 5.3 Who calls it

- `hark.models.clap.get()` and `hark.models.ast.get()` call `require_ml("CLAP")` and `require_ml("AudioSet tagger")`
  first. So every model user is covered.
- The handler of every ML command (core-io section 6 table) calls `require_ml(<command name>)` before it decodes
  audio, so the error comes before slow work.
- Model-free paths never call it and never import torch, transformers or huggingface_hub.

### 5.4 What the CLI does with `MLMissing`

`hark.cli.main` catches `MLMissing` from a handler:

1. stdin and stdout are a TTY: print `hark <command>: <feature> needs the <extra> extras (<missing>).` and the
   command on its own line, then ask `Install them now? [y/N] `. On `y` or `yes` (any case), run the command (split
   at `&&` into argument lists, `subprocess.run`, no shell), then run the handler once more. When the install fails,
   exit 1 with the error line. On any other answer, exit 3.
2. Not a TTY: print one stderr line
   `hark <command>: error: MLMissing: <feature> needs the <extra> extras. Install them with: <command>` and exit 3.
   No prompt.

The MCP server never prompts. A tool that raises `MLMissing` returns a tool error with the same one-line message.
Library callers (`hark.api`) get the exception.

## 6. CLI

### 6.1 Command table

Order = order in `hark -h`. Module = the module that `hark.cli` imports for that command. Function = the library
function the handler calls.

| command | module | function | ML |
|---|---|---|---|
| `describe` | `hark.listen.describe` | `describe` | yes |
| `match` | `hark.listen.match` | `match` | yes |
| `rank` | `hark.listen.search` | `rank` | yes |
| `find` | `hark.listen.search` | `find` | yes |
| `compare` | `hark.listen.compare` | `compare` | yes |
| `ab` | `hark.listen.compare` | `ab` | yes |
| `realness` | `hark.listen.realness` | `realness`, `realness_many` | yes |
| `timeline` | `hark.timeline.timeline` | `timeline` | yes |
| `sheet` | `hark.timeline.sheet` | `sheet` | yes |
| `critique` | `hark.timeline.critique` | `critique` | yes (not with `--fast`) |
| `profile` | `hark.listen.profile` | `profile` | yes (not with `--no-ast`) |
| `forge` | `hark.forge.engine.cli` | `hark.forge.engine.loop.forge`, `render_params`, `hark.forge.engine.schema.main` | search only |
| `fit` | `hark.fit.pipeline.cli` | `hark.fit.run_fit`, `run_morph` | only `--clap`, `--describe`, `--forge` |
| `sfx` | `hark.forge.bank.sfx` | `sfx_list`, `sfx_render`, `sfx_check` | no |
| `kit` | `hark.forge.bank.kit` | `init`, `render`, `check`, `audition` | no |
| `gallery` | `hark.surfaces.gallery` | `gallery` | no |
| `measure` | `hark.core.features` | `measure` | no |
| `llm` | `hark.surfaces.llm` | `critique` | only `--with-timeline` |
| `mcp` | `hark.surfaces.mcp_server` | `serve` | per tool |
| `doctor` | `hark.doctor` | `check` | only `--load` |

`hark.cli.COMMANDS: tuple[tuple[str, str], ...]` holds `(command, module)` in this order.

### 6.2 Registration contract

- Every command module defines `add_cli(sub: argparse._SubParsersAction) -> None`. It adds its parser (or both
  parsers, for `hark.listen.search` and `hark.listen.compare`), calls `hark.core.report.add_common(parser)` on each,
  and binds `parser.set_defaults(func=<handler>)`.
- A handler is `def handler(args: argparse.Namespace) -> int | None`.
- `add_cli` must not import heavy modules. Import them inside the handler.
- Sub-command groups (`sfx`, `kit`) add a nested sub-parser in `add_cli`. Their `--out` meaning follows
  bank-kit-sfx 3.2.
- When the command module itself is absent (`ModuleNotFoundError` whose `name` is that module or a parent of it),
  the CLI prints `hark: the "<command>" command is not built yet (module <module> is missing)` and exits 2. Any other
  import error prints the core-io 3.5 step 3 line and exits 2.

### 6.3 Shared helpers (`hark.core.report`, T-13)

```python
def add_common(parser: argparse.ArgumentParser) -> None   # --json, --out FILE, --threads N
def emit(result: dict, args: argparse.Namespace, text: str | None = None) -> dict
def clean(obj: Any) -> Any
def to_json(result: dict, indent: int = 1) -> str
def render_text(result: dict) -> str
def error_line(cmd: str, msg: str) -> None
def fmt_t(t: float | None) -> str
def expected_errors(cmd: str) -> Callable[[Handler], Handler]   # decorator: listen expected-error path
```

## 7. Python API

- `hark/__init__.py` (T-12) holds `__version__ = "0.1.0"` and a module `__getattr__` that forwards every name in
  `hark.core.api.__all__` (plus `llm_critique`) to `hark.core.api`. Any other name is a normal submodule import, else
  `AttributeError`. It imports nothing at module level.
- `hark.core.api` (T-13) is the lazy flat namespace of core-io 2.5. Its name -> target table:

| name | target |
|---|---|
| `describe` | `hark.listen.describe.describe` |
| `match` | `hark.listen.match.match` |
| `rank`, `find` | `hark.listen.search.rank`, `.find` |
| `compare`, `ab` | `hark.listen.compare.compare`, `.ab` |
| `realness`, `realness_many` | `hark.listen.realness.realness`, `.realness_many` |
| `profile` | `hark.listen.profile.profile` |
| `timeline` | `hark.timeline.timeline.timeline` |
| `sheet` | `hark.timeline.sheet.sheet` |
| `critique` | `hark.timeline.critique.critique` |
| `measure` | `hark.core.features.measure` |
| `doctor` | `hark.doctor.check` |
| `llm_critique` | `hark.surfaces.llm.critique` |
| `sfx_list`, `sfx_render`, `sfx_check` | `hark.forge.bank.sfx.<same>` |
| `kit_init`, `kit_render`, `kit_check`, `kit_audition` | `hark.forge.bank.kit.init`, `.render`, `.check`, `.audition` |

- Name clash: `hark.timeline` is both a package and an API name. `hark/timeline/__init__.py` (T-15) replaces its own
  module class with a subclass whose `__call__(*a, **kw)` imports `hark.timeline.timeline` and calls its
  `timeline` function. So `hark.timeline(path)` works whether or not the package was imported first. No other API
  name is a package, so no other module needs this.
- core-io AT-15 reads, in hark: `import hark; hark.measure(x)`, then `import hark.listen.describe` and
  `hark.describe(path)`, then `import hark.timeline` and `hark.timeline(path)`.

## 8. Forge contracts

### 8.1 Recipe registry (`hark.forge.recipes`, T-16)

```python
FOLDERS: tuple[str, ...] = ("ui", "impacts", "motion", "risers")

class UnknownRecipe(KeyError): ...

def names() -> list[str]: ...                 # sorted flat recipe names; scans folders with pkgutil, imports nothing
def folder_of(name: str) -> str: ...          # "motion" for "whoosh"; UnknownRecipe
def module_path(name: str) -> str: ...        # "hark.forge.recipes.motion.whoosh"
def load(name: str) -> types.ModuleType: ...  # importlib.import_module(module_path(name))
def short_name(obj: object) -> str: ...       # flat name of a recipe module, recipe object, or render function
def seed_path(name: str) -> pathlib.Path: ... # hark/forge/seeds/<folder>/<name>.json (may not exist)
```

- A recipe name is unique across the 4 folders. Two modules with one name make `names()` raise `RuntimeError`.
- Modules whose name starts with `_` are helpers and are not recipes. `tonal.py` sits beside the folders, so it is
  never a recipe.
- `hark.forge.recipes()` and `hark.forge.load_recipe(name)` of forge-core part 1 call `names()` and `load(name)`.
- `short_name` of a module is its last dotted component. Of a recipe object it is `obj.__name__` or `obj.MODULE`
  last component (8.4). Of a function it is the last component of `fn.__module__` when that module is under
  `hark.forge.recipes.<folder>.`, else `None`.
- Render blocks (forge-core part 6) and bank entries store the flat name in `recipe`. `schema.resolve(block)` calls
  `load(block["recipe"])` and takes `getattr(module, block.get("fn", "render"))`. `schema.render_ref(fn)` returns
  `(short_name(fn), fn.__name__)`.
- Seed banks: `search` reads and writes `seed_path(name)`. The file format is forge-core part 7 "Seed bank". Writing
  a seed file inside the package happens only in maintenance runs (`hark forge` with `--dir` and bank on); a run with
  `--no-bank` never writes it.

### 8.2 Recipe contract

A recipe is a module (or an object with the same attributes) that has:

| attribute | type | required | meaning |
|---|---|---|---|
| `SPACE` | `dict[str, ParamSpec]` | yes | search space (8.3) |
| `DEFAULTS` | `dict[str, float \| int \| str]` | yes | a complete good parameter set |
| `TARGET` | `str` | yes | critic target text |
| `ALTS` | `list[str]` | yes | confusion texts |
| `render` | `Callable[[dict, numpy.random.Generator], ndarray]` | yes | one take: float64 at 48000 Hz, `(n,)` or `(n, 2)`; deterministic for `p` and a generator from one seed; reads no file |
| `REFSET` | `str` | no | default refset category |
| `REFS` | `list[str]` | no | reference paths under `HARK_REFS` |
| `TARGETS` | `dict[str, str]` | no | category -> target text |
| `FAMILIES` | `dict[str, list[tuple[str, dict]]]` | no | named members per category (bank palette form) |
| `FAMILY`, `FAMILY_NAMES`, `FINE` | see recipes-impacts 1.1 | no | tonal palette form |
| hooks `measure(x)`, `target_check(x, row=None)`, `study_extra(x, row=None)`, `generic_extra(x, row=None)`, `extra_for(cat, weight=1.0)`, `pro_penalty(x, cat)`, `family_space(...)`, `family_params(...)`, `local_space(...)`, `space_for(cat)`, `recipe(...)`, `recipe_for(cat, preset=0)`, `preset(...)`, `preset_full(cat, params)` | | no | as the recipe parts define them |

- The generator is `hark.forge.engine.dsp.rng(seed) = numpy.random.default_rng(seed & 0xFFFFFFFF)`.
- The forge wrapper `hark.forge.engine.search.render(recipe, params, seed) -> ndarray | None` normalises to -1 dBFS
  and returns None for an empty, non-finite or silent (peak under 1e-7) render. Recipes do not normalise to -1 dBFS
  themselves.
- Members outside `SPACE` must render as given. `render` never clips parameters to `SPACE`.
- A recipe object (from `recipe(...)`, `recipe_for(...)`, `variant_recipe(...)`, or the pop v2, typed-run and
  chime-run objects) is a `types.SimpleNamespace` with the required attributes plus `MODULE` (the dotted module path
  of the recipe that owns its seed bank) and, when it stands for a member, `PRESET`.

### 8.3 Parameter schema

`ParamSpec` is one of:

| form | meaning | unit coordinate u in [0, 1] |
|---|---|---|
| `list` of choices | pick one; a 1-element list pins the value | index / (len - 1) |
| `(lo, hi)` or `(lo, hi, "lin")` | float range | (v - lo) / (hi - lo) |
| `(lo, hi, "log")` | float range on a log scale, `lo > 0` | log(v / lo) / log(hi / lo) |
| `(lo, hi, "int")` | inclusive integer range | (v - lo) / (hi - lo), decoded with rounding |

`hark.forge.engine.search.encode(space, params) -> numpy.ndarray` and `decode(space, u) -> dict` are the only
converters. Forge-core part 7 gives the exact mapping. Fit knob tables use the same tuple forms.

### 8.4 Recipe ownership

| folder | recipes | owner | spec part |
|---|---|---|---|
| `ui` | click, keypress, typing, toggle, shutter, ui_pop, popin, pop | 17a | recipes-impacts: ui, keys, pop |
| `impacts` | impact, cuthit, thump, knock, mallet, subdrop, drip, pluck | 17b | recipes-impacts: impact, hits, tuned |
| `motion` | whoosh, swish, swipe, brightswish, zip, hover, shimmer | 18a | recipes-motion: pass, sweep (zip), tonal (shimmer, hover) |
| `risers` | riser, darkriser, downlifter, revcymbal, glitch, burstglitch, notify, sting | 18b | recipes-motion: sweep, glitch, tonal (notify), sting |

The seed file of a recipe lives in `hark/forge/seeds/<its folder>/`, owned by the folder owner.

### 8.5 Cross-folder recipe imports

These module-level imports are allowed. Each provider must export exactly these names with the spec behaviour.
No other recipe-to-recipe import is allowed. The import graph has no cycle.

| importer | provider | names | provider spec |
|---|---|---|---|
| `impacts/thump` | `motion/whoosh` | `render`, `OPTIONAL`, `BASES`, `SPACE`, `preset`, `envelope` | recipes-motion-pass 1.2 |
| `motion/swish`, `motion/swipe`, `motion/brightswish` | `motion/whoosh` | as needed from 1.2 | same folder |
| `risers/sting` | `impacts/impact` | `hall_ir(rt60, damp, size, predelay, seconds=None)`, `measure`, `study_extra`, `study_penalty`, `FAMILIES`, `render` | recipes-impacts-impact 2.3, 4.5-4.8 |
| `risers/sting` | `impacts/subdrop` | `FAMILIES`, `render` | recipes-impacts-hits |
| `motion/zip` | `risers/riser` | `lvl`, `phase_from_end`, `gate_shape`, `unison`, `verb_ir`, `wet_of`, `stereo_of`, `env_db`, `band_env_db`, `side_db`, `low_share_db`, `centroid_track` | recipes-motion-sweep 5.10 |
| `risers/downlifter` | `risers/riser` | same as above | same folder |
| `motion/shimmer` | `risers/notify` | `measure`, `outside(v, lo, hi, s)` | recipes-motion-tonal 12.11, 12.12 |
| `ui/pop` | `risers/notify` | `measure` | recipes-motion-tonal 12.11 |
| `ui/ui_pop`, `ui/shutter`, `impacts/knock`, `impacts/mallet`, `impacts/drip` | `hark.forge.recipes.tonal` | 8.6 | forge-core-loop, recipes-impacts-tonal |

Rules:

- `motion/hover` restates the click layer helpers it needs (recipes-motion-tonal 14.5). It does not import
  `ui/click`.
- `motion/brightswish` builds its comb with P4 of the recipes-motion index. It does not import `impacts/impact`.
- `risers/sting` selects the impact member by name: `dict(FAMILIES["impact"])["punch"]` (recipes-motion Q1). 17b
  keeps `FAMILIES` as the final (version 3) families, and `FAMILIES["impact"]` contains a member named `punch`. The
  subdrop members are `FAMILIES["sub_drop"]` member `808` and `FAMILIES["pro_sub_drop"]` member `ref`.
- `FAMILIES[cat]` is always a list of `(name, full params)` pairs.
- A test of an importer whose provider may not be built yet starts with
  `pytest.importorskip("hark.forge.recipes.<folder>.<provider>")`. Integration (25b) runs them when all exist.

### 8.6 Tonal helpers (`hark.forge.recipes.tonal`, T-16)

T-16 builds this module from forge-core-loop "Algorithms: tonal building blocks" and recipes-impacts-tonal T1-T15.
Exact names:

| name | signature | recipes-impacts-tonal |
|---|---|---|
| `SR` | `48000` | |
| `env` | `env(t, onset, tau, hold=0, curve=1, dcurve=1)` | T1 |
| `two_rate` | `two_rate(t, onset, fast, tf, ts, curve=1)` | T2 |
| `glide` | `glide(t, f0, bend_st=0, ptau=0.02)` | T3 |
| `rise_track` | `rise_track(t, f0, rise_st, rtau)` | T4 |
| `nyquist_fade` | `nyquist_fade(f)` (gain per partial) | T5 |
| `tone` | `tone(f, r, parts=(), e=None)` | T6 |
| `follow_noise` | `follow_noise(n, r, fc, q, e)` | T7 |
| `burst` | `burst(n, r, lo, hi, tau, rise=0.0003, order=2)` | T8 |
| `thump` | `thump(n, f, tau, rise=0.002)` | T9 |
| `modes` | `modes(n, r, freqs, amps, taus, jitter=0, bend=0, bend_tau=0.004)` | T10 |
| `strike` | `strike(y, r, contact_s, bright=0.5)` | T11 |
| `room` | `room(y, r, wet, rt60, size=0.15, width=0.3)` | T12 |
| `finish` | `finish(y, r, hp=80, lp=12000, room_wet=0, rt60=0.15, width=0, cut=500, end_db=-60, fade=0.006, pan=0, size=0.15)` | T13 |
| `measure` | `measure(x) -> dict` | T14 |
| `outside` | `outside(m, targets) -> float` | T15 |
| `family_space` | `family_space(space, params, fine) -> dict` | T15 |

### 8.7 Forge engine API (T-16)

- `hark.forge` re-exports lazily from `hark.forge.engine.loop`: `forge`, `family_box`, `palette`, `palette_md`,
  `variant_recipe`, `pick_refs`, `resolve_refs`, `render_block`, `render_params`, `audition`, `report_md`,
  `SCORE_KEYS`, and `recipes()` / `load_recipe(name)` (8.1). Signatures: forge-core-loop "Module hark.forge".
- `hark.forge.engine.dsp` public names (forge-core-dsp): `rng n_of t_of noise env_exp env_curve contact modal
  modal_cloud excite butter peq svf room_ir room mic stereo fade place trim_tail normalize curve sweep osc shepard
  wander air sub_drop BELLS bell fm onset align pan_to_stereo stack xfade`, plus `stereo_of(x)` (mono to 2 equal
  channels; stereo unchanged).
- `hark.forge.engine.fx` public names: `IR_CACHE_MB ir_cache_stats ir_cache_clear ir_grid early reverb_ir reverb
  saturate transient limit split match_curve eq_bands eq_match doppler decorrelate pan width sweep_filter varispeed
  tape_stop crush stutter glitch granular reverse_swell`.
- `hark.forge.engine.knobs`: forge-core-knobs Interface, unchanged names. `apply(entry, run, ...)` calls
  `run(edit)` or `run(edit, fit_knobs)`. The bank renderer accepts both (bank-kit-sfx part 1, 5.1).
- `hark.forge.engine.schema`: forge-core-schema Interface, unchanged names, with `resolve` per 8.1.
- `hark.forge.engine.critic`, `search`, `tone`, `polish`, `refsets`, `viz`: their part Interfaces, unchanged names.
- `hark.forge.engine.cli.add_cli(sub)` registers `forge`.
- K-weighting: `hark.core.features.k_weight(x: ndarray, sr: int = 48000) -> ndarray` is the one BS.1770 K filter.
  `polish`, `timeline`, `gallery` and fit use it. They do not keep a second copy of the coefficients.

### 8.8 What the forge needs from listen (T-14 provides, exact)

`hark.listen.vocabulary`:

```python
class Calibration(NamedTuple):
    C: ndarray        # (n, T) term cosines
    U: ndarray        # (n, T) bias-free term scores
    Z: ndarray        # (n, T) term z
    mu: ndarray       # (n,)
    sigma: ndarray    # (n,)
    Cq: ndarray | None      # (n, k) query cosines
    z_q: ndarray | None     # (n, k) query z
    percentile: ndarray | None   # (n, k), 0..1
    rank: ndarray | None         # (n, k), from 1

class Vocab:
    terms: list[str]; categories: list[str]; term_category: list[str]
    templates: list[str]; E: ndarray; logit_scale: float; model_id: str
    def embed_text(self, texts: Sequence[str]) -> ndarray: ...                       # (k, 512), templated, unit rows
    def calibrate(self, A: ndarray, Q: ndarray | None = None, method: str = "both") -> Calibration: ...
    def scores(self, A: ndarray, queries: Sequence[str]) -> list[list[dict]]: ...   # A7 records, per sound
    def top_terms(self, a: ndarray, k: int, cats: Sequence[str] | None = None,
                  exclude: Sequence[str] = ()) -> list[tuple[str, str, float, float]]: ...
    def nearest_category(self, query: str) -> str: ...
    def bias(self, Q: ndarray) -> ndarray: ...

def get() -> Vocab: ...                     # process singleton, thread-safe
def embed_files(paths: Sequence[str], pad: str = "repeatpad", max_win: int | None = None,
                ranges: Sequence[tuple[float | None, float | None]] | None = None) -> ndarray: ...
def embed_file(path: str, start: float | None = None, end: float | None = None, pad: str = "repeatpad") -> ndarray: ...
def embed_audio(arrays: Sequence[ndarray], pad: str = "repeatpad") -> ndarray: ...
def verdict(z: float) -> str: ...
VERDICT_THRESHOLDS_Z: dict[str, float]      # {"strong": 3.0, "plausible": 2.5, "weak": 2.0}
VERDICT_EVIDENCE: dict[str, dict]
PAD = "repeatpad"
def zf(z: float) -> str: ...
UNVALIDATED = ("music_genre", "music_mood", "instruments", "voice")
```

`hark.listen.compare`: `feats(measurements: dict, audio: ndarray) -> dict` (listen-compare "Trim-proof
measurements") and `typicality(a: ndarray, refs: ndarray, k: int = 5) -> dict` (`{mean_cos_knn, z, percentile, k,
verdict}`).

`hark.listen.realness`: `load_model(path=None) -> dict`, `thresholds_of(model, file=False) -> dict` (`{lo, hi}`),
`head_logit(head, v) -> float`, `fuse(model, logits) -> float`, `pure_tone(x) -> bool`,
`signal_features(x) -> dict`, `audible(x) -> bool`, `realness(path, start=None, end=None, ast=False, cache=True)`,
`realness_many(paths, ast=False, start=None, end=None)`, `MODEL_PATH`.

`hark.listen.profile`: `profile(paths, styles=None, use_ast=True, out=None)`, `film_metrics(path, video=None,
use_ast=True)`, `load_profile(path=None)` (default: `hark/data/timeline/launch_profile.json`), `aggregate(films)`,
`media_problem(path)`, `unscorable(metrics)`, `fmt_value(key, v)`, `METRICS` (the metric table), `SHAPE_METRICS`,
`MIN_FILM_S = 10.0`, `MIN_SHAPE_S = 3.0`, `AST_WIN_S = 5.0`, `MUSIC_P = 0.3`, `ProfileError`.

## 9. Fit contracts

### 9.1 Model

A model is a plain JSON dict with `kind`, `version` and `sr = 48000`. Kind values and versions: fit.md "Model
kinds". Every analysis module owns its kind's model fields (its part).

### 9.2 Analysis interface (`hark.fit.analysis`, 20a)

Each module keeps the exact public names and signatures of its part Interface (fit-modal, fit-sines, fit-spectral,
fit-gesture, fit-space). In addition, every kind module (`sinusoidal`, `modal`, `spectral`, `gesture`) exports this
uniform face, which the pipeline uses:

```python
KIND: str                 # "sines", "modal", "spectral_noise", "gesture" (the model's kind value)
NAME: str                 # "sines", "modal", "spectral", "gesture" (the kind_of name)
VERSION: int              # 1
KNOBS: dict[str, tuple[ParamSpec, float, str]]   # knob -> (search range, identity, description)
def fit(x: str | ndarray, **kw) -> dict: ...
def render(model: dict, seed: int = 0, **knobs) -> ndarray: ...   # raw kind render, no post stage
def check(model: dict) -> None: ...                                # ValueError on wrong kind or version
def summary(model: dict) -> str | dict: ...
def save(model: dict, path: str) -> None: ...
def load(path: str) -> dict: ...
```

- `KNOBS` holds the kind's knob table of fit-render.md "Knob tables" (gesture: fit-gesture). 20a types the tables
  in. 20b reads them and never keeps a second copy.
- `sinusoidal` also exports `analyze`, `onsets`, `side_model`, `morph_side`, `vary_field`, `finish`, `hf_rolloff`,
  `synth_partials`, `synth_atoms`, `synth_noise`, `partial_freq`, `fade_in_s`, `band_masks`, `describe`, `to_code`.
- `modal` also exports `render_model`, `onsets`, `onset_strength`, `fade_out`, `esprit`, `ls_modes`,
  `band_masks`, `band_centres`, `to_code`, `mel_distance`, `env_corr`, `rise_ms`.
- `spectral` also exports `DETAIL`, `trajectory`, `bend`, `pass_pan`, `analyse`, `centers`, `warp`, `unwarp`,
  `weights`, `bandwidths`, `n_frames`, `compare`, `fit_report`, `to_code`. Its `to_code` output imports
  `hark.fit.analysis.spectral`.
- `gesture` also exports `air`, `residual`, `gain_tones`.
- `space` exports its part Interface (`fit_space`, `render_space`, `is_stereo`, `needs_space`, `tail_corr`, `corr`,
  `side_powers`, `diffuse_power`, `band_powers`, `active_span`, `tail_span`, `shaped_side`, `impulse`, `wet`,
  `tail_s`, `describe`). It has no `KIND` face: a space model is a layer, not a kind.
- `hark/fit/analysis/__init__.py` holds `KINDS: dict[str, str]` = `{"sines": "hark.fit.analysis.sinusoidal",
  "modal": "hark.fit.analysis.modal", "spectral_noise": "hark.fit.analysis.spectral", "gesture":
  "hark.fit.analysis.gesture"}` and `module_for(kind: str) -> ModuleType` (lazy import; `ValueError` for an unknown
  kind). It imports nothing at module level.
- Analysis modules never import `hark.fit.pipeline` or `hark.fit` (the package API).

### 9.3 Pipeline interface (`hark.fit`, `hark.fit.pipeline`, 20b)

`hark.fit.pipeline.auto` exports: `fit(x, kind="auto", **kw) -> dict`, `fit_kind(x, name, **kw) -> dict`,
`kind_of(model) -> str`, `knobs_of(model) -> dict`, `identity(model) -> dict`, and re-exports from
`hark.fit.pipeline.render`: `render(model, seed=0, take=None, bed=True, **knobs) -> ndarray`, `morph(a, b,
amount=0.5) -> dict`, `morph_model`, `finish`. `kind_of` accepts `hybrid` and `morph` too and raises `ValueError`
for any other kind.

`hark.fit.pipeline.quality`: fit-quality Interface. `hark.fit.pipeline.codegen`: fit-codegen Interface
(`to_recipe`, `write_recipe`, `load_recipe`). `hark.fit.pipeline.cli.add_cli(sub)` registers `fit`.

`hark.fit` (package API, lazy): `fit`, `render`, `morph`, `to_recipe`, `run_fit`, `run_morph`, `kind_of`, `load`,
`save`.

A generated recipe module imports only `numpy` and `hark.fit` (it calls `hark.fit.render`). It follows 8.2. It is
not under `hark.forge.recipes`, so the registry does not list it. `hark fit --forge recipe.py` loads it by path.

Bank use (bank-kit-sfx part 1, 5.1): `hark.fit.render(model, seed, vary=v, take=True, bed=False, **fit_knobs)`.

## 10. Other unit contracts

### 10.1 core (T-13)

core-io 2.5 is exact, with the module paths of section 2, plus:

```python
# hark.core.audio
Audio = numpy.ndarray
AUDIO_EXTS: frozenset[str]; VIDEO_EXTS: frozenset[str]; MEDIA_EXTS: frozenset[str]
def write_wav(path: str, x: ndarray, sr: int = 48000, subtype: str = "PCM_24") -> str: ...  # parents made; returns path
# hark.core.features
def k_weight(x: ndarray, sr: int = 48000) -> ndarray: ...
def add_cli(sub) -> None: ...    # registers "measure"
# hark.core.config
DATA_DIR = <hark package>/data
VENV_PYTHON_DEFAULT = "~/.local/state/hark/venv/bin/python"  (expanded)
```

Doctor data files (core-io 5.6), relative to `DATA_DIR`: `listen/vocab.json`, `listen/vocab_stats.json`,
`listen/realness_model.json`, `timeline/launch_profile.json`. Doctor bank summary calls
`hark.forge.bank.bank.VERSION`, `ids()`, and reads `HARK_BANK`; a missing bank module gives `{error}`.

### 10.2 listen (T-14)

The index Interface of listen.md is exact, with section 2 paths and 8.8 names. `match` lives in
`hark.listen.match` and T-14 builds it. Its summary sentence and texts follow surfaces-mcp M3 and M5. T-21 does not
build a second `match`. The MCP `match` tool calls `hark.listen.match.match`.

### 10.3 timeline (T-15)

timeline.md 1.2 is exact, with section 2 paths: `hark.timeline.timeline.{timeline, detect_events, level_envs,
level_peak, loudness, music_grid, read_cues, sync, top_accents, novelty_bounds}`, `hark.timeline.sheet.sheet`,
`hark.timeline.video.{info, has_video, frame_diff, diff_events, cuts, frame, frames}`,
`hark.timeline.critique.{critique, scorecard, real_thresholds}`. `hark.timeline.video` imports only core (layer 4),
because `hark.listen.profile` imports it.

### 10.4 bank, sfx, kit (T-19)

bank-kit-sfx 3.5 is exact, with section 2 paths. Fixes:

- `estimate_pitch(y, sr=48000, near_hz=None, span_st=None) -> dict | None` and `times(y, sr=48000) -> dict`. The
  knob engine calls `estimate_pitch(x, near_hz=target_hz, span_st=s)` and `times(x)["peak_s"]`.
- `renderer(entry, seed=None)` returns `run(edit=None, fit_knobs=None)`.
- A forge entry renders through `hark.forge.recipes.load(entry["recipe"])` and `getattr(module, entry["fn"])`.
- `hark.forge.bank.sfx.add_cli(sub)` registers `sfx`. `hark.forge.bank.kit.add_cli(sub)` registers `kit`.
- `python -m hark.forge.bank build|add-fit|check` is the maintenance CLI, plus `build --derive` (11.2).

### 10.5 surfaces (T-21)

- `hark.surfaces.gallery.gallery(...)`: surfaces.md 1.1. `add_cli` registers `gallery`.
- `hark.surfaces.mcp_server`: `TOOLS` (the table of section 11, in order), `build_server(warm=False)`,
  `serve(warm=False) -> int`, `tool_list() -> dict`, `add_cli(sub)` registers `mcp`; `python -m
  hark.surfaces.mcp_server [--warm]` runs it.
- `hark.surfaces.llm`: surfaces.md 1.3 functions (`critique`, `encode`, `context`, `build_prompt`, `parse_json`,
  `normalize`, `render`), `add_cli` registers `llm`, plus:

```python
AGY_VERDICT = "HEARS"                     # from docs/specs/hark/agy-audio-probe.md
AGY_DEFAULT_MODEL = "gemini-4-argon-high" # used when --model and HARK_LLM_MODEL are unset
AGY_TIMEOUT_S = 240
class AgyProvider: name = "agy"; ...      # surfaces.md 1.3 provider contract
```

- Transport, from the probe: write the clip as WAV (PCM 16-bit, mono, 16000 Hz) into a fresh temp folder, call
  `agy -p "<prompt naming the absolute WAV path>" --add-dir <that folder> --model <id> --output-format json
  --print-timeout 240s` with `ANTHROPIC_API_KEY` removed from the child environment, check `status == "SUCCESS"`
  and no `finished_reason`, read `response`. Never MP3: agy hears it as silence. So `encode` returns WAV bytes, mime
  `audio/wav`, and clip info `codec` = `"wav mono 16 kHz 16-bit"`.
- Coarse hearing: agy names the sound class and gross faults, but its times are off by 0.2-0.3 s and its counts
  are wrong. The prompt sends hark's own measured times (cuts, events, loudness) as context, and tells the model
  not to estimate times, counts, pitch or levels. `normalize` keeps the model's issue times only as coarse hints.
  The `audio-reviewer` teammate rubric uses hark measurements for every timing claim.
- Consent: nothing is sent without `--yes-upload` (CLI) or `consent=True` (API, MCP).

## 11. MCP tool table

Server name `hark`. 16 tools, in this order (surfaces.md 1.2 is exact for arguments):

| # | tool | backing function | ro | net | ML |
|---|---|---|---|---|---|
| 1 | `measure` | `hark.core.features.measure` | yes | no | no |
| 2 | `describe` | `hark.listen.describe.describe` | yes | no | yes |
| 3 | `match` | `hark.listen.match.match` | yes | no | yes |
| 4 | `rank` | `hark.listen.search.rank` | yes | no | yes |
| 5 | `find` | `hark.listen.search.find` | yes | no | yes |
| 6 | `compare` | `hark.listen.compare.compare` | yes | no | yes |
| 7 | `ab` | `hark.listen.compare.ab` | yes | no | yes |
| 8 | `realness` | `hark.listen.realness.realness`, `realness_many` | yes | no | yes |
| 9 | `timeline` | `hark.timeline.timeline.timeline` | yes | no | yes |
| 10 | `sheet` | `hark.timeline.sheet.sheet` | no | no | yes |
| 11 | `critique` | `hark.timeline.critique.critique` | yes | no | not with `fast` |
| 12 | `doctor` | `hark.doctor.check` | yes | no | only `load` |
| 13 | `llm_critique` | `hark.surfaces.llm.critique` | no | yes | only `with_timeline` |
| 14 | `sfx_list` | `hark.forge.bank.sfx.sfx_list` | yes | no | no |
| 15 | `sfx_render` | `hark.forge.bank.sfx.sfx_render` | no | no | no |
| 16 | `kit_render` | `hark.forge.bank.kit.render` | no | no | no |

The server imports each backing module inside its tool function. A tool whose module is not built yet returns a
tool error that says so (surfaces-mcp M1.6).

## 12. Data asset plan

Mark: R = REGENERATE, D = DERIVE, N = NEEDS-DATA. "Rebuild" names the module and the command. The owner builds or
writes the asset in its own ticket unless the row says 25b (integration).

| asset | path | mark | module | rebuild command | owner | fallback for N |
|---|---|---|---|---|---|---|
| CLAP and AST weights at the pins | `<HARK_HOME>/hf/` | D | `hark.models.hub` | `hark doctor --fetch` | T-13 (code); user runs it | none needed; never committed |
| K-weighting, key profiles, band tables | constants in `hark.core.features` | D | core | none | T-13 | none needed |
| vocabulary | `hark/data/listen/vocab.json` | D | `hark.listen.vocabulary` | hand-written; check with `python -m hark.listen.vocabulary` | T-14 | none needed |
| vocabulary reference statistics | `hark/data/listen/vocab_stats.json` | R | `hark.listen.vocabulary` | `python -m hark.listen.vocabulary --build-stats REF...` | T-14 (code); ship no file | no file: term bias 0, logit scale from the model |
| realness model | `hark/data/listen/realness_model.json` | N | `hark.listen.realness` | `python -m hark.listen.realness --train <labelled dir>` | T-14 | the zero-shot CLAP prompt score of listen-realness "Realness model file: NEEDS-DATA" (listen Q3); the file is valid against the model file format and its `about` says "zero-shot fallback" |
| launch profile | `hark/data/timeline/launch_profile.json` | N | `hark.listen.profile` | `hark profile <films> -o <path>` | T-15 | seed profile from critique table 3.4, `films: []` (timeline A1) |
| tag background means | constants in `hark.timeline.timeline` | R | timeline | timeline A2 procedure | T-15 | ship the 49 numbers as constants |
| query-kind labels, eval label files | `hark/listen/eval/*.json` | D | `hark.listen.eval` | hand-written | T-14 | none needed |
| semantic evaluation sets | outside the repo (`HARK_EVAL_ASSETS`) | N | `hark.listen.eval.run_eval` | `python -m hark.listen.eval.run_eval analyze` | T-14 (code) | EVS tests skip when the variable is unset |
| tuning mixes, evaluation film | outside the repo | N/R | timeline eval | timeline A4 procedure | T-15 (code) | acceptance tests on synthetic mixes |
| seed banks of every recipe | `hark/forge/seeds/<folder>/<recipe>.json` | N (tuned), D (fallback) | recipe owner | hand-written from the part's family tables (`source: "derived"`, `seed` 0, `rank_score` null) | 17a, 17b, 18a, 18b | the DERIVE banks of recipes-motion "Data assets" and recipes-impacts-seeds; an empty bank is valid |
| refs registry and cut WAVs | `<HARK_HOME>/refsets.json` | N | `hark.forge.engine.refsets` | `write(scan(root))` | T-16 (code) | designed mode with STUDY_TARGETS |
| STUDY_TARGETS, BELLS, KIT_BENCH | constants | D | forge engine | none | T-16 | KIT_BENCH empty list |
| benchmark and blind-test results | `hark/forge/engine/eval/results_*` | R | `hark.forge.engine.eval.bench` | `python -m hark.forge.engine.eval.bench` | T-16 (code) | none shipped until refs exist |
| `pluck.json` | `hark/forge/bank/data/pluck.json` | R | `hark.forge.bank.bank` | `python -m hark.forge.bank build --pluck -o <path>` | 25b (17b builds pluck; T-19 builds the builder) | none needed |
| `forge04.json` (51) | `hark/forge/bank/data/forge04.json` | R + N | `hark.forge.bank.bank` | `python -m hark.forge.bank build --forge04 -o <path>` | 25b | originality `{made: "recipe", max_ref_clap: null, ref: "none"}` |
| `forge_v2.json` (183) | `hark/forge/bank/data/forge_v2.json` | R | `hark.forge.bank.bank` | `python -m hark.forge.bank build --derive -o <path>` (11.2) | 25b | closest render, listed in the build log; no id missing |
| `fit.json` (15) | `hark/forge/bank/data/fit.json` | R / N | `hark.forge.bank.bank` | `python -m hark.forge.bank add-fit ...` per recording | 25b, only after the operator allows the 15 CC0 downloads | leave the ids out; presets use the forge replacements of bank part 3, 7.1 |
| kit presets | `hark/forge/kits/*.json` | D | `hark.forge.bank.kit` | hand-written from bank part 3, section 7 | T-19 | none needed |
| gallery family table, texts | constants in `hark.surfaces.gallery` | D | surfaces | none | T-21 | none needed |
| agy probe verdict | `AGY_VERDICT` in `hark.surfaces.llm` | R | probe ticket T-11 | re-run the probe | T-21 | DOES-NOT-HEAR: exit 2, nothing sent |

### 12.1 Interim bank files

Until 25b builds the bank files, T-19 tests with small fixture bank files in `tests/bank/` (`HARK_BANK` and a
`bank_dir` override). `ids()` with no shipped files returns the fixture ids only in tests. The 252-id test (bank AT-1)
and the preset take counts (AT-33) are integration tests that 25b runs.

### 12.2 `build --derive`

The forge file needs forge finals that a critic run with licensed refs made (N). The fallback builder: `python -m
hark.forge.bank build --derive -o OUT` makes one forge entry per row of bank-kit-sfx-catalog section 2. For each id
`<category>/<member>`, it takes the member params from the recipe's family table (`FAMILIES`, `FAMILY`, or the
recipe's `preset(...)` / `recipe_for(cat, member)`), seed 0, `peak_db` -1.0, `gain_db` 0.0, `mode` "designed",
measures `meta`, and writes the file. It logs every id whose measures miss the catalog row tolerances of bank-kit-sfx
section 7. T-19 builds the builder. 25b runs it after every recipe is in.

## 13. Venv

- Path: `/Users/mascott/.local/state/hark/venv`. Made on 2026-11-07 with `python3 -m venv` from
  `/usr/local/bin/python3`.
- Installed: base deps and pytest. Not installed: torch, transformers, mcp, and hark itself (tests run with
  `PYTHONPATH=skills/hark/engine`).

| package | version |
|---|---|
| Python | 3.12.8 |
| numpy | 2.5.4 |
| scipy | 1.18.1 |
| soundfile | 0.14.0 |
| soxr | 1.1.0 |
| librosa | 1.0.0 (numba 0.68.1, llvmlite 0.50.0) |
| pillow | 13.0.0 |
| matplotlib | 3.11.2 |
| pytest | 9.2.0 |

- External tools: `ffmpeg` and `ffprobe` at `/opt/homebrew/bin`. `agy` at `~/.local/bin/agy`.
- Latest versions on the index on 2026-11-07, for the extras: torch 2.15.0, transformers 5.22.2, mcp 2.3.0.
- Every check command of the implementer tickets is:
  `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/<area> -q`.
- Do not install packages into the venv in an implementer ticket. If you need a package, send a QUESTION.

## 14. Test plan

### 14.1 Shared test rules

- `skills/hark/engine/pyproject.toml` sets `--import-mode=importlib` and the markers `ml`, `live`, `slow`. So test
  files in different areas can share a base name. A test folder has no `__init__.py`. Tests cannot import each
  other: put shared test helpers in the test file, or in a fixture of your area's own `conftest.py`.
- Shared fixtures (`tests/conftest.py`, T-12): `sr`, `tone`, `noise`, `clicks`, `silence`, `wav`, `hark_home`,
  `rng`, `has_ml`. See the file for signatures.
- Marker `ml`: the test needs torch and transformers. It skips when they are absent.
- Marker `live`: the test calls a real external program over the network (agy). It skips unless `HARK_LIVE=1`.
- Marker `slow`: a test over 30 s. It runs by default.
- A test that needs private data (NEEDS-DATA) skips with the reason `needs-data: <asset>`.
- Use `tmp_path` and the `hark_home` fixture. Never write to `/tmp`, `~`, or the package folder.
- A test of a model-free path must also run when the ML extras are absent. To prove it, the core suite holds 1 test
  that hides torch (an import hook) and runs `measure --light`, `sfx list` (skipped until T-19 lands) and forge
  `--list`.
- Stub models: listen and timeline tests without the `ml` marker replace `hark.models.clap.get` and
  `hark.models.ast.get` with stubs through `monkeypatch` (fixed unit vectors, fixed probabilities).
- D-CHANGELOG: core AT-14 needs `skills/hark/engine/CHANGELOG.md` with the newest heading `## 0.1.0`. T-13 writes it.
  T-13's Own list must name that file (the orchestrator amends it).

### 14.2 Acceptance tests per area

Every acceptance test of a spec becomes at least 1 pytest case in the area below, named `test_<id>_<words>` with
the id in lower case and `-` as `_` (for example `test_at_12_cli_threads`). A test that cannot run in the area
(needs data, needs another unit, needs ML) still exists and skips with the reason.

| area folder | owner | spec tests | notes |
|---|---|---|---|
| `tests/core` | T-13 | core-io AT-1 .. AT-40 | AT-35 .. AT-38 marked `ml`. AT-40 expects exit 3 (4.4). AT-15 as in section 7. AT-16: 22 names. Add: launcher tests (no venv, non-TTY: exit 3 with the command), "not built yet" exit 2, `require_ml` non-TTY exit 3 with the 5.2 command, model-free import test (14.1). |
| `tests/listen` | T-14 | CAL-1 .. 12, DES-1 .. 10, MAT-1 .. 9, RNK-1 .. 6, FND-1 .. 14, CMP-1 .. 8, AB-1 .. 7, REA-1 .. 16, PRO-1 .. 11, EVS-1 .. 12 | [model] tests marked `ml`; the others run with stub models. EVS skip without `HARK_EVAL_ASSETS`. `match` texts follow surfaces-mcp M3 and M5. |
| `tests/timeline` | T-15 | T1 .. T19, V1 .. V6, S1 .. S7, C1 .. C14 | synthetic mixes made in the test; CLAP and AST stubbed unless marked `ml`. |
| `tests/forge` | T-16 | L1 .. L14, D1 .. D20, T1 .. T10, P1 .. P15, K1 .. K11, S1 .. S8, R1 .. R12, C1 .. C10, RS1 .. RS5, V1 .. V3, B1 .. B15, BT1 .. BT11 | Engine tests use a recipe object built inside the test file (`types.SimpleNamespace`, 8.2), never a package recipe. Critic tests that need CLAP are marked `ml`. Add registry tests: `names()` with a fake folder tree, duplicate names raise, `seed_path`, `UnknownRecipe`. K tests that need a bank entry use a hand-made entry dict and a fake `run`. |
| `tests/recipes_ui` | 17a | recipes-impacts index 8.1 .. 8.18 for click, keypress, typing, toggle, shutter, ui_pop, popin, pop; every per-recipe test of parts ui, keys, pop | contract 8.1 and determinism 8.2 for each of the 8; pop v2 tests start with `importorskip` of `risers/notify`. |
| `tests/recipes_impacts` | 17b | recipes-impacts index 8.1 .. 8.18 for impact, cuthit, thump, knock, mallet, subdrop, drip, pluck; tests of parts impact, hits, tuned, seeds | thump tests `importorskip` `motion/whoosh`. |
| `tests/recipes_motion` | 18a | recipes-motion AT1 .. AT4, AT7, AT17 .. AT23, AT29 (motion banks); AT1 and AT2 for the 7 motion recipes | zip `importorskip` `risers/riser`; shimmer `importorskip` `risers/notify`. |
| `tests/recipes_risers` | 18b | recipes-motion AT5, AT6, AT8 .. AT16, AT24 .. AT28, AT30, AT29 (riser banks); AT1 and AT2 for the 8 riser recipes | sting `importorskip` `impacts/impact` and `impacts/subdrop`. |
| `tests/bank` | T-19 | bank-kit-sfx AT 1 .. 34 | AT-1, AT-2, AT-12, AT-20 .. AT-22, AT-31, AT-33, AT-34 need the shipped bank: skip with `needs-bank` until 25b builds it; run the same logic now on fixture banks. AT-7, AT-8 use fixture entries of pluck and whoosh when the recipes are present (`importorskip`). |
| `tests/fit_analysis` | 20a | fit A-5 .. A-9, A-14, A-16 at the analysis level (per kind: fit, render, check, save/load, determinism, morph-side), plus every test that a part (modal, sines, spectral, gesture, space) states | S1 .. S5 synthetic sounds made in the test. A-1 .. A-4 skip `needs-data`. |
| `tests/fit_pipeline` | 20b | fit A-5 .. A-13, A-15, Q-1 .. Q-9, C-1 .. C-13 | Until 20a lands, use stand-in analysis modules built in the test that follow 9.2 (inject them with `monkeypatch` on `hark.fit.analysis.module_for`). CLAP and describe checks marked `ml`. |
| `tests/surfaces` | T-21 | G-T1 .. G-T20, M-T1 .. M-T23, C-T1 .. C-T20, L-T1 .. L-T5 | M-T tests of `match` call it through the MCP tool, with a stub model; T-14 tests the `match` function itself. | agy faked by a fake executable on `PATH` made in `tmp_path`; 1 test marked `live`. MCP stdio tests need the `mcp` extra: skip when absent. |

Integration (25b) runs every area together with the ML extras installed, removes no skip, and runs the tests that
skip for a missing unit or the shipped bank.

## 15. Decisions on spec open questions

| id | decision |
|---|---|
| core Q1 | Drop the legacy pickle CLAP state for hark's own cache. Pickle loads only from a user's local model folder. |
| core Q2 | ML tier absent: a doctor note that names the 5.2 command, not a problem. `--load` reports a problem. |
| core Q3 | Doctor reports `agy` (path or null). It never reads an API key. |
| core Q4 | librosa is a base dependency. pyloudnorm and scikit-learn are not dependencies. |
| core Q5 | `measure` applies `start` / `end` to arrays too. |
| core Q7 | No in-repo cache: `HARK_HOME`, else `~/.cache/hark`. |
| core Q8 | Version 0.1.0. |
| core Q9 | Confirmed: command detection skips the value of a global `--threads`. |
| core section 6 step 3 | ML install exits 3, not 1 (4.4). |
| listen Q1 | Our own vocabulary phrases with the thresholds 3 / 2.5 / 2 as starting values. |
| listen Q2 | Keep the anchor and prompt phrases as given (generic noun phrases, constants). |
| listen Q3 | Ship the zero-shot prompt fallback first (12, realness row). |
| listen Q4 | Ship the seed launch profile (timeline A1). |
| listen Q5 | Keep the behaviour. The help text says what the flag does (total tags and total hints). |
| listen Q6 | `describe` needs AST and uses the install prompt. No silent CLAP-only mode. |
| listen Q7 | Open. EVS tests skip until the operator names a corpus. |
| timeline Q1 .. Q8 | The proposed answers of timeline.md section 4. |
| forge Q1 | Recipe names are the flat names of 8.4. The knobs tables key by these names. LEGACY is empty. |
| forge Q2 | The bank renderer accepts `run(edit)` and `run(edit, fit_knobs)`. |
| forge Q3 | Keep the split: Python `forge()` defaults to the first-version loop; the CLI uses the second-version setup. |
| forge Q4 | Seed banks ship as the DERIVE banks of the recipe specs. |
| forge Q5 | `pro_*` categories are public categories. |
| forge Q6 | Keep the benchmark phrasings as given (constants). |
| recipes-impacts | Q-POP-1 keep numbered names; Q-POP-2 smooth the step; Q-EXACT-1 yes; Q-RANGE-1 widen the space; Q-TONAL-1 the tonal module is `hark.forge.recipes.tonal`, owned by T-16; Q-PLUCK-1 merge; Q-PLUCK-2 skip the mode at or above 0.45 x 48000; Q-SEED-1 fallbacks; Q-REFS-1 no refs. |
| recipes-motion | Q1 name the member `punch`; Q2 effective values; Q3 keep the 1-sample offset; Q4 keep; Q5 derived banks; Q6 keep every path. |
| bank Q1 | Null originality with the marker. |
| bank Q2 | `python -m hark.forge.bank`. |
| bank Q3 | Keep the sound ids. |
| bank Q4 | Tags `forge_v2h1`, `forge04h1`, `fit1h`, `pluck1h`. |
| bank Q5, fit Q6 | Open: the operator decides on the 15 CC0 downloads. Until then `fit.json` is absent and presets use the forge replacements. |
| bank Q6 | No tonal knobs in `sfx render`. |
| bank Q7 | Fix the quiet side channel. |
| fit Q1 .. Q5, Q7 | The proposed answers of fit.md. |
| surfaces Q1 | listen owns `match` (10.2). |
| surfaces Q2 | 72 dB. |
| surfaces Q3 | `HARK_LLM_MODEL`; when unset, use `AGY_DEFAULT_MODEL` (probe evidence), not agy's own default. |
| surfaces Q4 | The probe's best invocation, WAV only (10.5). |
| surfaces Q5 | Quote the validation numbers as source measurements with that label. |
| surfaces Q6 | `HARK_PYTHON`. |
| surfaces Q7 | Our own titles. |
| surfaces Q8 | Test all 16 tools over stdio. |
| surfaces Q9 | Identical-bytes mark last. |
| surfaces Q10 | Retry once after 4 s only when agy's output marks a rate limit. |
| layout | `python -m hark.listen.eval.*` and `python -m hark.forge.engine.eval.*` hold the eval scripts inside the owner's package folder. |
