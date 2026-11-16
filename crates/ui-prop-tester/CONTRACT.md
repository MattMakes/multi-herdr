# ui-prop-tester build contract

Working contract for the people building this crate in parallel. The product
spec is `docs/specs/ui-prop-tester.md`; this file fixes the names and shapes
the parts share. It is removed before the pull request; its content moves to
the skill's reference pages.

## Rules for all code in this crate

- No code comments at all, in Rust, JavaScript, HTML, shell or tests. No
  licence text, notices or credit to any other project. Names carry meaning.
- The design is our own. Do not look up or copy any other tool's source.
- `cargo clippy -p ui-prop-tester --all-targets -- -D warnings` and
  `cargo fmt -p ui-prop-tester` stay clean.
- The crate compiles on Windows (`cargo build`); Unix-only code sits behind
  `cfg(unix)` with a clear error on other systems.
- Tests check outputs at public boundaries: exit codes, stdout, stderr, trace
  files. Tests never raise a timeout to pass; a new executable is run once
  untimed before a timed call (macOS scans a new executable for 14-35 s).
- Never read, set or pass `ANTHROPIC_API_KEY`.
- Do not add a dependency unless the work cannot be done without it; say so
  in your report when you do.

## Modules and owners

| Path | Owner | Holds |
|---|---|---|
| `src/driver.rs`, `src/trace.rs`, `src/cli.rs`, `src/rng.rs`, `src/duration.rs`, `src/output_path.rs`, `src/lib.rs`, `src/main.rs` | lead (frozen; ask before changing) | shared types |
| `src/spec/**`, `src/js/core.js`, `src/engine.rs`, `src/fuzz.rs`, `src/shrink.rs`, `src/fake.rs`, `src/commands.rs`, `tests/engine_*.rs` | engine | specification runtime, run loop, replay, fuzz, shrink, fake driver |
| `src/terminal/**`, `src/js/terminal.js`, `src/js/terminal_defaults.js`, `tests/terminal_*.rs` | terminal | PTY driver, emulator, terminal JS module |
| `src/browser/**`, `src/js/browser.js`, `src/js/browser_defaults.js`, `tests/browser_*.rs`, `tests/support/**` | browser | CDP driver, page scripts, browser JS module, fixture server |
| `src/trace_cmd.rs`, `src/inspect.rs`, `src/inspect/**`, `tests/trace_*.rs`, `tests/inspect_*.rs` | trace tools | `trace` command, `browser inspect` viewer |

Stay inside your paths. If a frozen file must change, make the smallest
change and name it in your report.

## Entry point and exit codes

`ui_prop_tester::run(args, &mut Io, &dyn DriverFactory) -> i32`. Tests call
it with buffers for `Io` and, for engine tests, a factory that returns the
fake driver. Exit `0`: normal end (time limit, action limit, clean interrupt,
`--reproduce` with no violation). Exit `1`: bad arguments, setup or driver
error, stalled target, no available action, replay divergence, failed shrink.
Exit `2`: one or more property violations. Errors go to stderr as
`ui-prop-tester: error: <message>`.

## Run loop (engine)

1. `driver.prepare(setup)` then `driver.start()` gives state 0.
2. For each state: compute cells (local extractors in the JS engine, or
   `driver.extract` when `driver.extracts_remotely()`), step every property,
   record a `TraceState`.
3. Stop when: `observation.ended` is true; a violation with
   `--exit-on-violation`; `--time-limit`; `--actions-max`; the replay list is
   used up; an interrupt.
4. Otherwise sample one action from the generator tree (or take the next
   recorded action in replay) and call `driver.apply(action)`.
5. At the end resolve pending obligations (`always` true, `eventually` false,
   unmet `next` false only when its obligation was active), write the trace,
   print the summary with the seed, call `driver.shutdown()` always.

Without `--time-limit`, `--actions-max` or `--reproduce`, a `test` run
continues until a stop condition above; `fuzz` runs default to
`--time-limit-run 30s`.

## Specification modules (JavaScript, own API)

A specification is an ES module (`.js`, `.mjs`, or `.ts` with type
annotations stripped). With no `--specification`, the driver's defaults
module is the specification. Exports are classified by runtime tag: formulas
are properties (named by the export identifier); generators are action
categories (named by the export identifier); other exports are ignored.

### `ui-prop-tester` (core, `src/js/core.js`)

Cells:

