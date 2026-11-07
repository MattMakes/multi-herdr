# hark fit: recipe code generation and the `hark fit` command (part of fit.md)

This part specifies 3 things: how a fitted model becomes a Python recipe module, how that module is written and
imported, and the `hark fit` command with its 3 modes (fit, morph, forge search). The index is `fit.md`.

Modules: `hark.forge.fit.codegen` (code generation) and `hark.forge.fit` (package API and CLI).

## Interface

### Package API (`hark.forge.fit`)

Each function imports its engine module only when it is called, so `import hark.forge.fit` stays cheap.

| name | contract |
|---|---|
| `fit(x, kind='auto', **kw)` | Calls the engine's `fit(x, kind=kind, **kw)` (see `fit-auto.md`). |
| `render(model, seed=0, **knobs)` | Calls the engine's `render(model, seed=seed, **knobs)` (see `fit-render.md`). |
| `morph(a, b, amount=0.5)` | Calls the engine's `morph(a, b, amount)`. |
| `to_recipe(model, **kw)` | Calls `codegen.to_recipe(model, **kw)`. |
| `run_fit(path, kind='auto', layers=None, output=None, out_dir=None, render_path=None, report=False, clap=False, describe=False, seed=0, target=None, fast=False, verbose=True, log=print, takes=False, bed=True)` | The fit pipeline of the CLI as a function. Returns the result dict (below). `log` is accepted and not used. |
| `run_morph(path_a, path_b, amount=0.5, output=None, out_dir=None, render_path=None, clap=False, seed=0, fast=False, verbose=True)` | The morph pipeline of the CLI as a function. Returns the morph result dict (below). |
| `add_cli(sub)` | Adds the `fit` sub-command to the surfaces unit's argument parser. |

### Code generation (`hark.forge.fit.codegen`)

| name | contract |
|---|---|
| `literal(model, name='MODEL')` | The string `"<name> = <literal>"`. The literal is Python source that evaluates to a dict equal to the model without its `fit_engine` key. |
| `critic_words(source=None, category=None, character=None)` | `(TARGET, ALTS, REFSET)`: the critic's words for a fitted recipe. |
| `to_recipe(model, name='fitted', source=None, quality=None, floor=None, target=None, alts=None, refset=None, refs=None, licence=None)` | The full source text of a recipe module. |
| `write_recipe(path, model, **kw)` | Writes `to_recipe(model, **kw)` to `path` and returns `path`. |
| `load_recipe(path)` | Imports a recipe module from a file path and returns the module. |
| `DROP = ('fit_engine',)` | The model keys left out of `MODEL`. |

## The generated recipe module

The module is the deliverable of the fit unit. It is self-contained apart from one import of the hark engine. The
forge-core unit's recipe loader and search must accept it like any hand-written recipe.

### Top-level names, in file order

| name | type | content |
|---|---|---|
| module docstring | str | Free text (see "Docstring content"). |
| `np` | module | `import numpy as np`. |
| engine import | module | The engine module `hark.forge.fit.auto`, imported under a private alias of the implementer's choice. |
| `TARGET` | str | The critic's text description of the sound. |
| `ALTS` | list of str | Alternative descriptions (may be empty). |
| `REFSET` | str | The refset category of the critic's default references. Present only when a category is known. A trailing comment says what it is. |
| `REFS` | list of str | Absolute paths (forward slashes) of the critic's real references: the source recording when it exists on disk, else `[]`. |
| `KIND` | str | `kind_of(model)`: `modal`, `sines`, `spectral`, `gesture`, `hybrid` or `morph`. |
| `SPACE` | dict | The model's knob table as search ranges: `name: range`, one knob per line, with the knob's description as a trailing comment. The range is the tuple from the knob table (`(lo, hi)` or `(lo, hi, 'log')`). |
| `DEFAULTS` | dict | `{name: identity}` for every knob of `SPACE`, on one line. It equals the engine's `identity(model)`. A trailing comment says these values re-render the fit. |
| `MODEL` | dict | The fitted parameters as a Python literal (see "Model literal"). |
| `render(p, r)` | function | One take (see below). |
| `render_seed(seed=0, **knobs)` | function | A render with an explicit seed (see below). |
| main block | | Runs when the file is executed as a script (see below). |

### `render(p, r)`

