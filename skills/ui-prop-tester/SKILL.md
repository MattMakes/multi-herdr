---
name: ui-prop-tester
description: Use when a web page or a terminal program needs more than a scripted path - to find the click, key or navigation order that breaks it, check an invariant across thousands of random interactions, fuzz a form, REPL or full-screen TUI, replay a recorded failure after a fix, or cut a long failing walk down to its shortest steps. ui-prop-tester drives Chromium or a pseudo-terminal with seeded random actions and checks temporal properties after every step.
---

# ui-prop-tester: random walks with properties

A scripted test follows 1 path. `ui-prop-tester` walks a UI at random and checks rules after every step. You
write the rules (properties) and, when needed, the moves (action generators). Every run has a seed and writes
a trace, so each failure can be replayed and shrunk.

Rule: a claim that a UI "holds up" needs a run behind it. Report the command, the seed, the exit code and the
violation text. If you did not run it, say so.

## Run it

- Launcher: `scripts/ui-prop-tester` in this skill folder (`skills/ui-prop-tester/scripts/ui-prop-tester` in
  the repo). Call it by path. It runs `UI_PROP_TESTER_BIN`, else `ui-prop-tester` on `PATH`, else a cached
  build in `~/.local/state/horch/ui-prop-tester/bin/`. In a multi-herdr checkout it builds that cache on
  first use (a few minutes) and rebuilds when the source is newer. Outside a checkout with no binary it
  exits 3 and prints the `cargo install` command; show it to the user.
- Browser runs need Chrome, Chromium or Edge. `--chrome <FILE>` or `UI_PROP_TESTER_CHROME` names one.
- Terminal runs need macOS or Linux.

## Which command

| need | command |
|---|---|
| Walk a page with the built-in rules | `browser test --headless <ORIGIN>` |
| Walk a page with your rules | `browser test --headless --specification spec.js <ORIGIN>` |
| Walk a page in a running Chromium or Electron app | `browser test-external --remote-debugger <URL> <ORIGIN>` |
| Walk a CLI, REPL or TUI | `terminal test [OPTIONS] <PROGRAM> [ARGS...]` |
| Many short walks | `browser fuzz ...` or `terminal fuzz ...` |
| Check a recorded failure after a fix | `browser test --reproduce t.trace ...` or `terminal test --reproduce ...` |
| Shortest failing steps | `browser shrink --trace t.trace --output-path min.trace` (or `terminal shrink`) |
| Read a trace | `trace t.trace` (`--state`, `--json`, `--screen`, `--screenshots`) |
| Look at a browser trace | `browser inspect t.trace` |

`<ORIGIN>` is an `http://` or `https://` URL or a local HTML file path.

## Exit codes

| code | meaning | what to do |
|---|---|---|
| 0 | The run ended with no violation (limit reached, clean interrupt, replay passed) | Report the seed and the limits |
| 1 | Operational error: bad arguments, setup failure, stalled or hung target, no action available, replay diverged, shrink failed | Read stderr; fix the run, not the product |
| 2 | 1 or more properties were violated | Read the violation, save the trace, shrink it |
| 3 | The launcher found no binary | Show the install command |

Exit 1 is never a finding about the product, and exit 0 is only as strong as the limits you set.

## The loop to follow

1. **Start with defaults.** Run with no specification and a limit:
   `scripts/ui-prop-tester browser test --headless --time-limit 60s --output-path run.trace <ORIGIN>`.
   A run needs a limit (`--time-limit` or `--actions-max`), or it goes on until a violation with
   `--exit-on-violation`, an interrupt, or the program's exit.
2. **Read the result.** The summary prints the seed, the state count and each violation with the property
   name, the state index, the formula path and the cell values it read.
