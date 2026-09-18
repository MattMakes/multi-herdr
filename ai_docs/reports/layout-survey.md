# Worker pane layout survey (read-only)

Survey of how `horch` lays out worker panes today, as input to a spec for a new
tiling manager. All paths relative to `/Users/mascott/projects/multi-herdr`.

## 1. `horch layout` and `horch balance`

### Files

| Piece | File |
| --- | --- |
| CLI surface (`Layout`, `Balance` subcommands) | `crates/horch/src/main.rs:133-150` |
| `horch layout` driver | `crates/horch/src/cmd/layoutcmd.rs:15-42` |
| Grid analysis + renderer (pure) | `crates/horch-core/src/layout.rs` |
| `horch balance` driver + `equalize*` helpers | `crates/horch/src/cmd/balancecmd.rs` |
| Resize planner (pure) | `crates/horch-core/src/balance.rs` |
| Typed `herdr` CLI wrapper | `crates/horch-core/src/herdr.rs` |

### Target shape

The whole design targets **one tab, 2 rows tall by N columns wide**, with the
orchestrator as a full-height column on the far left
(`crates/horch-core/src/layout.rs:1-16`).

### The algorithm (`layout::analyze`, layout.rs:126-267)

1. Flatten `Layout.panes` into `Slot { id, x, y, w, h }` (layout.rs:129-139).
   Only rects are used — the split tree is never consulted.
2. `is_full(s) = s.h >= area.height - 1` — a 1-cell tolerance for border rows
   (layout.rs:143).
3. **Orchestrator exclusion** (layout.rs:145-152): use the pane id the mailbox
   registered for role `orchestrator` if non-empty; otherwise infer it as the
   **leftmost full-height pane** (`full.sort_by_key(|s| s.x).first()`). It is
   then filtered out of `workers` (layout.rs:154-157) and never appears in a
   cell, never gets resized, and is only mentioned in the header line
   (layout.rs:288-297). `horch layout` resolves the registered id at
   `crates/horch/src/cmd/layoutcmd.rs:32-38`; `horch balance` does the same at
   `crates/horch/src/cmd/balancecmd.rs:24-30`.
4. Split workers into `full` (full-height) and `half`; split `half` into
   `top` (`s.y == area.y`) and `bottom` (`s.y > area.y`) — layout.rs:158-161.
   **Note: this is a threshold test, not row clustering.** Any non-full pane
   whose top edge is below the area top counts as "bottom", whatever its height.
5. **Column detection is purely x-based** (layout.rs:164-170): collect every
   `x` and `x + w` edge from *both* rows, sort, dedup. Consecutive edge pairs
   are the column slices. For each slice `[x0,x1]`, `top`/`bottom` are the first
   pane in that row that `covers(x0,x1)` (`s.x <= x0 && s.x+s.w >= x1`,
   layout.rs:33-35), and `depth` = how many *half* panes cover the slice
   (layout.rs:172-184).
6. **offgrid** = `cells.iter().any(|c| c.depth > 2)` (layout.rs:186) — i.e. some
   x-slice has 3+ non-full panes stacked over it, so the tab is not a 2-row grid.
   Depth counts panes rather than comparing heights deliberately, so uneven split
   ratios do not read as a third row (layout.rs:43-48, test
   `uneven_split_ratios_still_read_as_two_rows`, layout.rs:513-524).
7. **ragged** detection (layout.rs:190-199): a row is ragged where one of its
   panes *strictly contains* an edge produced by the union set
   (`edges.any(|e| e > p.x && e < p.x + p.w)`); the leftmost such pane is the one
   to split RIGHT.
8. State precedence (layout.rs:205-219):
   `Offgrid` > `Empty` (no workers) > `Unpaired` (any full-height worker) >
   `RaggedBottom` > `RaggedTop` > `Complete` (`top.len() == bottom.len()`) >
   `Ragged`.
9. **Next split** (layout.rs:221-253):
   - `Unpaired` -> **DOWN** from the rightmost full-height worker.
   - `RaggedBottom` -> **RIGHT** from the spanning bottom pane.
   - `RaggedTop` -> **RIGHT** from the spanning top pane.
   - `Complete` -> **RIGHT** from the top-rightmost pane, with a `followup` of
     **RIGHT** from the bottom-rightmost (a new column always needs two splits).
   - `Empty` -> **RIGHT** off the orchestrator.
   - `Offgrid` / `Ragged` -> **no suggestion** (`next = None`), rendered as
     "no suggestion for this shape." (layout.rs:343-346).
10. `columns = cells.len() + full.len()` (layout.rs:260); `rows_top`/`rows_bottom`
    are the raw `top`/`bottom` counts.
