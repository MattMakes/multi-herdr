# herdr surface for `horch tile` (verification gate)

Verified against the installed `herdr 0.8.2` on darwin 25.5.0 on 2026-09-20, in
scratch workspaces created and closed for this probe (`horch-tile-surface`,
`horch-tile-ratio`, `horch-tile-shape`). Nothing in a live fleet workspace was
mutated. Docs compared: `herdr-docs/cli-reference.md:144-176` and
`herdr-docs/socket-api.md:225-232`.

## Gate verdict

`pane move` exists, keeps the public pane id, and keeps the running process
alive. The mutating code is safe to write. Two documented commands from the
brief are NOT needed and one of them is unsafe; see "Deviations" below.

## 1. Exact flags available

```
herdr tab list   [--workspace <WORKSPACE_ID>]
herdr tab create [--workspace <ID>] [--cwd <PATH>] [--label <TEXT>]
                 [--env <KEY=VALUE>] [--focus|--no-focus]
herdr tab close  <tab_id>
herdr tab focus  <tab_id>
herdr pane list  [--workspace <WORKSPACE_ID>]
herdr pane layout [--pane <ID>|--current]
herdr pane process-info [--pane <ID>|--current]      # --pane is a FLAG, not positional
herdr pane resize --direction left|right|up|down [--amount <FLOAT>] [--pane <ID>|--current]
herdr pane focus  --direction left|right|up|down [--pane <ID>|--current]   # neighbour only
herdr pane move <PANE_ID> --tab <TAB_ID> --split right|down
                [--target-pane <ID>] [--ratio <FLOAT>] [--focus|--no-focus]
herdr pane move <PANE_ID> --new-tab [--workspace <ID>] [--label <TEXT>] [--focus|--no-focus]
herdr pane move <PANE_ID> --new-workspace [--label <TEXT>] [--tab-label <TEXT>] [--focus|--no-focus]
```

The CLI matches `cli-reference.md:144-176` exactly. Three flag facts the brief
assumed differently:

- The `--new-tab` form takes `--label`, NOT `--tab-label`, and accepts neither
  `--split` nor `--ratio`. `herdr pane move <p> --new-tab --tab-label x --ratio 0.3`
  fails with the usage block.
- `--ratio` is accepted only on the `--tab` form.
- There is no "focus pane by id" command. `pane focus` is neighbour-based
  (`--direction`), so focus is restored with `tab focus <tab_id>`.

`pane split` in 0.8.2 also accepts `--ratio`, `--cwd` and `--env`
(`cli-reference.md:170`), which the wrapper's 0.6.1 pin does not use. Out of
scope here.

## 2. JSON shapes (trimmed real output)

`herdr tab list --workspace w0` — note that without `--workspace` it returns the
tabs of EVERY workspace:

```json
{"result":{"type":"tab_list","tabs":[
  {"tab_id":"w0:t1","workspace_id":"w0","label":"1","number":1,
   "pane_count":2,"agent_status":"working","focused":true}]}}
```

`herdr pane list --workspace w0` — every pane carries `tab_id`, so one call plus
one `pane layout` per tab covers a whole workspace:

```json
{"result":{"type":"pane_list","panes":[
  {"pane_id":"w0:p1","workspace_id":"w0","tab_id":"w0:t1","agent":"claude",
   "agent_status":"idle","focused":true,"cwd":"/Users/mascott/projects/multi-herdr",
   "agent_session":{"kind":"id","value":"05b62f2b-..."}}]}}
```

`herdr pane layout --pane w0:pE` — reports the whole TAB that holds the pane,
including `focused_pane_id`, `zoomed`, and the split list with ratios:

```json
{"result":{"type":"pane_layout","layout":{
  "workspace_id":"w0","tab_id":"w0:t1","zoomed":false,"focused_pane_id":"w0:p1",
  "area":{"x":26,"y":1,"width":184,"height":53},
  "panes":[{"pane_id":"w0:p1","focused":true,"rect":{"x":26,"y":1,"width":92,"height":53}},
           {"pane_id":"w0:pE","focused":false,"rect":{"x":118,"y":1,"width":92,"height":53}}],
  "splits":[{"id":"split_0_root","direction":"right","ratio":0.5,
             "rect":{"x":26,"y":1,"width":184,"height":53}}]}}}
```

`splits` is a flat list of nodes with an id, a direction, a ratio and a rect -
not the nested `first`/`second` tree that `socket-api.md` `layout.export`
returns. Parent/child is not stated, so `tile` drives its targets from the grid
it builds itself and from rects, never from this list.

`herdr tab create --workspace w19 --label horch-tile-scratch --no-focus` -
**it creates a shell pane too**:

```json
{"result":{"type":"tab_created",
  "tab":{"tab_id":"w19:t2","label":"horch-tile-scratch","number":2,"pane_count":1},
  "root_pane":{"pane_id":"w19:p4","tab_id":"w19:t2","workspace_id":"w19"}}}
```

`herdr pane move w19:p2 --tab w19:t2 --split right --target-pane w19:p4 --no-focus`:

```json
{"result":{"type":"pane_move","move_result":{
  "changed":true,
  "pane":{"pane_id":"w19:p2","tab_id":"w19:t2","workspace_id":"w19"},
  "previous_pane_id":"w19:p2","previous_tab_id":"w19:t1","previous_workspace_id":"w19",
  "focused_pane_id":"w19:p4",
  "source_layout":{"tab_id":"w19:t1","panes":[...],"splits":[...]},
  "target_layout":{"tab_id":"w19:t2","panes":[...],"splits":[...]}}}}
```

