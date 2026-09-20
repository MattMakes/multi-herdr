# Spec: `horch tile` - a deterministic pane placement manager for fleets

Status: DRAFT for operator review. Date: 2026-09-18. Author: orchestrator (Fable).
Input: `ai_docs/reports/layout-survey.md` (read-only survey of today's `horch layout`,
`horch balance`, `horch spawn` and the herdr primitives horch uses and ignores).

## 1. Problem

Today the orchestrator chooses `--from-pane` and `--direction` by hand for every
spawn, and `horch layout` infers the grid afterwards from pane rectangles. Both
halves fail in practice:

- One mis-ordered split (today: DOWN from the new top-right pane instead of RIGHT
  from the bottom-left pane) produces a quarter-height pane. `horch layout` then
  reports `offgrid` with "no suggestion", and `horch balance` refuses to act. The
  tab is stuck until a human closes a pane (survey s.6).
- Row membership is a `y == area.y` threshold, columns are a union of raw integer
  x-edges. Divider drift invents phantom columns; a quarter-height pane is filed
  as a "bottom row" member and the counts mislead (survey s.6, bullet 2).
- Everything is single-tab by construction. There is no path past 2xN on one tab,
  and no tab id is recorded anywhere (survey s.3, s.4).
- herdr already exposes what is needed and horch discards it: the split tree in
  `pane layout`, `pane split --ratio`, `pane move --tab/--new-tab`, `pane swap`,
  and the whole `herdr tab` family (survey s.3 "documented but unused").

## 2. Goals

1. The orchestrator never chooses a pane or direction. `horch spawn <teammate>`
   places the worker itself. `--from-pane`/`--direction` remain only as an
   explicit escape hatch and are logged as "manual placement".
2. Placement is deterministic and driven by the live split tree, never by
   rectangle heuristics.
3. Layout rules (the operator's):
   - Tab 1: the orchestrator is the leftmost pane, full height, never split
     vertically, never moved. To its right, workers fill a grid of at most
     2 rows x 2 columns (4 workers).
   - Tabs 2..N: no orchestrator pane. Workers fill at most 2 rows x 3 columns
     (6 workers).
   - Fill order inside any tab: column 1 top, column 1 bottom, column 2 top,
     column 2 bottom, column 3 top, column 3 bottom. A column is opened only
     when every earlier column has both rows filled.
   - A new tab is opened only when every earlier tab is full. Tabs are created
     without stealing focus.
4. When a worker's pane closes, its slot is free again and the next spawn takes
   the lowest free slot (earliest tab, leftmost column, top before bottom). A
   worker tab (tab 2+) with zero workers is closed automatically.
5. Every tab stays balanced: equal column widths, equal row heights. Tab 1
   treats the orchestrator as one column of three (so 1/3 of the width) by
   default; the share is configurable.
6. `horch layout` reports every tab, every slot, which are free, and where the
   next worker lands. If a tab does not match the canonical shape, it says so
   and offers a repair that moves panes rather than closing them.
7. All of it pure Rust in `horch`, shelling out to the `herdr` CLI like the rest
   of the crate. No bash, no jq, no socket client (README rule: horch has no
   runtime dependency beyond herdr). "Script" in the request is satisfied by a
   subcommand; a shell script would break the Windows and no-jq promises.

## 3. Non-goals

- Resizing panes to content, zooming, or focusing panes for the user.
- Preserving a worker's exact column when its neighbour closes (herdr hands the
  space to the sibling; we re-derive slots from the tree, we do not fight it).
- Layouts other than 2 rows. A 3-row option is out of scope.
- Replacing the mailbox or ledger. The tree is the source of truth for geometry;
  the mailbox still maps role -> pane id.

## 4. Model

### 4.1 Canonical tree
herdr's layout is a binary split tree (`layout.export`: `pane` / `split{direction,
ratio, first, second}`; the same rects and ratios come back from `pane layout`,
survey s.3). horch defines one canonical shape per tab kind and only ever
produces trees of that shape.

```
Tab 1 (orchestrator tab)          Worker tab (2+)
Split(right,                      Cols
  first  = Pane(orchestrator),
  second = Cols)