11. `render` (layout.rs:270-370) labels panes with mailbox roles, stars panes that
    span >1 cell, prints the `grid: N column(s), top row T, bottom row B -> state`
    line and the ready-to-paste `horch spawn ... --from-pane X --direction Y`.

### `horch balance` (balancecmd.rs + balance.rs)

- Purpose is stated at `crates/horch-core/src/balance.rs:1-34` and
  `crates/horch/src/cmd/balancecmd.rs:1-9`: because `analyze` unions *exact
  integer edges*, drifted dividers manufacture phantom columns -> phantom
  `ragged` -> a suggested split that adds a 3rd pane to a column -> `offgrid`
  with no advice at all. Balancing is a precondition of the advisor, not cosmetics.
- `worker_rows` (balance.rs:85-154) re-derives the orchestrator with the *same*
  rule as `analyze`, then bails (returns `None`) if: no workers; any full-height
  worker; distinct `y` values != 2; either row has < 2 panes; rows have different
  pane counts; rows have different x-extents.
- `plan` (balance.rs:172-236) targets absolute positions
  `x0 + round(k * w / n)` per row, models the row as a **right-leaning binary
  chain** (divider `k` is owned by a split spanning `[pos[k-1], x1]`, and
  `--amount` is a fraction of that span), and simulates herdr's proportional
  rescale so later ops are planned against post-op geometry. `TOLERANCE = 1` cell
  (balance.rs:40).
- `equalize` (balancecmd.rs:39-61) re-plans against the layout herdr returns after
  each resize, capped at `MAX_OPS = 24`, stopping on `changed: false` (min pane
  width) or a repeated `(pane, from_x)`.
- Called automatically after every spawn (`spawn.rs:202-204`,
  `equalize_quietly`) and, via a detached `setsid` child, after a worker closes
  its own pane (`balancecmd.rs:82-108` + `messaging.rs:114`).

## 2. `horch spawn`

- CLI: `crates/horch/src/main.rs:113-132`. `--from-pane` is `Option<String>` with
  **no default**; `--direction` is `#[arg(long, default_value = "right")]`
  parsed into `horch_core::herdr::Direction` (only `right` | `down`,
  herdr.rs:122-148; unknown values rejected — test main.rs:452).