- `p`: a dict of knob values. `r`: a numpy `Generator`.
- Start from a copy of `DEFAULTS`. Overwrite it with every entry of `p` whose key is in `DEFAULTS`. Ignore every
  other key of `p`.
- Draw one integer: `seed = int(r.integers(0, 2**31 - 1))`. This is the only draw from `r`.
- Return the engine's `render(MODEL, seed=seed, **q)`.

This is the forge-core recipe calling convention. The critic passes a fresh generator per take, so every take gets
its own noise and jitter.

### `render_seed(seed=0, **knobs)`

- Start from a copy of `DEFAULTS`. Overwrite it with every keyword given, without a filter.
- Return the engine's `render(MODEL, seed=seed, **q)`.

Extra engine keywords (`take`, `bed`) pass through. An unknown knob name reaches the engine, which raises
`ValueError`. `render_seed(0)` is the fit as measured.

### Main block

When the file runs as a script:
1. `seed` = the first command-line argument as an int, else 0.
2. `out` = the second argument, else `<name>.wav` (in the current folder).
3. Write `render_seed(seed)` as float32 to a 48 kHz WAV file with 32-bit float samples. A mono render gives a 1-channel
   file, an `(n, 2)` render a 2-channel file.
4. Print `out`.

### Bitwise identity contract

For every model m, every seed s and every set of knob values k:

`load_recipe(write_recipe(path, m)).render_seed(s, **k)` is equal, element by element and bit for bit, to
`engine.render(m, seed=s, **k)`.

Two things make this hold:
- The literal keeps every float exactly (`repr` of a Python float round-trips).
- The engine's render does not read `fit_engine`, and `MODEL` leaves only that key out.

`run_fit` checks the contract on every fit and reports the result as `code_renders_fit`.

### Model literal

`literal` writes the model with these rules (`ind` = the current indent, width limit 116 characters):

| value | written as |
|---|---|
| bool (Python or numpy) | `True` / `False` |
| int (Python or numpy) | the int |
| float (Python or numpy) | Python `repr` of the float. NaN and the infinities as `float('nan')`, `float('inf')`, `float('-inf')`. |
| None, str | Python `repr` |
| empty dict / empty list | `{}` / `[]` |
| dict | `{`, then one line per key: `ind + 4` spaces, `repr(key)`, `: `, the value, `,`. Then `}` at `ind`. Key order is kept. |
| list or tuple of scalars only | One line `[a, b, ...]` when `len + ind <= 116`. Otherwise wrapped: items are joined with `, ` and a line breaks before an item when the current line is not empty and `len(current) + len(item) + ind + 6 > 116`. Each wrapped line sits at `ind + 4` spaces with its trailing space removed. The closing `]` sits at `ind`. |
| list or tuple with a non-scalar | `[`, then one element per line at `ind + 4`, each followed by `,`, then `]` at `ind`. |

A tuple becomes a list. That is harmless: models are plain JSON, so their arrays are lists already.

So a mode, a partial, a strike atom or a keyframe each sit on their own line, and a long row of numbers wraps.

Annotations: for some well-known model keys, the literal adds a short comment that tells a reader what the field
holds. When the value fits on one line, the comment trails it on the same line. When the value spans lines, the
comment sits on its own line just above the key. The keys that get an annotation are `strikes`, `modes`, `attack`,
`noise`, `sub`, `side`, `partials`, `transients`, `residual`, `env`, `stereo`, `texture`, `ridges` and `layers`. The
comment wording is free: write each from the field tables of the fitter parts (`fit-modal.md`, `fit-sines.md`,
`fit-spectral.md`, `fit-auto.md`). Comments never change the value.

### Docstring content

The docstring is free text for a human reader. Tests must not compare its wording. It holds, in this order:
1. A first line: the recipe name, that `hark fit` made it from a recording, and the model kind.
2. The engine's `summary(model)` lines, with trailing spaces removed.
3. `Source: <name>` with ` (refset <category>)` when a category is known. `<name>` is the source path's base name,
   else the model's `source`, else `unknown`.
4. A licence note. By default: a fit derives from its recording, so the source's attribution and licence must stay
   with the file, and CC0 sources are preferred for anything shipped. A caller can replace this with `licence`.
5. When a quality dict is given: a line that says the measures compare the original with this recipe at `DEFAULTS`
   and seed 0, then the quality text line (`quality.text(quality, floor)`, see `fit-quality.md`), indented 2 spaces.
