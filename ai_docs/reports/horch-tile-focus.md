# `horch tile` takes over the operator's focused pane

Worker `opus-2`, branch `horch-tile-keep-focus`, herdr 0.8.2.

All observations come from scratch workspace `w1R`, created with
`herdr workspace create --label focus-repro --no-focus`. The live fleet
workspace `w1Q` was never tiled.

## 1. Before the fix: 4 observations

Each row is one run. "focused pane" is `focused_pane_id` of the viewed tab,
read with `herdr pane layout --pane <any pane of that tab>`. "viewed tab" is
`active_tab_id` of the workspace, read with `herdr workspace list`.
`w1R:p1` is the orchestrator pane in every row.

| # | Case | Viewed tab before | Focused pane before | Viewed tab after | Focused pane after | Kept? |
|---|------|-------------------|---------------------|------------------|--------------------|-------|
| A | `horch tile`, worker focused on tab 1 | `w1R:t1` | `w1R:p3` | `w1R:t1` | `w1R:p1` | tab yes, pane NO |
| B | `horch tile`, viewed tab 2 emptied by the tiling | `w1R:t3` | `w1R:p5` | `w1R:t1` | `w1R:p1` | tab NO, pane NO |
| C | `horch tile`, viewed overflow tab, 8 workers | `w1R:t6` | `w1R:p5` | `w1R:t1` | `w1R:p1` | tab NO, pane NO |
| D | `horch spawn smoke`, worker focused on tab 1 | `w1R:t1` | `w1R:p2` | `w1R:t1` | `w1R:p1` | tab yes, pane NO |

The focused pane becomes the orchestrator pane in all 4 cases.

### Exact command lines

Scratch workspace and a deliberately bad shape (1 orchestrator, 3 workers in
one column):

```
herdr workspace create --label "focus-repro" --no-focus        # -> w1R, root pane w1R:p1
herdr pane split w1R:p1 --direction right --no-focus           # -> w1R:p2
herdr pane split w1R:p2 --direction down  --no-focus           # -> w1R:p3
herdr pane split w1R:p3 --direction down  --no-focus           # -> w1R:p4
```

Case A:

```
herdr pane focus --direction right --pane w1R:p1               # focus -> w1R:p2
herdr pane focus --direction down  --pane w1R:p2               # focus -> w1R:p3
target/debug/horch tile --workspace w1R
```

Case B:

```
herdr tab create --workspace w1R --label "extra" --focus       # -> tab w1R:t3, root pane w1R:p5
target/debug/horch tile --workspace w1R
```

Case C (8 workers, so the overflow tab holds 4 and survives as a tab):

```
herdr tab focus w1R:t6
herdr pane focus --direction right --pane w1R:p4               # focus -> w1R:p5 on the overflow tab
herdr pane split w1R:p2 --direction down --no-focus            # 8 workers, so the shape is not canonical
target/debug/horch tile --workspace w1R
```

Case D (isolated ledger and state, so the live fleet ledger is untouched):

```
herdr tab focus w1R:t1
herdr pane focus --direction right --pane w1R:p1               # focus -> w1R:p2
env HORCH_WORKSPACE_ID=w1R HORCH_PROJECT_DIR=$TMPP HORCH_STATE_DIR=$TMPS \
  target/debug/horch spawn smoke "focus repro" --from-pane w1R:p1
```

### One more fact: the overflow tab gets a NEW id on every rebuild

Case C shows it. The viewed tab `w1R:t6` did not survive. The tiler parks
every worker in a scratch tab, so the overflow tab loses its last pane and
herdr closes it. The tiler then creates a new overflow tab, `w1R:t8`. A
restore that only matches the OLD tab id therefore fails for every overflow
tab. The restore must find the tab that now holds the remembered pane.

## 2. Verdict on `herdr plugin pane focus <pane_id>`

**It does not work for an ordinary pane.** It is only for plugin-owned panes.

```
$ herdr plugin pane focus w1R:p4
{"error":{"code":"plugin_pane_not_found","message":"plugin pane not found"},"id":"cli:plugin"}
$ echo $?
1
```

`focused_pane_id` of `w1R:t1` stayed `w1R:p3`. The command changed nothing.
This confirms section 1 line 41 of `ai_docs/reports/horch-tile-herdr-surface.md`:
herdr 0.8.2 has no focus-by-id for an ordinary pane. The fix therefore uses a
neighbour walk with `herdr pane focus --direction`.