- `extract(fn)` returns a cell. `fn(state)` is pure and returns a
  JSON-serialisable value. `cell.current` is the value for the current
  state. Reading any cell while an extractor is running throws
  `Error("an extractor cannot read another extractor's cell")`. An extractor
  that throws aborts the run with exit `1` and prints its JavaScript stack.
- Browser extractors run inside the page (the engine sends
  `fn.toString()`), so they cannot close over module variables. Terminal and
  fake-driver extractors run in the engine.

Formulas:

- `now(fn)`: `fn()` returns a boolean, or a formula that is then stepped
  from the current state. A bare function passed where a formula is expected
  is promoted to `now(fn)`.
- `always(f)`, `eventually(f)`, `next(f)`.
- `always(f).within(n, unit)`, `eventually(f).within(n, unit)`; unit is
  `"milliseconds"` or `"seconds"`; time is the state timestamp.
- `and(...fs)`, `or(...fs)`, `not(f)`, `implies(a, b)`.
- Three-valued finite-trace semantics: true, false, pending.

Generators (every generator has `.map(fn)`, `.filter(fn)`, `.flatMap(fn)`,
`.when(condition)` and `.weight(n)`; `condition` is a boolean, a cell, or a
function; `weight` is a nonnegative integer used where the generator sits in
a choice, default 1):

- `constant(value)`, `from(array)`, `integer(min, max)`, `float(min, max)`,
  `regex(patternOrRegExp)`, `email()`, `text({ min, max })` (Unicode),
  `chars(characters, { min, max })`.
- `action(template)`: a leaf. `template` is an object with a string `kind`;
  any field may be a generator, sampled when the leaf is chosen.
- `oneOf(...generators)`, `weighted([[weight, generator], ...])`.
- `actions(fn)`: `fn()` runs in every state and returns a generator, an
  array of generators (treated as `oneOf`), or `null`/`undefined`/`[]` for
  "nothing to offer now".
- The top level is `oneOf` over all exported generators, each with its
  `.weight`. A choice whose available weight is 0 is unavailable; if the top
  level is unavailable the run ends with exit `1` and
  `no actions are available in state <index>`.
- All randomness comes from the run's `SplitMix64` (`src/rng.rs`).

### `ui-prop-tester/terminal` (`src/js/terminal.js`)

Action builders (each returns a generator leaf): `write(textOrGenerator)`,
`writeBytes(arrayOfNumbers)`, `resize(columns, rows)`, `click(row, column)`,
`scrollToTop()`, `scrollToBottom()`. Key generators: `controlKeys` (Enter,
Tab), `editingKeys`, `arrowKeys`, `modifiedArrowKeys`, `navigationKeys`,
`functionKeys`, `bracketedPaste(textOrGenerator)`, `safeText` (the safe
Unicode set).

`ui-prop-tester/terminal/defaults` (`src/js/terminal_defaults.js`) exports
the properties `exitsCleanly` and `noReplacementCharacters` and the
generator `typing`.

### `ui-prop-tester/browser` (`src/js/browser.js`)

Action builders: `click(candidate)`, `doubleClick(candidate)`,
`type(textOrGenerator)`, `key(name)`, `scrollBy(dy)`, `scrollToTop()`,
`back()`, `forward()`, `reload()`, `wait(ms)`,
`drag({ from, to, steps, delayMs })`, `resize(width, height)`,
`setFiles(selector, paths)`, `custom(name, args)`,
`defineAction(name, asyncFn)` (registers a page-side handler
`asyncFn({ document, window, cells, args })`), and the formula helper
`resourceGrowth({ metric, window, minimumGrowth })` (fails when `metric`
grew in every one of the last `window` states by a total of at least
`minimumGrowth`). Cells exported for reuse: `page` (the whole captured state
below).

`ui-prop-tester/browser/defaults` (`src/js/browser_defaults.js`) exports the
properties `noHttpErrors`, `noUncaughtExceptions`, `noUnhandledRejections`,
`noConsoleErrors` and the generators `clicks` (weight 100), `inputs` (100),
`scrolling` (50), `navigation` (10), `waiting` (1).

A specification keeps defaults by re-exporting them:
`export * from "ui-prop-tester/browser/defaults";` or by name.

## Action shapes (JSON)

Every action is an object with a string `kind`. The engine adds `category`
(the export name of the generator that produced it).