6. Usage lines: one take with a seeded generator, `render_seed(0)`, one variation that changes the first knob of the
   table, the `hark fit --forge <name>.py --budget 96 --dir made/<name>` command, and how to install the file as a
   forge recipe.
7. A line that says `SPACE` holds the search ranges and `DEFAULTS` is the identity.

Escaping: every `"""` in the text becomes `'''` and every backslash becomes `/`. The docstring is then closed by
`"""` on its own line. So the module always parses, whatever paths or names the text holds.

## Algorithms

### Critic words

Input: the source path, an optional category and the measured character dict (the engine's `fit_engine.character`,
see `fit-auto.md`).

1. When `category` is not given, look the source up in the refsets registry (forge-core unit). The category is the
   first category whose `forge` or `bench` file list holds the source. Compare paths after making them absolute and
   normalising their case. With no source, no registry, or any registry error, there is no category.
2. With a category:
   1. Walk the forge recipes in the order the forge-core unit lists them. Skip any recipe that fails to load. The
      first recipe whose `REFSET` equals the category lends its words. `TARGET` is that recipe's `TARGETS[category]`
      when it has a `TARGETS` dict with that key, else its `TARGET`. `ALTS` is a list copy of its `ALTS` (or `[]`).
      Return `(TARGET, ALTS, category)`.
   2. Any error during this walk drops to the next step.
   3. When the category is in the refsets `CATEGORIES` table, return its description with a trailing parenthetical
      (and the spaces around it) removed, `[]` and the category.
3. With no category, or a category that neither step found: pick a description from the character. Take the first
   rule that holds:

| rule | TARGET |
|---|---|
| `gliding` | `a short round pop with a pitch glide` |
| `tonal` and `t40_ms > 400` | `a bright ringing chime` |
| `tonal` | `a short pitched ding` |
| `impulsive` and `dur_s < 0.5` (`dur_s` defaults to 1) | `a short crisp click` |
| `impulsive` | `a deep hit with a long tail` |
| `noisy` | `an air whoosh` |
| otherwise | `a sound effect` |

   Return `(TARGET, [], None)`. A missing character counts as an empty dict, so the result is `a sound effect`.

These strings are critic prompts. The critic encodes them with CLAP, so they are part of the behaviour.

### to_recipe

1. `kind = kind_of(model)`, `knobs = knobs_of(model)`, character = `model.fit_engine.character` (or none).
2. `(t, a, cat) = critic_words(source, refset, character)`. The `refset` argument is the category hint.
3. `TARGET = target or t`. `ALTS = a` when `alts` is None, else `list(alts)`. `REFSET = refset or cat`.
4. `REFS = refs` when given. Else `[absolute source path with backslashes as '/']` when the source exists on disk,
   else `[]`.
5. Build the docstring, then the names in file order. End the text with a newline.

### write_recipe

1. When `name` is not given, it is the file's stem with every non-word character (not a letter, digit or `_`)
   replaced by `_`.
2. Create the parent folder when it is missing.
3. Write the text as UTF-8. Return the path.

### load_recipe

1. `name` = the file's stem, cleaned as in `write_recipe`.
2. Import the file as a new module object under a private unique module name made from a fixed prefix and `name`.
   Do not use the normal import path, and do not add it to `sys.modules`. Loading the same path twice gives 2
   independent module objects.
3. Set the module's `__name__` to `name`. Return the module.

## The `hark fit` command

### Flags

The surfaces unit's common flags (among them `--json`) are added first.