- `--from-pane` default resolution (`crates/horch/src/cmd/spawn.rs:180-189`):
  fall back to `$HERDR_PANE_ID` (herdr's *internal* id, e.g. `p_2`), round-tripped
  through `herdr pane get` to get the public id (`w0:p1`). Errors with
  "horch spawn needs --from-pane when not run inside a herdr pane" otherwise.
- Pane creation: **`herdr pane split`**, nothing else. `spawn.rs:191` calls
  `Herdr::pane_split` which shells out to
  `herdr pane split <from> --direction <right|down> --no-focus`
  (`crates/horch-core/src/herdr.rs:219-229`). **No tab is ever created.**
- New pane id parsing: `herdr.rs:220-228` deserializes the
  `{"result": {"pane": {"pane_id": ...}}}` envelope (`Envelope<PaneResult>`,
  herdr.rs:75-88 + the generic `json()` at herdr.rs:190-196) and returns
  `r.pane.pane_id`. `spawn()` returns that id, and `main.rs` prints it on stdout
  so callers can chain splits (spawn.rs:1-7, :222).
- After splitting: `herdr pane run <new_pane> "<horch worker <role>>"`
  (spawn.rs:192-194 via `PaneShell::host().command_line`), then
  `pane_layout(new_pane)` -> `equalize_quietly` (spawn.rs:202-204).
- Ordering contract: ledger record first, mailbox brief second, pane last
  (spawn.rs:145-178), so the worker can assume both exist.

## 3. herdr primitives actually used

All herdr access goes through `Command::new("herdr")` in
`crates/horch-core/src/herdr.rs:168` (plus direct `Command::new("herdr")` at
:200, :326, :336). **There is no HTTP or socket client anywhere** — grep for
`UnixStream|reqwest|hyper|jsonrpc` only hits `prime.rs`, which is the unrelated
Prime Agent daemon socket.

| Wrapper | herdr CLI invoked | Defined | Call sites |
| --- | --- | --- | --- |
| `server_reachable` | `workspace list` | herdr.rs:199-205 | doctor |
| `pane_get` | `pane get <id>` | herdr.rs:209-212 | 7 sites (id upgrade) |
| `pane_list` | `pane list --workspace <ws>` | herdr.rs:214-217 | layoutcmd.rs:22, balancecmd.rs:126 |
| `pane_split` | `pane split <from> --direction <d> --no-focus` | herdr.rs:219-229 | spawn.rs:191; recipes.rs:227-230; smoke.rs:71 |
| `pane_run` | `pane run <pane> <cmd>` | herdr.rs:234-237 | spawn.rs:194, recipes.rs, smoke.rs |
| `pane_send_text` / `pane_send_keys` | `pane send-text` / `pane send-keys` | herdr.rs:239-247 | `send_line` (herdr.rs:351-359) |
| `pane_close` | `pane close <pane>` | herdr.rs:249-252 | messaging.rs:116 (`horch done`) |
| `pane_exists` | `pane get` | herdr.rs:255-257 | |
| `pane_read` | `pane read <pane> --source <s>` | herdr.rs:259-261 | codex/session harvest |
| `pane_layout` | `pane layout --pane <p>` or `--current` | herdr.rs:263-269 | layoutcmd.rs:29, balancecmd.rs:133, spawn.rs:202 |
| `pane_resize` | `pane resize --pane <p> --direction <l/r> --amount <f.4>` | herdr.rs:279-294 | balancecmd.rs:53 (only) |
| `workspace_create` | `workspace create --label L [--cwd] [--no-focus]` | herdr.rs:296-315 | recipes.rs:177,224; smoke.rs:69,138 |
| `workspace_close` | `workspace close <ws>` | herdr.rs:317-320 | smoke teardown |
| `wait_output` | `wait output <pane> --match --timeout` | herdr.rs:324-332 | smoke |
| `integration_status` | `integration status` | herdr.rs:335-342 | recipes.rs |

**Tab creation is not used anywhere today.** There is no `tab` subcommand in the
wrapper, no `pane move --new-tab`, and `WorkspaceCreateResult`
(herdr.rs:109-113) deliberately drops the `.result.tab.tab_id` that the docs say
is present (herdr-docs/cli-reference.md:126). `Layout.tab_id` is deserialized
(herdr.rs:64) and printed in the header (layout.rs:283-286) but never persisted.
`HERDR_TAB_ID` (cli-reference.md:444) is never read.

Also note herdr.rs:4-6: the wrapper is deliberately pinned to the herdr 0.6.1
surface — **no `pane current`, and no `--env` or `--ratio` on `pane split`**.
That is why every split halves its parent and why `horch balance` has to exist.

### What herdr-docs documents but horch does not use

- Tabs: `herdr tab list|create|get|focus|rename|close`
  (herdr-docs/cli-reference.md:144-149); semantics at :152; focus defaults at :154.
- `herdr pane move <id> --tab <tab_id> --split right|down [--target-pane] [--ratio]`,
  `--new-tab`, `--new-workspace` (cli-reference.md:173-175, socket-api.md:230).
- `herdr pane split ... --ratio FLOAT --cwd --env --focus/--no-focus`
  (cli-reference.md:170) — `--ratio` would remove most of the need to balance.
- `herdr pane swap --direction ...` / `--source-pane/--target-pane`
  (cli-reference.md:171-172, socket-api.md:220 — same-tab, preserves split shape,
  ratios, pane ids and running processes).
- `herdr pane neighbor`, `pane edges`, `pane focus --direction`, `pane zoom`,
  `pane rename` (cli-reference.md:163-168); `pane.layout` also returns
  `zoomed`, `focused_pane_id` and **split rects/ratios** (socket-api.md:162)
  that the current `Layout` struct discards.
- No native balance or equalize command exists in herdr at all: grepping the docs
  mirror for `balance` returns nothing. The only primitives are
  `herdr pane resize --amount` (cli-reference.md:167) and the socket-only
  `layout.set_split_ratio` (socket-api.md:101).
- Socket-only layout methods: `layout.export` (full BSP tree of `pane`/`split`
  nodes with `direction`, `ratio`, `first`, `second`), `layout.apply`
  (declarative tab from a tree; recreates the tab, does **not** preserve live
  PTYs or running processes), `layout.set_split_ratio`
  (socket-api.md:101, :166-176). These have no CLI equivalent in the docs mirror.

## 4. Ledger / mailbox: where pane ids and roles live

- **Mailbox** (`crates/horch-core/src/mailbox.rs`) is the role -> pane-id
  registry. Directory: `$TMPDIR/herdr-orchestration-<workspace_id>`
  (mailbox.rs:66-87). Files per role: `<role>.id` (the *public* pane id, one
  line) and `<role>.brief.json`.
  - `Mailbox::register` (mailbox.rs:135-156) resolves `$HERDR_PANE_ID` ->
    `pane get` -> public id and writes `<role>.id`.
  - `pane_for(role)` (:159), `roles()` (:166), `panes_to_roles()` (:187 — used
    for layout labelling and orchestrator lookup), `role_taken` (:207),
    `unregister` (:212), `next_seq` (:217, the `<teammate>-<n>` counter).
  - **No tab id is recorded**, and the `Brief` struct (mailbox.rs:15-58) has no
    `pane_id` or `tab_id` field at all — only role/teammate/agent/model/
    record_id/session_id/task/dirs.
- **Ledger** (`crates/horch-core/src/ledger.rs`) is the durable per-project
  session store. `Record` (ledger.rs:35-54) holds `record_id`, `session_id`,
  `agent`, `tier`, `model`, `phase`, `role`, `status`, `task`, `history`,
  timestamps. **No pane id and no tab id.** The only link between a ledger record
  and a live pane is `role`, resolved through the mailbox.

Consequence for a tiling spec: the system today has *no persistent record of
geometry*. Everything is re-derived from `herdr pane layout` on demand, and the
only identity carried across is `role -> pane_id` in a temp dir.

## 5. Existing layout tests

`crates/horch-core/src/layout.rs:372-572` (`mod tests`), all built from
`(id, x, y, w, h)` tuples in a 200x50 tab via the `layout()` helper (layout.rs:378-393):

| Test | Shape covered |
| --- | --- |
| `empty_grid_suggests_splitting_off_the_orchestrator` :395 | orchestrator alone -> `Empty`, RIGHT off orch |
| `a_full_height_worker_is_unpaired` :408 | orch + 1 full-height worker -> `Unpaired`, DOWN |
| `a_2x2_grid_is_complete_and_needs_two_splits_for_a_new_column` :420 | canonical 2x2 -> `Complete`, RIGHT from `tr` + followup `br` |
| `bottom_row_spanning_an_extra_boundary_is_ragged_bottom` :442 | 2 top / 1 wide bottom -> `RaggedBottom` |
| `top_row_spanning_an_extra_boundary_is_ragged_top` :457 | 1 wide top / 2 bottom -> `RaggedTop` |
| `three_stacked_panes_are_offgrid_with_no_suggestion` :471 | 3 equal panes stacked full-width -> `Offgrid`, `next == None` |
| `orchestrator_is_inferred_as_the_leftmost_full_height_pane` :487 | no registered orch |
| `registered_orchestrator_overrides_the_positional_guess` :501 | registered orch not leftmost |
| `uneven_split_ratios_still_read_as_two_rows` :513 | 40/10 vertical split -> still `Complete`, depth 2 |
| `render_labels_panes_with_their_roles_and_marks_spanning_cells` :526 | role labels, `*` span marker |
| `render_shows_both_splits_for_a_complete_grid` :552 | two-split followup text |
| `render_names_a_down_split_vertical` :567 | VERTICAL/HORIZONTAL wording |

Balance tests: `crates/horch-core/src/balance.rs:236-481` — helpers `grid()`,
`apply()` (simulates herdr's proportional rescale), `assert_balanced()`,
`edges()`, `run()`; cases: `fresh_three_column_grid_needs_one_op_per_row` :372,
`balancing_lands_both_rows_on_identical_integers` :385, `repairs_a_drifted_row`
:392, `four_columns_converge` :400, `plan_is_idempotent` :412,
`interleaved_passes_converge` :424, `already_even_grid_plans_nothing` :438,
`ragged_grid_is_left_alone` :444, `unpaired_full_height_worker_is_left_alone`
:452, `single_column_grid_plans_nothing` :467,
`orchestrator_column_is_never_moved` :473.

**Gap:** no test covers a *quarter-height* pane, i.e. the exact runtime shape in
section 6. Every offgrid test uses three equal full-width rows.

## 6. Why the observed runtime shape is `offgrid`

Observed sequence from orchestrator `w0:p1`:

```
right from p1 -> p2      p2 = full-height worker column
down  from p2 -> p3      p2 = top half,  p3 = bottom half (both full worker width)
right from p2 -> p4      p2 and p4 split the TOP HALF only
down  from p4 -> p5      p4 and p5 split p4's half -> two QUARTER-height panes
```

Resulting geometry (worker area `x in [X, W]`, area height `H`, `y0 = area.y`):

| pane | x range | y | h |
| --- | --- | --- | --- |
| p2 | `[X, M]` | `y0` | `H/2` |
| p4 | `[M, W]` | `y0` | `H/4` |
| p5 | `[M, W]` | `y0 + H/4` | `H/4` |
| p3 | `[X, W]` | `y0 + H/2` | `H/2` |

Now trace `analyze`:

- No pane is full-height except the orchestrator, so `full` is empty and
  `half = {p2, p3, p4, p5}` (layout.rs:158-159).
- Row split is the **threshold test** at layout.rs:160-161:
  `top = {p2, p4}` (both have `y == area.y`), `bottom = {p3, p5}` (both have
  `y > area.y`). p5 is a quarter-height pane sitting in the *top* half, but it is
  silently filed under "bottom row".
- Edges (layout.rs:164-170) are `{X, M, W}` -> two cells: `[X, M]` and `[M, W]`.
- `depth` for cell `[M, W]` counts every *half* pane that covers that x-slice
  (layout.rs:181): **p4, p5 and p3** (p3 spans `[X, W]`, so it covers `[M, W]`
  too) = **3**.
- `offgrid = cells.any(|c| c.depth > 2)` (layout.rs:186) -> **true**, and
  `Offgrid` wins the state precedence outright (layout.rs:205-206), so
  `next = None` (layout.rs:252) and the renderer prints
  "no suggestion for this shape."
- The printed numbers follow exactly: `columns = cells.len() (2) + full.len() (0)
  = 2`, `rows_top = |{p2,p4}| = 2`, `rows_bottom = |{p3,p5}| = 2` — i.e.
  `grid: 2 column(s), top row 2, bottom row 2 -> offgrid`.

**Two distinct things are going on, and a spec should separate them:**

1. *The judgement is factually correct.* Step 4 was a real mistake. Column 2 of
   that tab genuinely has three panes stacked over it (p4, p5, p3), because
   step 3 (`right from p2`) subdivided only the **top half**, leaving p3 as a
   full-width bottom pane. The canonical order is the one in
   `crates/horch/src/cmd/recipes.rs:227-230`:
   `right(root) -> down(top_left) -> right(top_left) -> right(bottom_left)` —
   the fourth split must go **RIGHT from the bottom-left pane**, not DOWN from
   the new top-right pane. `horch layout` run after step 3 would have reported
   `RaggedBottom` and advised exactly that: `RIGHT from p3`. This is the
   `Complete` -> `followup` rule (layout.rs:243-249) and the advice string
   "then, to finish the new column (2 splits per column)" (layout.rs:363-368).
2. *But the analysis is also structurally fragile*, and this is what the new
   tiling manager should fix:
   - Row membership is `y == area.y` vs `y > area.y` (layout.rs:160-161), not
     real clustering. A quarter-height pane is counted as a whole "bottom row"
     member, so `rows_top 2 / rows_bottom 2` reads like a healthy 2x2 while the
     tab is in fact a 3-deep column. The counts the operator sees actively
     mislead.
   - Columns come from a **union of raw integer x edges** across both rows
     (layout.rs:164-170), so a one-cell divider drift invents phantom columns —
     the whole reason `horch balance` exists (balance.rs:1-34, README.md:98-103).
   - `Offgrid` and `Ragged` produce **no recovery advice at all**
     (layout.rs:252, :343-346); the only documented remedy is "close the extra
     pane(s) or start a fresh workspace" (layout.rs:86-92).
   - `horch balance` refuses to act on this shape too: `worker_rows` sees three
     distinct `y` values and returns `None` (balance.rs:117-122), so
     `horch balance` prints "not a settled 2-row grid"
     (balancecmd.rs:139-145). The tab is stuck until a human closes a pane.
   - herdr already hands over everything needed to do this properly and horch
     throws it away: `pane.layout` returns split rects/ratios
     (socket-api.md:162) that `Layout` (herdr.rs:61-68) does not deserialize,
     and `layout.export`/`layout.apply` expose a real BSP tree
     (socket-api.md:166-176). `balance.rs:7` says it outright: "It never reads
     the split tree."

### Additional constraints a tiling spec must respect

- `horch layout --workspace X` analyses only the tab containing
  `pane_list(ws)[0]` (layoutcmd.rs:20-27, same in balancecmd.rs:124-131), so the
  tool is **single-tab by construction** today.
- `herdr pane close` takes down the calling pane's whole process tree, which is
  why post-close rebalancing has to be a detached `setsid` child
  (balancecmd.rs:73-108).
- A departing worker hands its width to its split sibling, lumping the grid
  (messaging.rs:107-114).
- `pane_resize` returns `changed: false` when herdr refuses a move — that is how
  the minimum pane width announces itself rather than being hard-coded
  (herdr.rs:96-102).
- herdr stores split ratios to four decimal places; `pane_resize` formats
  `--amount` to `{:.4}` to stay in step (herdr.rs:280-282).