### `herdr pane focus --direction` facts the walk relies on

- It works on a workspace that is NOT the focused workspace.
- It accepts `--pane <ID>` as the origin, so the walk never needs a
  second `pane layout` call.
- It returns the post-move layout, `changed`, and `focused_pane_id` in one
  reply.
- With no neighbour on that side it returns exit 0 and
  `"changed":false,"reason":"no_neighbor"`. That is the walk's loop breaker.
- WARNING: it pulls the whole workspace into view. `workspace list` reported
  `"focused":false` for `w1R` before the call and `"focused":true` after it.
  `herdr tab focus` does the same, so `horch tile` already had this effect.
  Use `herdr workspace focus <ws>` to put the operator's view back.

## 3. The cause, in 3 sentences

`crates/horch/src/cmd/tilecmd.rs:430-437` restores only the viewed TAB after
`apply`, with `herdr.tab_focus(active)`, and never restores the focused pane.
The pane focus is lost inside `apply`, because the park phase moves every
worker out of its tab with `--no-focus`, and herdr then hands that tab's focus
to a pane that stayed, which on tab 1 is always the orchestrator; the direct
probe below shows it. Every later `pane_move` also passes `--no-focus`
(`crates/horch-core/src/herdr.rs:459, 471`), so nothing ever gives the focus
back.

Direct probe of the mechanism:

```
$ herdr pane layout --pane w1R:p1 | jq -r .result.layout.focused_pane_id
w1R:p2
$ herdr pane move w1R:p2 --new-tab --label probe --no-focus
$ herdr pane layout --pane w1R:p1 | jq -r .result.layout.focused_pane_id
w1R:p1
```

`crates/horch/src/cmd/spawn.rs` needs no change for case D. Its split already
passes `--no-focus`: `Herdr::pane_split` hardcodes the flag and takes no
`focus` parameter (`crates/horch-core/src/herdr.rs:304-315`). The plan's
reference to a `focus` value at `herdr.rs:505` is stale. Case D loses the
focused pane inside the auto-tile that `spawn` calls at
`crates/horch/src/cmd/spawn.rs:203`, which is the same cause as case A, so the
restore in `tilecmd.rs` fixes it.

## 4. The fix

Three pure additions to `crates/horch-core/src/tile.rs`:

- `FocusState { tab, pane }` - where the operator was looking.
- `FocusTarget { tab, pane, kept, reason }` - where the view belongs after.
- `focus_target(before, after, orchestrator)` - the rule, over `TabShape`
  values the command already builds. No herdr call inside.
- `step_toward(from, to)` - one neighbour direction, for the walk.

Two additions to `crates/horch-core/src/herdr.rs`:

- `FocusDir { Left, Right, Up, Down }`, `Focus`, and
  `pane_focus(origin, direction)`. `FocusDir` is separate from `Direction`
  on purpose: `Direction` is split geometry, and its `FromStr` rejects
  `left` and `up`.
- `pane_focus_walk(tab, target)` - steps the focus to a pane by id. It returns
  `false` instead of an error, because a view that cannot be restored must
  never fail a tiling that already moved the panes.

In `crates/horch/src/cmd/tilecmd.rs`: `Snapshot::focus_state()` reads both
values from the gather that already happened, so the capture costs no herdr
call. `restore_focus` runs after the column balance, so the view settles once
on the finished grid. The fast path (`already` canonical) skips the restore
entirely: nothing moved, so nothing needs putting back, and a focus call would
pull the workspace into view for no reason.

`crates/horch/src/cmd/spawn.rs` is UNCHANGED. See section 3.

### A correction to the planned `step_toward` rule

The plan specified "the neighbour direction whose axis has the larger centre
distance". That rule fails on the shape the grid ends with whenever a tab is
not full. Case A's grid is one of them:

```
w1V:p1@(26,1,127x59)  w1V:p2@(153,1,128x30)  w1V:p4@(281,1,127x30)
                      w1V:p3@(153,31,255x29)   <- spans both columns
```

`w1V:p3` spans both worker columns, so its centre (x 280) sits LEFT of
`w1V:p4`'s centre (x 344) even though `w1V:p3` is directly BELOW `w1V:p4`.
The centre rule therefore answered `Left` where the answer is `Down`, and the
walk oscillated `p2 -> p4 -> p2` until the step budget ran out. The first run
of case A after the fix printed:

```
focus: kept w1V:t1 w1V:p3 (herdr would not step the focus there)
```

The rule is now: the axis the two rects are SEPARATED on decides the
direction, and the centre distance only breaks the tie for a diagonal
neighbour, where they are separated on both. Test
`step_toward_goes_down_to_a_pane_that_spans_the_columns_above_it` locks it in.

## 5. After the fix: the same 4 observations

| # | Case | Viewed tab before | Focused pane before | Viewed tab after | Focused pane after | Kept? |
|---|------|-------------------|---------------------|------------------|--------------------|-------|
| A | `horch tile`, worker focused on tab 1 | `w1V:t1` | `w1V:p3` | `w1V:t1` | `w1V:p3` | YES |
| B | `horch tile`, viewed tab 2 emptied by the tiling | `w1V:t3` | `w1V:p5` | `w1V:t1` | `w1V:p5` | pane YES, tab followed it |
| C | `horch tile`, viewed overflow tab, 8 workers | `w1V:t6` | `w1V:p5` | `w1V:t8` | `w1V:p5` | pane YES, tab followed it |
| D | `horch spawn smoke`, worker focused on tab 1 | `w1V:t1` | `w1V:p2` | `w1V:t1` | `w1V:p2` | YES |

The lines the command printed:

```
A  focus: kept w1V:t1 w1V:p3
B  focus: moved to w1V:t1 w1V:p5 (the viewed tab closed, so the view follows the pane)
C  focus: moved to w1V:t8 w1V:p5 (the viewed tab closed, so the view follows the pane)
D  (horch spawn prints the tiling to stderr; the pane and tab are unchanged)
```

In B and C the viewed tab no longer exists, so rule 1 sends the view to the
tab that now holds the pane. That is the wanted behaviour: the operator keeps
watching the same worker.

### One case that keeps today's behaviour on purpose

Between B and C the shape grew from 4 workers to 7. The focused pane `w1V:p5`
moved to the NEW overflow tab, but the viewed tab `w1V:t1` still existed. Rule
1 keeps the viewed tab, and rule 2 then has no remembered pane on it, so the
focus goes to the orchestrator:

```
active_tab_id=w1V:t1   focused_pane_id=w1V:p1
```

This is the rule as the plan states it. The viewed TAB wins over the viewed
PANE.

## 6. The smoke checks

`horch smoke tile` now focuses a WORKER pane on tab 1 before each tiling and
asserts that both the viewed tab and the focused pane survive it. The
orchestrator pane is deliberately not the one watched: it is the pane the
tiler used to hand the keyboard to, so a check that watched it would pass
either way.

`target/release/horch smoke tile` (exit 0):

```
PASS: a bad shape became the canonical grid over 2 tabs, all 7 panes still running.
PASS: focus stayed on w1X:t1 w1X:p2.
PASS: horch spawn tiled the workspace with no placement flags and no tokens.
PASS: horch spawn left focus on w1X:p2.
PASS: four workers left, the remaining four are on tab 1, and the overflow tab is gone.
```

The tiling in part 1 printed its own line, which is the new one:

```
focus: kept w1X:t1 w1X:p2
```

`target/release/horch smoke fleet` (exit 0):

```
PASS: spawn, brief, register, ledger add/note/done, tell, and pane self-close all verified.
```

`cargo test --workspace`: 311 tests pass, 0 fail. 8 of them are new, all in
`crates/horch-core/src/tile.rs`.

`target/release/horch tile --plan` on the live fleet workspace `w1Q` is
read-only and reports the grid is already laid out.

## 7. Noticed, outside this task, left alone

- `herdr pane focus` and `herdr tab focus` both pull the whole WORKSPACE into
  view, not only the tab. `horch smoke tile` and `horch smoke fleet` build
  their scratch workspace with `--no-focus` but then tile it, so they take
  the operator's screen for the length of the run and leave it there. This is
  not new - `horch tile` has always called `tab focus` - and `herdr workspace
  focus <ws>` would put it back. Out of scope here.
- `cargo fmt --check -p <pkg> -- <files>` ignores the file list and checks the
  whole package. `crates/horch-core/src/herdr.rs`, `crates/horch/src/cmd/
  doctor.rs`, `install.rs`, `ledgercmd.rs` and `agent.rs` all carry
  pre-existing rustfmt diffs. Every line this task added is rustfmt-clean,
  checked by formatting a copy of each touched file and comparing the
  remaining diff with the same file on `main`.
