# Traces: read, replay, shrink

Every run writes 1 trace file (to `--output-path`, or to a file in the temporary folder that the summary
names): the target, the seed, the specification text, and for
each state its timestamp, the action that led to it, the cell values, the violations, and a screenshot
(browser) or screen (terminal). The file is self-contained.

Output rules, all commands: the folder of `--output-path` must exist; an existing file is refused without
`--output-path-overwrite`.

## Read

```
ui-prop-tester trace <TRACE_FILE> [--state <INDEX|START..END>] [--json] [--screen [--color]] [--screenshots <DIRECTORY>]
```

| flag | gives |
|---|---|
| none | Header, then 1 block per state: index, time, action, cells, violations |
| `--state 12` | State 12 |
| `--state 10..15` | States 10 to 14. `10..` and `..15` also work |
| `--json` | 1 JSON object per state, 1 per line. Pipe to `jq` |
| `--screen` | Terminal traces: the screen of each state. `--color` keeps colours and attributes |
| `--screenshots <DIRECTORY>` | Browser traces: `state-000012.png` per state. An identical file is kept; a different file with that name is refused |

Triage order for a failing trace:

1. `trace t.trace --json | jq -c 'select(.violations | length > 0) | {index, violations}'` finds the first
   failing state and the property.
2. `trace t.trace --state <N-3>..<N+1>` shows the actions that led to it.
3. Terminal: add `--screen`. Browser: `--screenshots shots/` and read the PNGs for those states.
4. Read the violation's cells: they are the values the failing formula saw.

The action list for a bug report: `trace t.trace --json | jq -c '.action | select(. != null)'`.

## Replay

```
ui-prop-tester browser test --reproduce t.trace [same target and options as the recorded run]
ui-prop-tester terminal test --reproduce t.trace [same options] <PROGRAM> [ARGS...]
```

The recorded actions run in order. Each one must still be offered by the generators in the state it is
applied to. Browser clicks are matched by element fingerprint (test attribute, id, role, name, link target,
form name, placeholder, input type, short text, then structural path), not by coordinates, so small layout
changes do not break a replay.

| result | meaning |
|---|---|
| exit 2 | The failure still happens |
| exit 0 | All actions ran and no property failed: the fix holds for this trace |
| exit 1, `diverged` | A recorded action is no longer offered: the UI changed shape. Record a new trace |

With no `--specification`, the replay reads the specification file the trace recorded, or the copy stored
in the trace when that file is gone. Pass `--specification` to replay with another one.

## Shrink

```
ui-prop-tester browser shrink --trace t.trace --output-path min.trace [OPTIONS]
ui-prop-tester terminal shrink --trace t.trace --output-path min.trace [OPTIONS]
```

| option | default | meaning |
|---|---|---|
| `--property <NAME>` | the only violated one | Required when the trace violates several properties |
| `--calibration-runs` | 3 | Replays of the input that must all fail before the search starts |
| `--confirmation-runs-max` | 5 | Most replays used to confirm 1 shorter candidate |
| `--parallelism` | 1 | Candidates replayed at the same time |
| `--time-limit` | none | Stop searching and keep the best result so far |
| `--specification` | from the trace | Use another specification file |

It takes the target and the specification from the trace, removes chunks of actions and then single actions,
and keeps a shorter list only when it still violates the property. `--output-path` must not be the input
trace, directly or through a symlink or hard link.

| result | meaning |
|---|---|
| exit 0 | `min.trace` holds the shortest confirmed failing list |
| exit 1, calibration | The input does not fail every time. The target is not deterministic: fix that first |
| exit 1, several properties | Pass `--property` |

## Fuzz campaigns

```
ui-prop-tester browser fuzz --headless --parallelism 4 --time-limit-fuzz 10m --time-limit-run 30s --swarm \
  --output-path out/run.trace <ORIGIN>
```

| option | default | meaning |
|---|---|---|
| `--parallelism` | 1 | Runs at the same time. Each browser run starts its own Chromium |
| `--time-limit-fuzz` | none | End of the campaign |
| `--time-limit-run` | 30s | End of each run |
| `--runs-max` | none | Stop after this many runs |
| `--actions-max` | none | Action limit per run |
| `--swarm` | off | Each run uses a random subset of the action categories, so runs differ in bias |
| `--seed` | random, printed | Campaign seed; each run derives its own and records it |
| `--exit-on-violation` | off | Stop the campaign at the first failing run |

Run N writes `out/run-N.trace`. The campaign exits 2 when any run had a violation. To redo run N alone, use
its seed from the summary with `test --seed`.

## What to put in a report

- The exact command and the seed.
- The exit code, and for exit 2 the property name, the state index and the cells from the violation.
- The shrunk action list, and the path of the shrunk trace.
- The limits (time, actions, runs). A clean result means "no violation within these limits".
