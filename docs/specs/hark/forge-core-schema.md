# forge-core part 6: the report schema and the re-render check

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.schema`: the render block that every
delivered sound carries in its report.json, the functions that re-render a pick from it, the check of a final
folder against its WAV files, and the upgrade of older reports.

Rule: every delivered sound re-renders from its report.json alone, sample for sample, to one 24-bit step.

## Interface

| name | kind | meaning |
|---|---|---|
| `SCHEMA` | int | 2 |
| `TOL` | float | 1e-6 (max sample error for a match) |
| `PEAK_DB` | float | -1.0 (every render is peak-normalised to this before the gain) |
| `BLOCK_KEYS` | tuple | `recipe, fn, member, params, seed, mode, peak_db, gain_db, gain` |
| `LEGACY` | dict | stand-in names of older reports -> (module, function); see "Legacy reports" |
| `NotReproducible` | exception, subclass of `ValueError` | a pick whose render cannot be resolved or fails |
| `render_ref(fn)` | function | `(module short name, function name)` or None |
| `resolve(block)` | function | the render function a block names |
| `make_block(fn, params, seed, member=None, mode=None, gain=None, gain_db=None, peak_db=PEAK_DB, ref=None)` | function | a block dict or None |
| `legacy_block(report, pick)` | function | an inferred block for an older report |
| `block(report, pick)` | function | the pick's own block, else the inferred one |
| `raw(b)` | function | the render function's float64 output, or None when it fails or is silent |
| `level(x, b)` | function | the raw render at the written level, float32 |
| `rerender(b)` | function | `level(raw(b), b)`; raises NotReproducible on failure |
| `compare(x, y)` | function | max absolute sample difference; inf when shapes differ |
| `check_final(report, folder=None, tol=TOL, workers=None)` | function | list of rows |
| `fill(b, x=None)` | function | the block with params completed from DEFAULTS where bit-identical |
| `upgrade(report, write=False, tol=TOL, folder=None, log=print)` | function | `(report, rows)` |
| `main(argv=None)` | function | CLI entry, exit code |

### Render block (schema 2)

| key | meaning |
|---|---|
| `recipe` | short module name in `hark.forge.recipes` that defines the render function |
| `fn` | the render function's name in that module (`render` for most; another name for a second render path of a module, for example a typed-run renderer) |
| `member` | family member, preset or character name, or null |
| `params` | the complete dict the function was called with, including every key a stand-in's DEFAULTS, a family member or a fill pass supplied. A later change of a recipe's DEFAULTS therefore cannot change a re-render. |
| `seed` | int; the generator is `dsp.rng(seed)` |
| `mode` | the critic mode the pick was chosen in (`designed`, `real` ...), or null |
| `peak_db` | float; the render is peak-normalised to this first (-1.0) |
| `gain_db` | float, 2 decimals: the gain after normalisation (palette loudness match; 0.0 for a plain forge pick) |
| `gain` | optional exact linear gain, written when it is known (palette). Without it, the gain is 10^(gain_db / 20). |

The written WAV equals float32(normalise(fn(params, rng(seed)), peak_db) x gain), stored as PCM 24-bit. A re-render
must equal it within one 24-bit step (about 1.2e-7).

### CLI

`python -m hark.forge.schema DIR... [--upgrade] [--write] [--tol T]` (the `hark forge --check DIR...` command
calls the same `main` with the folders):
- `DIR` items may be glob patterns. Keep each matched path (or the literal when nothing matches) that holds a
  `report.json`. Sort and de-duplicate.
- Default: `check_final` each folder.
- `--upgrade`: `upgrade` each folder; with `--write` the report is written when every pick matches. A
  NotReproducible error prints `<dir>: <message>`, counts as bad, and the loop continues.
- Per folder print `<dir>: <n_ok> / <n> re-render within <tol> (max err <e>; <sources>)` and, for each failing row,
  an indented line `<name>: err <e> <why>`. Sources are `block`, `legacy`, or for upgrade `had a block`, `block
  added` (with write), `block inferred, not written`.
- Exit code 1 when any folder has a failing pick or an upgrade error, else 0.

## Behaviour

### `render_ref(fn)`

Unwrap a partial through its `func`. Take its module name and function name. Return None when the module is not
inside `hark.forge.recipes`, when there is no name, or when the name is `<lambda>`. Return None when the module's
attribute of that name is not the same object (a closure or a rebound name). Return None on an import error. Else
return (short module name, function name).

### `resolve(block)`

Import `hark.forge.recipes.<recipe>` and get attribute `fn` (default `render`). Any import, attribute, key or type
error becomes NotReproducible with the text `render <recipe>.<fn> not found: <error>`.

### `make_block`

ref = the given ref, else `render_ref(fn)`. Return None when there is no ref (the writer of a lambda must give the ref
itself). gain_db = the given value, else 20 log10(gain) rounded to 2 decimals when gain is given and not 0, else 0.0.
Build the dict with member as text (or None), a copy of params, seed as int, peak_db and gain_db as floats. Add
`gain` only when a gain was given.

### `raw(b)` and `level(x, b)`

`raw`: call the resolved function with a copy of params and `rng(seed)`; convert to float64. Return None when the
output is empty, has a non-finite value, or its peak is under 1e-7.
`level`: normalise to `peak_db` (default PEAK_DB) and convert to float32. Gain = `gain` when present, else
10^(gain_db / 20) (gain_db missing = 0). When the gain is not exactly 1.0, multiply in float64 and convert to float32
again. The float32 rounding before the gain is part of the contract: it reproduces the written samples.

### `compare(x, y)`

x is a render (a mono render becomes 1 column). y is a WAV read as 2-D (frames, channels). Different shapes give
infinity. An empty array gives 0.0. Else the max absolute difference.

### `check_final(report, folder=None, tol=TOL, workers=None)`

`report` is a folder (its report.json), a path to a .json file, or a dict (then `folder` is required for the WAVs).
For each pick, in parallel threads when the worker count (part 7 worker rule) is above 1 and there is more than 1
pick, build a row:
- `name`, `file` (the basename of the pick's `file`, else `<name>.wav`), `source` (`block` when the pick has its
  own render dict, else `legacy`).
- The block: `recipe`, `fn`, `member`.
- A missing WAV: err inf, ok false, why `WAV missing`.
- Else read the WAV as 2-D and compare with the re-render.
- NotReproducible: err inf, why = the message.
- ok = err <= tol.
Rows keep the pick order.

### `fill(b, x=None)`

Completes a block's params so that later DEFAULTS changes cannot change a re-render.
1. x = the raw render (or the given one). None: return b unchanged.
2. Candidate DEFAULTS tables, in order, without duplicates (by identity): the module's `DEFAULTS` when fn is `render`;
   the `DEFAULTS` of every module-level object whose `render` attribute is the resolved function; every module-level
   dict whose name ends with `DEFAULTS`.
3. For each table: the missing keys are the table's keys that the params lack. When none, skip. Else try params =
   missing overlaid by current params. Keep the candidate only when its raw render has the same shape and exactly
   equal samples (any exception skips the table).

### `upgrade(report, write=False, tol=TOL, folder=None, log=print)`

For each pick: has = the pick has a render dict. b = block(report, pick). x = raw(b). bf = b when it has a block,
else fill(b, x). err = compare(level(x, bf), WAV), or inf when x is None. Row {name, err, ok, filled (number of keys
fill added), had_block}. When the pick had no block, set its `render` to bf.
When every row is ok, set the report's `schema` to 2.
With write: when any row failed, raise NotReproducible `not written: <name> err <e>, ...`. Else write report.json
through a temporary file `report.json.tmp<pid>` and an atomic replace (indent 1; arrays as lists; other objects as
text), and log `wrote <path>: <n> render blocks (schema 2)`.

### Legacy reports

Reports from before schema 2 have no render block. `legacy_block` infers one:
- Module: the report's `recipe_module`; else the LEGACY entry of the report's `recipe` (a driver's stand-in name);
  else the `recipe` itself, with function `render`. A name that does not import raises NotReproducible (`recipe <x>
  is no module of hark.forge.recipes and not in schema.LEGACY`).
- Params: the pick's params. When they are only `{'variant'}`: the pick's `family_params`, else
  `report['family']['members'][variant]`, else raise NotReproducible.
- Member: the first of the pick's `member`, `family_name`, `family`, `preset` that is text or an int (not a bool),
  else `family_member` as text; else, when `report['family']['labels']` is a list, the label at the pick's index.
- Mode: `report['critic']['mode']`, else `report['mode']`, when it is text.
- gain_db: the pick's `gain_db` or 0.

A clean build has no legacy reports. Implement `legacy_block` for the generic fields (`recipe_module`, `recipe`,
`family_params`, `family.members`, `family.labels`) and keep `LEGACY` as an empty table that maps a stand-in name to
(module, function). The source's table mapped 9 category names to its own module names; our recipe modules have our
own names (units recipes-impacts and recipes-motion), so the entries do not carry over.

## Constants

SCHEMA 2; TOL 1e-6; PEAK_DB -1.0; one 24-bit step is 2^-23, about 1.19e-7 (the measured worst re-render error over
183 delivered picks in the source was 1.19e-7).

## Errors and edge cases

- A lambda render function: `make_block` returns None; `forge()` and `palette()` then drop the report `schema` key
  (part 1), and `check_final` uses the legacy inference.
- A missing WAV, a shape difference (mono render vs stereo WAV), a failed or silent render: err inf, ok false.
- `upgrade(write=True)` never writes a partly matching report.
- The write is atomic: a crash leaves the old report.json or the new one, never half of it.

## Acceptance tests

S1. A forge run with `out=D` followed by `check_final(D)` gives ok true for every pick and max err <= 1.2e-7.
S2. A palette run (gains other than 1) passes `check_final` with max err <= 1.2e-7.
S3. Editing one pick's `seed` in report.json makes that row fail (err > 1e-3) and the CLI exit 1.
S4. Deleting one WAV gives a row with why `WAV missing` and err inf.
S5. `make_block(lambda p, r: ..., {}, 0)` is None; `make_block(None, {}, 0, ref=('m', 'render'))` is a dict with
    gain_db 0.0 and no `gain` key; with gain 0.5 its gain_db is -6.02 and `gain` is 0.5.
S6. `fill` adds a missing key from DEFAULTS only when the render stays bit-identical: a recipe that reads
    `p.get('k', 1.0)` with DEFAULTS `{'k': 1.0}` gets `k` added; with DEFAULTS `{'k': 2.0}` it does not.
S7. `upgrade(D, write=True)` on a report without render blocks, whose picks all re-render, writes `schema: 2` and a
    render block on every pick; when 1 pick fails it raises NotReproducible and report.json is unchanged.
S8. `compare(np.zeros(10), np.zeros((10, 2)))` is inf; `compare(np.zeros(10), np.zeros((10, 1)))` is 0.0.