| flag | default | meaning |
|---|---|---|
| `file` (positional, optional) | | The recording to fit. |
| `-o`, `--output` | | Recipe path (`.py`). Its folder and stem name every output. |
| `--dir` | current folder | Output folder when `-o` is not given. With `--forge`: the output folder of the search. |
| `--kind` | `auto` | One of `auto`, `modal`, `spectral`, `sines`, `gesture`, `hybrid`. Another value is a usage error. |
| `--layers` | | Comma-separated hybrid layers, for example `sines,modal,spectral`. The engine's default is `modal,spectral`. |
| `--render` | next to the recipe | WAV path of the resynthesis. |
| `--report` | off | Also measure the floor and CLAP, and write `<name>.report.json`. |
| `--clap` | off | CLAP takes part in the choice of model (loads CLAP). |
| `--describe` | off | Run the listen unit's describe on the original and the render. |
| `--target` | | The critic description for the recipe. Overrides the critic words. |
| `--seed` | 0 | The seed of the written render. With `--forge`: the search seed. |
| `--fast` | off | Cheaper fitter settings, for previews. |
| `--takes` | off | Score every candidate on seeds `TAKE_SEEDS = (0, 1, 2)`, not on seed 0 alone. |
| `--keep-bed` | off | Keep the bed and fit the unmodified recording. Without this flag the engine removes the bed. |
| `--morph A B` | | Fits A and B with a shared kind and writes a morph recipe. |
| `--amount` | 0.5 | With `--morph`: 0 is A, 1 is B. |
| `--forge RECIPE_PY` | | Runs the forge critic search for variations of a recipe file. |
| `--budget` | 96 | With `--forge`: candidates the critic scores. |
| `--pop` | 16 | With `--forge`: population per generation. |
| `--variants` | 4 | With `--forge`: different sounds to keep. |
| `--refset` | the recipe's `REFSET` | With `--forge`: the critic's refset category. |
| `--quiet` | off | Do not print the candidates while fitting. |

### Dispatch

The modes are tried in this order. The first that applies runs. Each successful mode returns exit code 0.
1. `--forge` given: the forge search.
2. `--morph` given: the morph.
3. No `file`: print a one-line usage text that shows the 3 forms (fit a file, `--morph A B`, `--forge recipe.py`) and
   return exit code 2.
4. Else: the fit.

Every mode passes its result dict and a readable text to the surfaces unit's `emit`. `emit` prints JSON with
`--json` and the text otherwise.

### Output paths

From the `-o` path and the stem of the input:
- With `-o`: folder = the folder of `-o` (absolute), base = the stem of `-o`.
- Without `-o`: folder = `--dir` or the current folder (absolute), base = `<input stem>_fit`.
- Create the folder.

| key | path |
|---|---|
| `recipe` | `<folder>/<base>.py` |
| `wav` | `--render` when given, else `<folder>/<base>.wav` |
| `png` | `<folder>/<base>.png` |
| `model` | `<folder>/<base>.json` |
| `report` | `<folder>/<base>.report.json` |
| `name` | `base`, the recipe name |

All paths use forward slashes. A WAV is written as float32 at 48 kHz with 32-bit float samples, and its folder is
created when missing.

### Fit mode (`run_fit`)

1. Paths as above. Start a timer.
2. `X` = the recording, loaded by the quality loader (see `fit-quality.md`).
3. `m = engine.fit(X, kind, layers, clap, fast, name=<input base name>, verbose=<verbose and kind is auto>,
   seeds=TAKE_SEEDS when takes else (0,), bed)`. The CLI passes `layers` as a tuple from the comma list,
   `verbose = not --quiet and not --json` and `bed = not --keep-bed`. Note the fit time.
4. Write the recipe (`name`, `source = path`, `target`). Load it. `y = recipe.render_seed(seed)`. The written WAV is
   the code's render, not the engine's, so the WAV shows the output of the file itself.
5. Write `y` to the WAV path.
6. `q = quality(X, y, clap = report or clap, describe = describe)`.
7. With `report`: `floor = quality.floor(lambda s: recipe.render_seed(s), clap=False)`. Else no floor.
8. Write the recipe again, now with `quality = q` and `floor`, so the docstring holds the measures. Load it again.
   `y2 = recipe.render_seed(seed)`.