Cols  := Col | Split(right, first = Col, second = Cols)   -- right-leaning chain
Col   := Pane(top) | Split(down, first = Pane(top), second = Pane(bottom))
```

Invariants:
- I1. On tab 1 the root is a `right` split whose `first` is the orchestrator pane.
- I2. The chain is right-leaning: every `Cols` node's `first` is a `Col`, never
      another `Cols`.
- I3. A `Col` has depth at most 2 and its split is `down`.
- I4. Column count <= 2 on tab 1, <= 3 on worker tabs.
- I5. Columns are filled left to right; a column with one pane may only be the
      rightmost column OR a column whose bottom worker exited (herdr collapses
      the `down` split, leaving a full-height pane). Both are "bottom slot free".

Any tree that violates I1-I4 is `Unmanaged(reason)`. It is never silently
"fixed" by a spawn; `horch tile repair` fixes it explicitly (s.6.4).

### 4.2 Slot addressing
`Slot { tab_index: u8 (1-based, in workspace tab order), column: u8 (1-based,
excluding the orchestrator), row: Top | Bottom }`. Ordering is lexicographic
(tab, column, row) with Top < Bottom. This ordering IS the fill order and the
reuse order.

### 4.3 Classification (pure, tested)
`classify(tab_kind, tree, orchestrator_pane) -> Result<TabGrid, Unmanaged>`
walks the tree top-down against the grammar in 4.1 and returns:
`TabGrid { columns: Vec<Column { top: PaneId, bottom: Option<PaneId>, node: NodeRef }>, capacity }`.
No rectangles are consulted for classification. Rectangles are used only for
the balance step and for rendering.

### 4.4 Free slot and next split
`next_slot(grids: &[TabGrid]) -> Placement` returns the first of, in order over
tabs 1..N:
1. A column whose `bottom` is `None` -> `Placement::SplitDown { from: column.top }`.
2. `columns.len() < capacity` -> `Placement::SplitRight { from: last column's top pane, ratio }`
   where `ratio` makes the new column equal width after balance (s.5.3) - the
   split itself may use the equal-share ratio `k/(k+1)` for the parent, then
   balance corrects the chain.
   NOTE: opening a column produces a transient one-pane column (I5 allows it);
   the very next spawn on that tab fills its bottom by rule 1. This is the
   RaggedTop shape the old code understood; it is a legal intermediate here.
3. No tab has room -> `Placement::NewTab { after: last tab }`. The first worker
   in a new tab does not split; it takes the tab's initial pane (s.6.2).

Rule 1 before rule 2 is what makes "top and bottom, then next column" hold even
after a bottom worker exits mid-run.

## 5. Behaviour

### 5.1 `horch spawn` (changed default)
1. Read every tab in the workspace: `herdr tab list --workspace <ws>`, then
   `herdr pane layout --tab <tab>` for each (verify the exact flag against
   `herdr-docs/cli-reference.md`; if `pane layout` cannot take a tab, use
   `pane list --workspace` to find one pane per tab and `pane layout --pane`).
2. Deserialize the split tree, not just rects. Extend `horch_core::herdr::Layout`
   to keep `split` nodes (`direction`, `ratio`, `first`, `second`) alongside
   `panes`. Existing rect consumers keep working.
3. Classify each tab. Tab 1 is the tab that contains the orchestrator's pane
   (mailbox role `orchestrator`, else the leftmost full-height pane, exactly as
   today). Any other tab is a worker tab. An `Unmanaged` tab is reported and
   skipped for placement (a spawn never lands in a broken tab), unless every
   tab is unmanaged, in which case spawn fails with the repair advice.
4. `next_slot` -> execute the placement:
   - `SplitDown`/`SplitRight`: `herdr pane split <from> --direction <d> --no-focus`
     (add `--ratio` when the installed herdr supports it; s.7).
   - `NewTab`: `herdr tab create --workspace <ws> --label "workers <n>" --no-focus`
     (exact flags to be verified); read `tab_id` and the tab's initial pane id
     from the result envelope. Do not split; the initial pane is the slot.
5. Then exactly what spawn does today: ledger record, mailbox brief, `pane run`.
6. Then `balance` on the affected tab only (s.5.3), quietly.
7. Print the new pane id on stdout (unchanged contract) and, on stderr, one line:
   `placed <role> at tab <t> col <c> <top|bottom> (<pane_id>)`.
8. `--from-pane`/`--direction` given explicitly: skip 1-4, do the split as
   asked, and print `manual placement; run horch layout to check the grid`.
   `--tab <id|index>` is new: force a tab, then auto-place inside it.

### 5.2 Worker exit
`horch done` closes the worker's pane (unchanged). The detached post-close child
that today runs `balance` instead runs `tile settle`:
1. Re-read and classify all tabs.
2. If a worker tab (2+) has zero worker panes, `herdr tab close <tab>`.
   Never close tab 1.
3. Balance every tab whose column count or row count changed. Cheap enough to
   just balance all.
Nothing moves panes on exit. The freed slot is discovered by `next_slot` on the
next spawn (rule 4.4/1 or the shorter chain).

### 5.3 Balance (tree-driven, replaces edge-union balance)
Target ratios are computed from the canonical tree, not from rect edges:
- Tab 1 root (`right`, orchestrator | Cols): ratio = `orchestrator_share`,
  default `1/3` when the tab has 2 worker columns, `1/2` when it has 1, and
  `1` when it has 0 (orchestrator alone keeps the whole tab). The default rule is
  "orchestrator counts as one equal column"; `--orchestrator-share <0.2..0.6>`
  and `HORCH_ORCHESTRATOR_SHARE` override it (operator preference; see s.9 Q1).
- A right-leaning chain of N columns: node k (0-based from the left, N-1 nodes)
  gets ratio `1/(N-k)`, which yields equal widths.
- Every `Col` with a bottom pane gets ratio `0.5`.
Mechanism, in order of preference:
1. `herdr pane split --ratio` at creation time so new panes are born close to
   target (needs herdr > 0.6.1 surface; s.7).
2. `herdr pane resize --pane <p> --direction <l|r|u|d> --amount <f.4>` to move
   the node's divider from its current ratio to the target, reusing today's
   rescale simulation in `balance.rs` (the simulation stays; what changes is
   that the target and the node ownership come from the tree instead of the
   edge union). Keep `MAX_OPS` and the `changed: false` stop.
`horch balance` remains as a user-facing alias of `horch tile balance` and now
walks every tab.

### 5.4 `horch layout` (report, all tabs)
Output, one block per tab, in tab order:

```
tab 1 (w0:t1)  orchestrator w0:p1 | 2/2 columns  4/4 slots
           c1                 c2
  top      researcher-1 (p2)  researcher-3 (p4)
  bottom   researcher-2 (p3)  researcher-4 (p5)
tab 2 (w0:t2)  workers  1/3 columns  1/6 slots
           c1
  top      opus-1 (p7)
  bottom   -
next: tab 2 col 1 bottom  (split DOWN from w0:p7)
```
Unmanaged tabs print `tab 3 (w0:t3)  UNMANAGED: column 2 has depth 3 (p4, p5 over p3)`
followed by the repair plan (5.5). `--json` emits the `TabGrid`s, the next
placement, and the unmanaged reasons, for tests and for the orchestrator to read
cheaply.

### 5.5 `horch tile repair`
For an `Unmanaged` tab, compute the smallest sequence of `herdr pane move`
(same tab, `--split right|down --target-pane <p>`) that reaches a canonical tree
with the same set of panes, filling slots in 4.2 order. Panes are never closed
and processes are never restarted (`pane move` and `pane swap` preserve PTYs,
survey s.3). Print the plan; apply it only with `--apply`. Overflow (more panes
than capacity) moves the extras to the next tab with room, or a new tab, with
`pane move --tab`/`--new-tab`. The quarter-height case from today is the first
test fixture: expected repair = `pane move p5 --split right --target-pane p3`.

### 5.6 The orchestrator's briefing
Update the fleet orchestrator prompt (`teammates/orchestrator.md`,
`teammates/orchestrator-codex.md`, the `horch layout` section of the herdr-
orchestrator skill) to: "`horch spawn` places panes for you. Do not pass
`--from-pane` or `--direction`. Run `horch layout` if you want to see the grid."
This removes the manual-placement paragraph from the orchestrator's context.

## 6. herdr surface required (verify each before implementing)
From `herdr-docs/cli-reference.md` (line refs per survey s.3):
- `herdr tab list|create|get|focus|close` (:144-149). Need: create with
  `--workspace`, `--label`, `--no-focus`; result must expose `tab_id` and the
  initial `pane_id`. If the result does not name the initial pane, find it via
  `pane list --workspace` filtered by `tab_id`.
- `herdr pane layout` returning split nodes with `ratio` (socket-api.md:162).
  Confirm the CLI JSON includes them, not only the socket method.
- `herdr pane split --ratio` (:170) and `herdr pane move ... --tab/--new-tab/
  --split/--target-pane/--ratio` (:173-175), `pane swap` (:171-172).
- `herdr tab close` behaviour when the tab has panes (must refuse or must be
  called only on an empty tab; we only call it on empty tabs).
- `$HERDR_TAB_ID` (:444) for `--tab current`.
The wrapper `crates/horch-core/src/herdr.rs` is pinned to herdr 0.6.1 on
purpose (survey s.3). This spec raises the floor: `horch doctor` must report the
installed herdr version and whether `tab create`, `pane split --ratio`, and
`pane move` are available; `tile` degrades as follows when they are not:
- no `--ratio`: place with plain split, then balance by resize (works today).
- no `tab create`: placement stops at tab 1 capacity and spawn fails with
  "tab 1 full (4 workers) and this herdr cannot create tabs; upgrade herdr".
- no `pane move`: `tile repair` prints the plan and says it cannot apply it.

## 7. Files (for the implementation plan, not exhaustive)
- `crates/horch-core/src/tile.rs` NEW: tree types, `classify`, `next_slot`,
  `balance_plan`, `repair_plan`. Pure, no I/O, unit-tested.
- `crates/horch-core/src/herdr.rs`: deserialize split nodes; add `tab_list`,
  `tab_create`, `tab_close`, `pane_move`, `pane_swap`, optional `--ratio` on
  `pane_split`; version probe.
- `crates/horch-core/src/layout.rs` and `balance.rs`: retire the edge-union
  analysis in favour of `tile::classify`; keep the rescale simulation and the
  renderer helpers that still apply; keep the old tests that describe
  behaviour that survives, delete the ones that describe the heuristic.
- `crates/horch/src/cmd/spawn.rs`: call `tile::place` unless manual flags given;
  add `--tab`.
- `crates/horch/src/cmd/tilecmd.rs` NEW: `horch tile {status|balance|repair|settle}`;
  `horch layout` and `horch balance` become thin aliases.
- `crates/horch/src/cmd/balancecmd.rs`: the detached post-close child calls
  `tile settle`.
- `crates/horch/src/cmd/doctor.rs`: herdr capability report.
- `teammates/orchestrator*.md`, `README.md` (Layout section), the
  herdr-orchestrator skill's layout paragraph.

## 8. Tests
Pure tests in `tile.rs` over synthetic trees (builders: `pane(id)`,
`right(a,b,ratio)`, `down(a,b,ratio)`):
1. Spawn sequence 1..11 from an empty tab 1: assert the slot and the exact
   placement (from-pane, direction) at each step. Expected:
   1 RIGHT from orch -> t1c1T; 2 DOWN from c1T -> t1c1B; 3 RIGHT from c1T -> t1c2T;
   4 DOWN from c2T -> t1c2B (rule 4.4/1, fill the open column's bottom, precedes
   opening a column); 5 NewTab t2c1T; 6 D t2c1B; 7 R t2c2T; 8 D t2c2B; 9 R t2c3T; 10 D t2c3B; 11 NewTab t3c1T.
   (Note the difference from the old recipe order `right(top_left) -> right(bottom_left)`:
   the tree-driven rule fills a column by splitting its own top pane DOWN, which
   never touches the neighbouring column and cannot produce the quarter-height bug.)
2. Close then spawn: with t1 full, close t1c1B -> herdr collapses c1 to one pane
   -> next spawn is D from that pane into t1c1B. Close t1c2T and t1c2B -> chain
   shortens -> next spawn is R into t1c2T. Close all of t2 -> `settle` closes t2.
3. Capacity: tab 1 never exceeds 2 columns; worker tabs never exceed 3.
4. Unmanaged fixtures: today's quarter-height shape (survey s.6 table) ->
   `Unmanaged("column 2 depth 3")` with repair = move p5 right of p3; a
   left-leaning chain; a `right` split inside a `Col`; orchestrator not `first`
   at the root. Each yields a plan that re-classifies as canonical.
5. Balance: for N = 1..3 columns, ratios `1/(N-k)` produce equal widths within
   1 cell under the existing rescale simulation; orchestrator share 1/3 with 2
   columns yields three equal thirds.
6. Rendering snapshot for a two-tab fleet, and `--json` round-trip.
Integration: extend `horch smoke fleet` to spawn 5 fake (`smoke`) workers, assert
a second tab appears, close two, assert the slot reuse, close all, assert tab 2
is gone. Spends no tokens.

## 9. Open questions for the operator
Q1. Orchestrator width on tab 1: equal third (default here), or a fixed
    minimum column count (e.g. never narrower than 60 cells)? A fixed minimum
    keeps the orchestrator readable on narrow terminals; equal thirds keeps
    workers readable.
Q2. Should a new worker tab take focus when created? Spec says no (the operator
    is watching the orchestrator). A `--focus` flag can be added later.
Q3. Slot reuse vs append: spec fills the lowest free slot (keeps tab 1 dense).
    The alternative, always appending at the end, keeps a worker's position
    stable for the whole run but leaves holes next to the orchestrator.
Q4. Is a hard cap on tabs wanted (e.g. refuse the 17th worker), or unlimited?
Q5. Confirm "no shell script": this spec makes it a `horch` subcommand to keep
    the no-bash / no-jq / Windows promises in the README.

## 10. Rollout
1. Implement `tile.rs` pure logic and tests (no behaviour change yet).
2. Extend the herdr wrapper and `doctor`.
3. Switch `horch spawn` to auto-placement behind `HORCH_TILE=1`, run it for a
   fleet, then flip the default and keep `HORCH_TILE=0` as the fallback for one
   release.
4. Replace `layout`/`balance` internals; update orchestrator briefs and README.
5. Extend `smoke fleet`.
Each step is one worker task; the implementation plan will name them.