Terminal: `{kind:"write", text}` or `{kind:"write", bytes:[...]}`;
`{kind:"resize", columns, rows}`; `{kind:"click", row, column}` (zero-based,
sent as an SGR primary-button press and release);
`{kind:"scroll_to_top"}`; `{kind:"scroll_to_bottom"}`.

Browser: `{kind:"click", target, point}` and `{kind:"double_click", target,
point}` where `target` is a fingerprint and `point` is `{x, y}` in viewport
pixels; `{kind:"type", text}`; `{kind:"key", key}` (`Backspace`, `Tab`,
`Enter`, `Escape`, ...); `{kind:"scroll", dy}`; `{kind:"scroll_to_top"}`;
`{kind:"back"}`; `{kind:"forward"}`; `{kind:"reload"}`;
`{kind:"wait", ms}`; `{kind:"drag", from, to, steps, delay_ms}`;
`{kind:"resize", width, height}`; `{kind:"set_files", selector, paths}`;
`{kind:"custom", name, args, cells}` (the engine fills `cells` with the
exported cells by name).

Fingerprint: `{test_id, id, role, name, href, form_name, placeholder,
input_type, text, path}`; absent fields are omitted. Replay matches a
recorded click to the offered candidate that agrees on the earliest of those
keys in that order, and uses the candidate's current `point`.

Fake driver: `{kind:"step", name}` and whatever its script declares.

## State shapes (JSON, `Observation.state`)

Terminal:

```json
{
  "columns": 100, "rows": 40,
  "lines": ["one string per visible row, trailing blanks trimmed"],
  "cells": [{"row": 0, "column": 0, "text": "a", "bold": true}],
  "cursor": {"row": 0, "column": 0, "visible": true, "blink": true, "shape": "block", "color": null},
  "scroll_offset": 0,
  "scrollback": {"lines": [], "cells": []},
  "exit": null,
  "last_action": null
}
```

`cells` is sparse: it lists only cells that are not a default blank. A cell
carries `row`, `column`, `text` and only its non-default attributes:
`wide_continuation`, `fg`, `bg`, `underline_color` (each
`{"indexed": n}` or `{"rgb": [r, g, b]}`), `underline` (`single`, `double`,
`curly`, `dotted`, `dashed`), and the booleans `bold`, `italic`, `blink`,
`inverse`, `strikethrough`, `dim`, `invisible`, `overline`. `cursor.shape`
is `block`, `underline` or `bar`. `exit` is `null` while the child runs,
then `{"code": 0, "signal": null}` or `{"code": null, "signal": 15}`.
The terminal JS module wraps this for extractors: `state.cell(row, column)`
returns a full cell object with every attribute filled with its default,
and `state.scrollback.cell(row, column)` does the same.

Browser:

```json
{
  "url": "", "title": "", "mime_type": "text/html",
  "viewport": {"width": 1024, "height": 768, "device_scale_factor": 1},
  "navigation": {"status": 200, "kind": "load"},
  "history": {"index": 0, "entries": [{"url": "", "title": ""}]},
  "console": [{"level": "error", "text": "", "url": "", "line": 0}],
  "exceptions": [{"kind": "exception", "message": "", "url": "", "line": 0, "column": 0, "stack": ""}],
  "resources": {"js_heap_used": 0, "js_heap_total": 0, "dom_nodes": 0, "event_listeners": 0, "layout_objects": 0},
  "coverage": null,
  "scroll": {"x": 0, "y": 0, "max_x": 0, "max_y": 0},
  "document": {"is_html": true, "has_body": true},
  "focused": null,
  "clickables": [{"kind": "link", "tag": "a", "target": {}, "point": {"x": 0, "y": 0}}],
  "downloads": [],
  "last_action": null
}
```

`navigation.kind` is `load`, `same_document`, `non_document` or `bfcache`;
`navigation.status` is `null` when there was no HTTP response.
`console` holds warning and error entries of the current page and is cleared
on navigation; `exceptions[].kind` is `exception` or `rejection`.
`focused` is `null` or `{"tag", "input_type", "value_length", "target"}`.
`coverage` is `null` or `{"branches_new": n, "branches_total": n}`.
In the page, extractors receive this object plus `document` and `window`.

## Trace file

`src/trace.rs` is the format: `TraceHeader` (driver, seed, `target` = the
serialised `BrowserConfig` or `TerminalConfig`, the specification path and
source), `TraceState` records each followed by its PNG screenshot when
present, then a `TraceSummary` whose `outcome` is `passed`, `violation` or
`error`. `TraceState.cells` is an object keyed by exported cell name.