9. `code_renders_fit` = `y2` equals `engine.render(m, seed=seed)` exactly, and `y2` equals `y` exactly.
10. Save the model JSON (the engine's `save`, with `fit_engine` kept).
11. Write the pair picture of `X` against `y`. Its title names the input, the recipe file and the kind, plus
    `auto: <chosen>` for an auto fit.
12. Build the result dict. With `report`: write the report JSON (the result dict plus `fit_engine` and the engine's
    `summary` text, indent 1, numpy numbers as floats) and add `report` to `files`.

Result dict:

| key | value |
|---|---|
| `source` | The input path (forward slashes). |
| `kind` | `kind_of(m)`. |
| `chosen` | `fit_engine.chosen`, else the `kind` argument. |
| `quality`, `floor` | The quality dict, and the floor dict or null. |
| `files` | `recipe`, `wav`, `png`, `model` paths (plus `report` with `--report`). |
| `code_renders_fit` | bool. |
| `seconds` | `fit` and `total`, rounded to 2 decimals. |
| `candidates` | Per candidate row of `fit_engine.candidates`: `candidate`, `score`, `fit_s`, `error` (null when absent). Empty for a single-kind fit. |

Text: a first line with the input name, the kind, `auto chose <chosen>` for `--kind auto` and the fit time with one
decimal. Then the quality text line with its floor. With a floor that has a `mel_db`, a line that explains the
floor: the distance between 2 seeds of the same model, which is the best a model with noise can reach. Then a line that states
`code_renders_fit`. Then one `wrote <key>: <path>` line per file.

### Morph mode (`run_morph`)

1. Stems `sa`, `sb` of the 2 inputs. Stem = `<sa>__<sb>_morph` with every `_fit` removed.
2. Paths from that stem. Without `-o`, the base would end in `_morph_fit`. Replace `_morph_fit` by `_morph` in every
   path and in the name, so the base is `<sa>__<sb>_morph`.
3. `(ma, mb, kinds) = engine.fit_pair(path_a, path_b, clap, fast, verbose)` (see `fit-render.md`).
4. `mm = engine.morph_model(ma, mb, amount)`.
5. Recipe text: `to_recipe(mm, name, source=None, licence=<a note that names both recordings' base names and asks
   to keep both sources' attribution and licence>)`. With no source, `REFS = []`. A morph model has no
   `fit_engine`, so it has no character and `TARGET` is `a sound effect`. The CLI does not pass `--target` in this
   mode. Create the folder and write the file as UTF-8.
6. Load the recipe. `y = recipe.render_seed(seed)`. Write the WAV.
7. `Xa`, `Xb` loaded. `vs_a = quality(Xa, y, clap)`, `vs_b = quality(Xb, y, clap)`.
8. Pad the mono mixes of `Xa`, `y` and `Xb` with zeros to the longest length. Pair picture with 3 columns: `Xa`,
   the morph, and `Xb` as the extra panel. The title holds the amount with 2 decimals and both input names.
9. Save the morph model JSON.
10. Return `{a, b, amount, kind: kind_of(ma), kinds_tried: kinds, vs_a, vs_b, files}` with the 4 file paths.

No floor, no report file and no `code_renders_fit` check in morph mode.

Text: a first line with the amount (2 decimals), both input names and the kind. Then `vs A` and `vs B` quality text
lines. Then the `wrote` lines.

### Forge mode

1. Load the recipe file with `load_recipe`.
2. `refset` = `--refset`, else the recipe's `REFSET` (or none).
3. `refs` = none when there is a refset, else the recipe's `REFS` when it is not empty, else none.
4. `out` = `--dir`, else the recipe path without its extension plus `_forge`.
5. Run the forge-core search on the recipe module with `refs`, `refset`, `budget`, `pop`, `keep = variants`,
   `seed`, `out`, and the fixed settings `robust = 3`, `prescreen = 3 x pop`, `shortlist = 2`, `strategy = 'es'`.
6. Emit the search result without its `history` entry. Text: the search summary, one line per pick (its name and
   its score summary), and a last line that names `out` and what the search wrote there.

## Constants

| name | value |
|---|---|
| literal width limit | 116 characters |
| wrap margin | 6 characters |
| dropped model key | `fit_engine` |
| recipe seed draw | `r.integers(0, 2**31 - 1)` |
| default recipe name | `fitted` (in `to_recipe`), the cleaned file stem (in `write_recipe`) |
| output stem without `-o` | `<input stem>_fit` |
| morph stem | `<a>__<b>_morph`, `_fit` removed |
| WAV format | 48000 Hz, float32 samples, 32-bit float subtype |
| forge mode fixed settings | `robust = 3`, `prescreen = 3 x pop`, `shortlist = 2`, `strategy = 'es'` |
| forge defaults | budget 96, pop 16, variants 4 |
| usage exit code | 2 |

## Errors and edge cases

- An unknown model kind raises `ValueError` in `to_recipe` (from `kind_of`).
- A NaN or infinite float in the model is written as `float('nan')` or `float('inf')`, so the literal still parses.
- A docstring with `"""` or a backslash in a path is escaped (see "Docstring content").
- A file name with `-`, `.` or spaces gives a recipe name with `_` in their place. Two names that clean to the same
  name are not told apart.
- A refsets registry that is missing or broken gives no category, never an error.
- A forge recipe that fails to load is skipped in the critic-words walk.
- `render(p, r)` ignores keys that are not knobs. `render_seed` does not filter, so an unknown knob raises
  `ValueError` in the engine.
- A source path that does not exist gives `REFS = []`.
- `--kind` with a value outside the list is a usage error from the parser (exit code 2).
- With `--forge` and `--morph` both given, only `--forge` runs.
- An error inside the engine's fit or render reaches the caller. The CLI does not catch it.
- `code_renders_fit = False` is reported, not raised. It is a bug when it happens.

## Acceptance tests (codegen and CLI)

These are part of the index's acceptance list. Use short synthetic recordings made in the test (for example a 0.3 s
sum of 3 decaying sines at 440, 1100 and 2300 Hz with a 2 ms noise click, and a 0.4 s band-passed noise swell).