3. **Write the rules that matter.** Defaults catch crashes. Product rules ("the cart total equals the sum of
   its rows", "the status line never shows a stack trace") need a specification. Keep the defaults by
   re-exporting them. See [specifications](references/specifications.md).
4. **Widen.** `fuzz --parallelism 4 --time-limit-fuzz 10m --time-limit-run 30s --swarm --output-path
   out/run.trace` runs many short walks; run N writes `out/run-N.trace`.
5. **Shrink a failure.** `shrink --trace run-7.trace --output-path min.trace`. Then `trace min.trace` gives
   the short action list for the bug report.
6. **After the fix**, `test --reproduce min.trace` with the same target and specification must exit 0. Keep
   the trace and the command as the regression check.

## Options every run takes

- `--seed <SEED>` (or `UI_PROP_TESTER_SEED`): the same seed, target and specification give the same choices.
  With no seed, 1 is picked and printed. Always quote the seed.
- `--specification <FILE>`: an ES module, `.js`, `.mjs` or `.ts`.
- `--time-limit <DURATION>` (`500ms`, `30s`, `5m`, `1h`), `--actions-max <COUNT>`.
- `--exit-on-violation`: stop at the first violation. Without it the run goes on and reports all of them.
- `--output-path <FILE>`: the trace. The folder must exist. An existing file is refused without
  `--output-path-overwrite`. With no path the trace goes to the temporary folder and the summary names it.

## A specification in 20 lines

```js
import { always, eventually, extract, implies, next, now } from "ui-prop-tester";
export * from "ui-prop-tester/browser/defaults";

export const count = extract((state) => Number(state.document.querySelector("#count")?.textContent ?? 0));
export const saving = extract((state) => !!state.document.querySelector(".saving"));

export const countNeverNegative = always(() => count.current >= 0);
export const savingEnds = always(implies(() => saving.current, eventually(() => !saving.current).within(5, "seconds")));
export const countStepsByOne = always(now(() => {
  const before = count.current;
  return next(() => Math.abs(count.current - before) <= 1);
}));
```

- `extract(fn)` makes a cell: a pure function of the captured state. Read it with `.current`.
- An exported formula is a property; its export name is the name in reports and `--property`.
- An exported generator is an action category. Leave a default out of the re-export to switch it off.
- Browser extractors run in the page. They see `state.document`, `state.window` and the captured fields,
  and nothing from the module around them.

The full API, both state shapes, generators and worked patterns: [specifications](references/specifications.md).

## What the defaults check

- Browser: no navigation response with status 400 or above, no uncaught exception, no unhandled promise
  rejection, no console entry at error level. Actions: clicks on visible controls (also in same-origin
  iframes, open shadow roots and slots), typing into the focused field, scrolling, back, forward, reload,
  wait. The walk stays on the starting scheme, host and port; `--allow-url <MATCH_PATTERN>` replaces that.
  Details and flags: [browser](references/browser.md).
- Terminal: a normal exit has code 0, and no cell shows U+FFFD (malformed output). Actions: safe Unicode
  text, Enter and Tab. It never sends Ctrl+C, Ctrl+D or Ctrl+Z unless your specification does.
  Details and flags: [terminal](references/terminal.md).

## Mistakes to avoid

- Reporting exit 0 from a 5-second run as "no bugs". State the limits and the action count.
- Treating exit 1 as a product bug. `no actions are available in state N` means the generators offer
  nothing there: a dead-end page, a boundary that is too tight, or a guard that is always false.
- Writing an `eventually` with no `.within`. It only fails at the end of the run. Give it a deadline.
- Reading 1 cell inside another cell's extractor. It throws. Compute both from `state`.
- Closing over module variables in a browser extractor. The function is sent to the page as text.
- Shrinking a flaky failure. `shrink` first replays the trace `--calibration-runs` times and stops if it
  does not fail every time. Make the target deterministic (fixed data, no live clock) before you shrink.
- Pointing a browser run at a site you do not own. Keep the default boundary.

Traces, replay and shrink in depth: [traces](references/traces.md).