`herdr pane move w19:p3 --new-tab --label "workers 2" --no-focus` adds
`"created_tab":{"tab_id":"w19:t3","label":"workers 2","number":3,"pane_count":1}`
and the new tab holds ONLY the moved pane.

`herdr tab close w19:t2` returns `{"result":{"type":"ok"}}`.
`herdr pane resize` returns `{"result":{"resize":{"changed":bool,"layout":{...}}}}`
(already wrapped by `herdr.rs`).

Two response fields are conditional and must deserialize as `Option`:
`source_layout` is ABSENT when the move emptied and closed the source tab, and
`created_tab` is present only for `--new-tab`.

## 3. Does `pane move` keep the running process? YES

Method: three panes each ran `i=0; while true; do i=$((i+1)); echo TICK_<pane>_$i;
sleep 2; done`, then one was moved to another tab.

- Before the move, pane `w19:p2` printed `TICK_w19:p2_14`.
- `pane move` returned `changed: true` and the SAME `pane_id` (`w19:p2`), with
  `previous_pane_id` equal to it.
- 6 seconds after the move the same pane printed `TICK_w19:p2_15`, `_16`, `_17`.
- `herdr pane process-info --pane w19:p2` still reported `shell_pid: 2804` with
  the live `sleep 2` child.

`socket-api.md:232` states the same: a move keeps the terminal process and emits
no fake close/create events. Cross-WORKSPACE moves do assign a new public pane
id, so `tile` never uses `--new-workspace` - a changed pane id would break the
mailbox role -> pane registry.

## 4. Behaviours that shape the algorithm

1. **A same-tab move is a no-op.** `socket-api.md:231`: "moving to the source tab
   returns `changed: false` with `reason: "same_tab"`". Park-then-place is
   therefore mandatory, not a simplification - a pane can only be repositioned by
   leaving its tab first.
2. **An emptied tab closes itself.** Moving the last pane out of `w19:t1` removed
   the tab: `tab list` went from 3 tabs to 2 and the response omitted
   `source_layout`. So `tile` never needs `tab close` for source or scratch tabs.
3. **`tab close` kills the panes in the tab.** `tab close w19:t2` while `w19:p4`
   lived returned ok, and `pane get w19:p4` then answered
   `{"error":{"code":"pane_not_found"}}`. `cli-reference.md:152` adds that closing
   a workspace's last tab closes the workspace. `tile` must never call it.
4. **A zoomed tab refuses moves.** `socket-api.md:231`: moves involving a zoomed
   source or target tab return `changed: false` with `reason: "zoomed_tab"`.
   `pane layout` reports `zoomed`, so `tile` checks it before moving anything.
5. **`--ratio` is the share the TARGET pane keeps.** `pane move <p> --tab t1
   --split right --target-pane <q> --ratio 0.25` in a 184-wide area left `q` 46
   cells wide (25%) and gave the moved pane 138 (75%). So the orchestrator's
   third is `--ratio 0.3333` against the orchestrator as target, and an equal
   N-column chain places column `c` with `--ratio 1/(N-c+2)`.
6. **herdr enforces no minimum pane size.** Chained right splits produced widths
   46, 23, 11, 5, 2, 1, 0, 0, 0 without one failure. Nothing errors, so a park
   phase that splits one pane N times silently drives live agent panes to zero
   columns. The park must therefore use a 2-row grid, not a single chain.
7. **A right split subdivides only its own pane's rect.** Verified in a real
   sequence (widths/heights in a 184x53 area, orchestrator `p1` at `--ratio
   0.3333`):

   | step | move | result |
   | --- | --- | --- |
   | 1 | `p2 --split right --target-pane p1 --ratio 0.3333` | p1 61x53, p2 123x53 |
   | 2 | `p3 --split down --target-pane p2 --ratio 0.5` | p2 123x27, p3 123x26 |
   | 3 | `p4 --split right --target-pane p2 --ratio 0.5` | p2 62x27, **p4 61x27**, p3 still 123x26 |
   | 4 | `p5 --split right --target-pane p3 --ratio 0.5` | clean 2x2: p2/p4 top 62+61, p3/p5 bottom 62+61 |

   After step 3 the new column exists in the TOP ROW ONLY. Splitting `p4` DOWN at
   that point would produce two quarter-height panes - the exact bug
   `ai_docs/reports/layout-survey.md` section 6 documents.

## 5. Deviations from the brief's algorithm, forced by the above

The brief's step 3 says "column c bottom: `--split down --target-pane <top pane
of column c>`". That is correct only for column 1, whose top pane is
full-height. For column 2 and beyond it reproduces the quarter-height bug
(finding 7, step 3 then a DOWN split). The rule `tile` implements instead is the
canonical order already used by `crates/horch/src/cmd/recipes.rs:227-230`:

- column 1 top: `--split right --target-pane <orchestrator>` (or `--new-tab` on
  an overflow tab)
- column 1 bottom: `--split down --target-pane <column 1 top>`
- column c >= 2 top: `--split right --target-pane <column c-1 top>`
- column c >= 2 bottom: `--split right --target-pane <column c-1 bottom>`

That is the sequence verified in finding 7 and it lands the exact grid the brief
draws. Three smaller deviations, all forced:

- The park tab is created with `pane move <first worker> --new-tab --label
  horch-tile-scratch`, not `tab create`, because `tab create` adds a stray shell
  pane that would have to be closed (finding: section 2) and `--new-tab` adds
  none.
- No `tab close` call anywhere: emptied tabs close themselves (finding 2) and
  `tab close` would kill live agent panes (finding 3).
- The park tab holds panes in a 2-row grid rather than one right-split chain, so
  no live pane is ever driven to zero width (finding 6).