| id | setup | expect |
|---|---|---|
| C-1 | Fit each kind (`modal`, `sines`, `spectral`, `gesture`, `hybrid`, and `auto`) on the synthetic sounds. `eval` the literal of each model. | The result equals the model without `fit_engine` (dicts and lists equal, floats bitwise equal). |
| C-2 | Same models. Write and load each recipe. | `render_seed(s)` equals `engine.render(m, seed=s)` bit for bit for s in 0, 1, 7, and also with one non-identity knob value from `SPACE`. |
| C-3 | A recipe, `r = numpy.random.default_rng(5)`, and a copy `r2` of the same state. | `render({}, r)` equals `engine.render(MODEL, seed=int(r2.integers(0, 2**31 - 1)), **DEFAULTS)`. `render({'nosuchknob': 3}, r3)` equals `render({}, r4)` for 2 generators in equal states. |
| C-4 | A recipe of each kind. | `DEFAULTS == identity(model)`. The keys of `SPACE` equal the knob table's names in order, and each range equals the table's range. `KIND == kind_of(model)`. |
| C-5 | A morph recipe of 2 sines fits a and b (`morph_model(a, b, 0.5)`). | `SPACE` has a `morph` knob with range 0.2 .. 0.8 and `DEFAULTS['morph'] = 0.5`. `render_seed(0, morph=0)` equals `engine.render(a without fit_engine, seed=0)` and `render_seed(0, morph=1)` equals `engine.render(b without fit_engine, seed=0)`, bit for bit. |
| C-6 | `write_recipe('out/my-fit.v2.py', m)`. | The module name is `my_fit_v2`. Running the file as a script with no arguments writes `my_fit_v2.wav`: 48 kHz, 32-bit float, the same samples as `render_seed(0)` as float32. |
| C-7 | `critic_words(character=...)` for each rule of the table, and for none. | The listed TARGET for each rule, `ALTS = []`, `REFSET = None`. No character gives `a sound effect`. |
| C-8 | A fake refsets registry that lists the source under category `click`, and a fake forge recipe with `REFSET = 'click'`, `TARGETS = {'click': 'X'}`, `ALTS = ['Y']`. | `('X', ['Y'], 'click')`. Without `TARGETS`, the recipe's `TARGET`. Without that recipe, the category's description with its trailing parenthetical removed. |
| C-9 | `to_recipe` with a source named `a"""b\c.wav` and a quality dict. | The text compiles. Its docstring holds no `"""` inside and no backslash, names the source and holds the quality text line. |
| C-10 | `run_fit` on a synthetic file with `report=True`. | `code_renders_fit` is True. The recipe, WAV, PNG, model JSON and report JSON exist. The floor is a dict. The recipe docstring holds the quality line. Without `report`, there is no report file and the floor is null. |
| C-11 | `run_morph('x/a_fit.wav', 'x/b.wav')` with no `-o` and `out_dir='o'`. | The files are `o/a__b_morph.py`, `.wav`, `.png`, `.json`. |
| C-12 | `hark fit` with no file and no mode flag. | The usage line is printed and the exit code is 2. |
| C-13 | `hark fit --forge rec.py` where the recipe has `REFSET`. | The search runs with `refs = None`, that refset, `robust = 3`, `prescreen = 48`, `shortlist = 2`, `strategy = 'es'`, and `out = rec_forge`. |
