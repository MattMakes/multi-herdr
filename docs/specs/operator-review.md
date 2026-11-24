# Spec: operator review tool (`horch review`)

## Problem Statement

When the fleet turns an operator's rough idea into a spec (`docs/specs/<slug>.md`) or a ticket map (`docs/tickets/<slug>/MAP.md`), the orchestrator stops at the review gate before moving to the next step (`docs/specs/task-ledger.md` section 5.5b). Today, the operator can review the file only by reading it in an editor or terminal and typing prose into the orchestrator chat pane, after which the orchestrator writes the notes file by hand.

Chat prose has no direct line anchors. When the operator wants to mark five separate passages in a 200-line spec—requesting a change on lines 12–14, asking a question on line 40, and deleting lines 58–63—typing line numbers and quoting text by hand in chat is slow and error-prone. Third-party annotation tools, browser apps, and external npm or binary packages are forbidden on the operator machine (`docs/specs/external-imports.md`, D1 and D2). Furthermore, running an interactive review inside a worker pane would stall the worker until the watchdog reaps it after 3 checks of 10 minutes.

The operator needs a native terminal tool inside the `horch` Rust binary that runs in an operator-opened herdr pane, displays a spec or ticket map with line numbers, lets the operator select line ranges and attach typed notes (`change`, `question`, `delete`), writes the exact notes file contract of `docs/specs/task-ledger.md` section 5.5b (`ai_docs/plans/<slug>/review-<target>-<n>.md`), and notifies the orchestrator with 1 mail line naming the written notes file path.

## Solution

Add a subcommand `horch review <file> [--slug <slug>] [--target spec|map]` to the `horch` binary in `crates/horch`, backed by pure state-machine and formatting logic in `crates/horch-core` and rendered in the terminal with `ratatui` and `crossterm` (already workspace dependencies of `crates/horch`). The tool uses no browser, no network, no new background service, and no new external crate dependency.

The workflow from the operator's perspective:

1. **Open**: At the spec gate or the ticket-map gate, the operator runs `horch review docs/specs/<slug>.md` or `horch review docs/tickets/<slug>/MAP.md` in a herdr pane (or the orchestrator prints the exact command for the operator to run).
2. **Resolve target and round**: `horch review` resolves `<target>` (`spec` or `map`) and `<slug>` from the flags or by inferring them from the file path (`docs/specs/<slug>.md` -> target `spec`, slug `<slug>`; `docs/tickets/<slug>/MAP.md` or `docs/tickets/<slug>/<file>.md` -> target `map`, slug `<slug>`). It computes the git blob SHA-1 of the reviewed file content and picks the next review round `<n>` (from `1`) by scanning `ai_docs/plans/<slug>/review-<target>-<n>.md` for the lowest unused positive integer.
3. **Read and annotate**: The terminal UI shows the file content with 1-based line numbers, a cursor line, an optional visual selection anchor for multi-line ranges, and a side or bottom notes pane listing all current notes `N1`, `N2`, ... in line order. The operator moves through the file, selects a single line or a line range (`L<start>-L<end>`), adds a note with a kind (`change`, `question`, or `delete`) and free-text body, edits or deletes existing notes, and inspects the notes list.
4. **Finish**: The operator chooses one of the three gate decisions—`approved`, `annotated`, or `dismissed`:
   - `horch review` writes the notes file to `ai_docs/plans/<slug>/review-<target>-<n>.md` (creating parent directories if needed) using the exact contract defined in `docs/specs/task-ledger.md` section 5.5b (ticket 01 of `docs/tickets/external-imports/`).
   - `horch review` sends 1 durable mail line to `orchestrator` in the current workspace with the result and the notes file path:
     `[operator] NOTE: Review <target> round <n> is <result>: ai_docs/plans/<slug>/review-<target>-<n>.md`
   - The terminal UI restores the terminal screen and exits `0`.

## User Stories

1. As an operator, I want to run `horch review <file>` in a herdr pane with no browser, no network connection, and no background server, so that I can review specs and ticket maps while respecting the zero-third-party-tools rule (D1, D2).
2. As an operator, I want `horch review docs/specs/retry-limit.md` to infer `--target spec` and `--slug retry-limit` automatically when I omit those flags, so that I do not have to type redundant arguments for standard spec paths.
3. As an operator, I want `horch review docs/tickets/retry-limit/MAP.md` to infer `--target map` and `--slug retry-limit` automatically when I omit those flags, so that standard ticket map paths work with a single file argument.
4. As an operator, I want to override the inferred slug and target with `--slug <slug>` and `--target spec|map`, so that I can review a file at a non-standard path or force a specific plan folder.
5. As an operator, I want `horch review` to fail immediately with a clear error message if `--slug` or `--target` cannot be inferred from `<file>` and was not passed on the command line, so that notes never land in an ambiguous path.
6. As an operator, I want `horch review` to determine the review round `<n>` starting at `1` by checking existing `ai_docs/plans/<slug>/review-<target>-<n>.md` files and choosing the next integer, so that round 1, round 2, and later revisions never overwrite earlier review rounds.
7. As an operator, I want the header of the TUI to show the reviewed file path, the inferred or explicit `<slug>`, `<target>`, round `<n>`, and the current count of notes, so that I always know which gate and round I am recording.
8. As an operator, I want to see the reviewed file rendered with 1-based line numbers (`1`, `2`, ...) and a highlighted cursor line, so that I can read the spec or ticket map line by line.
9. As an operator, I want to move the cursor line up and down by 1 line (`k`/`Up`, `j`/`Down`), by 1 page (`PgUp`/`Ctrl-U`, `PgDn`/`Ctrl-D`), or to the first and last line (`Home`/`g`, `End`/`G`), so that I can navigate short and long specs quickly.
10. As an operator, I want to start a line-range selection at the cursor line (with `v` or `Space`) and extend or shrink it by moving the cursor line, so that I can target a multi-line passage `L<start>-L<end>` where `<start> <= <end>`.
11. As an operator, I want to cancel an active line-range selection with `Esc`, returning to a single-line cursor selection, so that I can recover from a mis-anchored selection without adding a note.
12. As an operator, I want to add a note on the current line or selected line range by pressing a key for the note kind (`c` for `change`, `q` for `question`, `d` for `delete`, or `a` / `Enter` to open the note editor and pick or cycle the kind), so that I can record structured feedback directly on the lines it applies to.
13. As an operator, I want the note editor to show the target line range `L<start>-L<end>`, the note kind (`change`, `question`, `delete`), and the quoted start of line `<start>` (at most 80 characters), so that I can see the exact anchor while typing my note text.
14. As an operator, I want to switch the note kind between `change`, `question`, and `delete` inside the note editor (for example with `Tab` or `BackTab`), so that I can fix a note's kind without discarding what I typed.
15. As an operator, I want to save a note in the note editor (for example with `Enter`) and have the editor reject an empty or whitespace-only note body, so that every recorded note `N<k>` carries operator instructions.
16. As an operator, I want to cancel adding or editing a note with `Esc` before saving, leaving the existing notes list unchanged, so that I can back out of a draft note cleanly.
17. As an operator, I want lines that already have one or more notes attached to display a gutter marker (such as `N1`, `N2` or a kind badge) in the file view, so that I can see at a glance which passages I have already annotated.
18. As an operator, I want a notes list pane that shows every note numbered `N1`, `N2`, ... with its kind, line range `L<start>-L<end>`, quoted first line start, and operator text, so that I can review all my feedback before finishing.
19. As an operator, I want notes to be numbered `N1`, `N2`, ... deterministically in order of `(start_line, end_line, insertion_order)` and renumbered automatically when a note is inserted or deleted, so that the output file always has contiguous `N1..N<m>` identifiers ordered from top to bottom of the file.
20. As an operator, I want to toggle focus between the file view and the notes list (or jump between notes with `n` / `p` or `Tab`), so that I can select any existing note and jump the file view to its line range.
21. As an operator, I want to edit the selected note's kind and text (with `e` or `Enter` on the note), so that I can refine my feedback before submitting the review.
22. As an operator, I want to delete the selected note (with `x` or `Delete`), so that I can remove a note I no longer want to send.
23. As an operator, I want to finish the review with `approved`, `annotated`, or `dismissed` from a finish menu or direct finish keys, so that the orchestrator receives an unambiguous gate verdict.
24. As an operator, when I finish with `annotated`, I want `horch review` to require at least 1 note in the notes list (refusing in the UI status bar if 0 notes exist), so that the writer is never queued a revision task with an empty `annotated` notes file.
25. As an operator, when I have added notes and choose `approved`, I want `horch review` to either keep the notes in the file under `Result: approved` or prompt me to confirm `approved` versus `annotated`, so that I do not accidentally approve a file I meant to send back for revision.
26. As an operator, when I quit or dismiss the review without approving or annotating, I want to choose between writing `Result: dismissed` (and notifying the orchestrator) or aborting without writing a file (exit non-zero, sending no mail), so that an accidental window close or Ctrl-C does not silently tell the orchestrator the plan was dismissed unless I chose `dismissed`.
27. As an operator, I want the written file at `ai_docs/plans/<slug>/review-<target>-<n>.md` to match the notes file contract of `docs/specs/task-ledger.md` section 5.5b byte for byte: `# Review: <target>, round <n>`, `Reviewed: <repo-relative or given path>`, `Blob: <40-hex git blob sha>`, `Result: approved|annotated|dismissed`, and each note formatted as `N<k> <kind> L<start>-L<end> "<quoted start of line start, at most 80 chars>"` followed on the next line by the operator's note text.
28. As an operator, I want `horch review` to compute `Blob:` using the standard Git blob SHA-1 digest (`sha1("blob <byte_len>\0" + content)`) or `git hash-object <file>` without requiring the file to be committed first, so that uncommitted specs and ticket maps get an accurate content hash.
29. As an operator, I want `horch review` to send 1 durable mail line to `orchestrator` after writing the notes file when running inside a fleet workspace, and to still succeed (printing the written notes file path to stdout and exiting `0`) when no orchestrator is registered in the workspace, so that the tool works both during a live fleet run and stand-alone.
30. As a developer running `cargo test`, I want the notes file writer to be a pure function, the key handling to be a state machine with no terminal dependency, and 1 end-to-end test to verify the full CLI flow from file input and scripted keys to the written notes file and orchestrator mail, so that every behavior is verified hermetically without a real TTY.

## Test Seams

Following the ticket instructions and the repository's architecture rules (`docs/specs/architecture.md` §2.3–§2.4: functional core in `horch-core`, thin CLI and presentation in `horch`, hermetic tests in `horch-e2e`), `horch review` is tested at 3 seams:

1. **Seam 1 — Notes file writer and round/path resolver as pure functions (`crates/horch-core`)**:
   - A pure module in `crates/horch-core` (for example `horch_core::review`) defines the domain types (`ReviewTarget`, `ReviewDecision`, `NoteKind`, `LineRange`, `ReviewNote`, `ReviewDocument`) and the pure function `format_notes_file(doc: &ReviewDocument) -> String` (plus pure helpers for quoting the first line up to 80 characters, normalizing repo-relative paths, computing the git blob hash of the file bytes, and choosing the next round `<n>` from a list of existing file names).
   - **Why this seam**: It has zero I/O, zero terminal state, and zero process calls. Unit tests feed a `ReviewDocument` and assert that the output string matches the exact contract and example in `docs/specs/task-ledger.md` section 5.5b (header `# Review: <target>, round <n>`, `Reviewed: <path>`, `Blob: <sha>`, `Result: <decision>`, and `N<k> <kind> L<a>-L<b> "<quote>"\n<text>`).
2. **Seam 2 — Key handling and review session as a pure state machine with no terminal (`crates/horch-core` or `crates/horch/src/cmd/review/view.rs`)**:
   - Mirroring `crates/horch/src/cmd/telemetry/view.rs` (`on_key(state, key, ...)` and `ratatui::backend::TestBackend`), the interactive review state (`ReviewState`) and its transition function `on_key(state: &mut ReviewState, key: KeyInput) -> ReviewAction` take abstract key events and mutate the cursor line, selection anchor, mode (`Normal`, `Selecting`, `EditingNote`, `ConfirmFinish`, `Help`), note draft, and notes list without touching `crossterm` terminal raw mode or stdout.
   - Rendering (`draw_buf(state: &ReviewState, area: Rect, buf: &mut Buffer)`) draws into a `ratatui::buffer::Buffer` (or `TestBackend`) without a real TTY.
   - **Why this seam**: Every navigation key, multi-line selection, note addition (`change`, `question`, `delete`), note edit, note deletion, contiguous `N1..N<k>` renumbering, empty-note rejection, `annotated`-requires-notes guard, and `approved`/`annotated`/`dismissed` finish transition can be tested in fast in-memory unit tests with no PTY and no terminal device.
3. **Seam 3 — 1 hermetic end-to-end test (`crates/horch-e2e` or `crates/horch/tests/`)**:
   - One end-to-end test runs the built `horch review` binary in an isolated temp project and state root (using the existing `horch-e2e` fake pane host or a non-TTY scripted input seam / `ui-prop-tester` PTY harness), feeds a sequence of keystrokes that selects lines, adds notes of each kind, and finishes with `annotated`, and asserts:
     1. `ai_docs/plans/<slug>/review-<target>-1.md` exists on disk and equals the expected notes file contract of ticket 01.
     2. The orchestrator mailbox in the workspace holds 1 pending mail line naming `ai_docs/plans/<slug>/review-<target>-1.md` and `annotated`.

## Implementation Decisions

### 1. Existing crates and dependencies used

- **`crates/horch-core`**: Holds the pure review data model, path/slug/target inference, round discovery, git blob hash calculation, notes file serializer (`format_notes_file`), and pure key-handling state machine (`ReviewState`, `on_key`). Uses only existing `horch-core` dependencies (`anyhow`, `serde`, `fsx` for atomic writes).
- **`crates/horch`**: Holds the CLI subcommand `ToolCommand::Review` (or `ProjectCommand::Review`) in `crates/horch/src/main.rs` and the terminal runner in `crates/horch/src/cmd/review/` (`mod.rs`, `view.rs`). Uses existing `crates/horch` dependencies:
  - `clap` (already in `horch`) for CLI argument parsing (`horch review <file> [--slug <slug>] [--target spec|map]`).
  - `ratatui` `0.29` and `crossterm` `0.28` (already in `crates/horch/Cargo.toml` and `scripts/check-deps.sh` `allowed_horch`) for the terminal UI and key event reading, following the exact pattern of `crates/horch/src/cmd/telemetry/`.
  - `horch-core` messaging (`cmd::messaging::tell_as` / durable mail enqueue) to send the 1 mail line to `orchestrator`.
- **Blob SHA-1 calculation without a new dependency**:
  - The notes file contract requires `Blob: <git blob sha of that file>`. Git blob hashes are SHA-1 (`blob <len>\0<bytes>`). Neither `horch-core` nor `horch` has a `sha1` crate in `scripts/check-deps.sh` (`sha2` is present, not `sha1`).
  - Rather than adding a new crate dependency to `Cargo.toml` and `scripts/check-deps.sh`, `horch-core` computes the blob SHA either by calling `git hash-object -- <file>` via the existing `horch_core::vcs::git::GitCli` / `horch_marketplace::git::GitRunner` (which works on tracked and untracked files alike) or with a small self-contained 80-line pure-Rust SHA-1 function over `blob <len>\0<bytes>` inside `horch-core`. Using the pure-Rust function (or `git hash-object` with pure-Rust fallback) requires **zero new crate dependencies** and keeps `scripts/check-deps.sh` unchanged.
- **No browser, no network, no new service**: `horch review` runs synchronously in the calling terminal pane, reads the target file from disk, writes the notes file via `horch_core::fsx::write_atomic`, enqueues 1 mail line to `orchestrator`, and exits.

### 2. Command-line interface

```
horch review <FILE> [--slug <SLUG>] [--target <spec|map>]
```

- `<FILE>`: Required path to the markdown file to review (for example `docs/specs/operator-review.md` or `docs/tickets/external-imports/MAP.md`). May be passed as a repo-relative path or an absolute path inside the repository.
- `--slug <SLUG>`: Optional run/plan slug (matching `[A-Za-z0-9._-]+`). When omitted, inferred from the repo-relative path of `<FILE>`:
  - `docs/specs/<slug>.md` -> `<slug>`
  - `docs/tickets/<slug>/MAP.md` or `docs/tickets/<slug>/<file>.md` -> `<slug>`
  - `ai_docs/plans/<slug>/<file>.md` -> `<slug>`
  - If `<FILE>` matches none of these patterns and `--slug` is omitted, `horch review` exits `1` with an error asking for `--slug <slug>`.
- `--target <spec|map>`: Optional review target (`spec` or `map`). When omitted, inferred from `<FILE>`:
  - Path under `docs/specs/` or file name ending in `spec.md` -> `spec`
  - Path under `docs/tickets/` or file name `MAP.md` -> `map`
  - If `--target` cannot be inferred and is omitted, `horch review` exits `1` with an error asking for `--target spec|map`.

### 3. Notes file contract (output format)

`horch review` writes the exact format defined by ticket 01 (`docs/specs/task-ledger.md` section 5.5b):

- **Output path**: `<repo_root>/ai_docs/plans/<slug>/review-<target>-<n>.md`, where `<target>` is `spec` or `map` and `<n>` is the positive integer round (`1`, `2`, ...).
- **Round selection**: Before opening the UI (and re-verified at write time), scan `ai_docs/plans/<slug>/` for existing `review-<target>-<k>.md` files and set `<n>` to `max(<k>) + 1` (or `1` when none exists).
- **File structure**:

```markdown
# Review: <target>, round <n>

Reviewed: <repo-relative path of FILE>
Blob: <40-character lowercase hex git blob sha>
Result: <approved|annotated|dismissed>
```

- When there are notes (`N1`, `N2`, ...), a blank line follows `Result: <decision>`, and each note is separated by a blank line:

```markdown
N<k> <change|question|delete> L<start>-L<end> "<quoted start of line start>"
<operator text>
```

- **Formatting rules for each note `N<k>`**:
  - `<k>` is 1-based (`N1`, `N2`, ...), ordered by `(start_line, end_line, insertion_seq)`.
  - `<kind>` is lowercase `change`, `question`, or `delete`.
  - `L<start>-L<end>` uses 1-based inclusive line numbers of the reviewed file (`L40-L40` for a single line, `L12-L14` for a range).
  - `"<quoted start of line start>"` takes line `<start>` of the reviewed file, strips trailing `\r` and `\n`, replaces any internal `"` with `\"` (or `'`) so the quoted token stays on 1 line, and truncates to at most 80 Unicode scalar values (`.chars().take(80)`).
  - `<operator text>` is the trimmed operator note body (1 or more lines; blank lines inside a note are collapsed to single newlines so note blocks stay unambiguous).

### 4. Minimum TUI feature set and state machine

The state machine has 4 modes: `Browse`, `SelectRange`, `EditNote`, and `FinishPrompt` (plus a modal `?` help overlay).

| Feature | Behavior in the state machine |
|---|---|
| Show file with line numbers | Splits the file on `\n` into 1-based lines. Renders a scrollable file pane with right-aligned line numbers, a gutter column showing attached note badges (`N1`, `N2`, ...), and the line text. Empty files show 1 empty line `1`. |
| Move cursor & select a line range | `Up`/`k` and `Down`/`j` move `cursor_line` by 1; `PgUp`/`Ctrl-U` and `PgDn`/`Ctrl-D` move by viewport page height; `Home`/`g` and `End`/`G` jump to line 1 and the last line. Pressing `v` or `Space` in `Browse` enters `SelectRange` with `anchor_line = cursor_line`; moving the cursor updates the selected inclusive range `L<min(anchor, cursor)>-L<max(anchor, cursor)>`. `Esc` in `SelectRange` clears `anchor_line` and returns to `Browse`. |
| Add a note with a kind | Pressing `c` (`change`), `q` (`question`), or `d` (`delete`) in `Browse` or `SelectRange` opens `EditNote` pre-filled with that `NoteKind` and the current `LineRange` (`cursor_line..=cursor_line` in `Browse`, or the anchored range in `SelectRange`).Inside `EditNote`, the operator types text, uses `Tab` to cycle `change -> question -> delete`, presses `Enter` (or `Ctrl-S`) to save when the trimmed text is non-empty, or `Esc` to cancel. Saving inserts the note, clears `SelectRange`, and renumbers `N1..N<m>`. |
| Show the notes list | A second pane (right or bottom, according to terminal width) lists all notes `N1..N<m>` with kind, `L<start>-L<end>`, quoted line start, and summary text, or `"No notes yet."` when empty. `Tab` switches focus between the file pane and the notes list pane; `n` and `p` jump to the next and previous note and scroll the file view to that note's `start_line`. |
| Edit and delete a note | When a note is selected in the notes list (or when the cursor is on a line with a note and the operator presses `e`), pressing `e` opens `EditNote` loaded with that note's kind, line range, and text; saving updates the note in place. Pressing `x` or `Delete` on a selected note removes it and renumbers the remaining notes `N1..N<m>`. |
| Finish with `approved`, `annotated`, or `dismissed` | Pressing `f` (or `F`) opens the `FinishPrompt` bar offering `a` = `approved`, `n` = `annotated`, `x` = `dismissed`, `Esc` = back. Direct shortcuts from `Browse` are also supported (`A` for `approved`, `S` for `annotated`, `D` for `dismissed`). Choosing `annotated` when `notes.is_empty()` is refused in the status bar (`"add at least 1 note before finishing as annotated"`). Choosing `approved` when `!notes.is_empty()` asks for confirmation (`"notes exist; press a again for approved, n for annotated, Esc to cancel"`). Pressing `Ctrl-C` or `Q` prompts `"x: finish as dismissed, q: abort without writing, Esc: back"`. |

### 5. Writing the notes file and notifying the orchestrator

When the state machine returns `ReviewAction::Finish(decision)`:

1. Build `ReviewDocument` with `target`, `round`, repo-relative `reviewed_path`, `blob_sha`, `decision`, and the ordered `notes`.
2. Render the markdown string via the pure function `format_notes_file(&doc)`.
3. Ensure `ai_docs/plans/<slug>/` exists and atomically write `ai_docs/plans/<slug>/review-<target>-<n>.md` using `horch_core::fsx::write_atomic`.
4. Send 1 mail line to `orchestrator`:
   - Call `cmd::messaging::tell_as(ctx, Some("operator".to_string()), "orchestrator", &msg)` (or when `ctx.worker.role` is set, send from `"operator"` so the orchestrator sees an operator review notification) with:
     `[operator] NOTE: Review <target> round <n> is <decision>: ai_docs/plans/<slug>/review-<target>-<n>.md`
   - Because `role == "orchestrator"` and `sender.is_some()`, `is_durable` in `crates/horch/src/cmd/messaging.rs` is `true`: the message is enqueued durably in the orchestrator's mailbox and pushed through the channel relay or typed into the orchestrator pane.
   - If the command runs outside a herdr workspace or no orchestrator is registered in the workspace, log a 1-line note to stderr and still exit `0` after printing `ai_docs/plans/<slug>/review-<target>-<n>.md` on stdout.

## Testing Decisions

- **What makes a good test**:
  - Test observable behavior at the 3 seams: the exact formatted markdown text of the notes file, the state machine's observable state (`cursor_line`, selected range, `notes` list, active mode, status message, and `ReviewAction::Finish` outcome) after sequences of key inputs, the rendered `ratatui` text frame on a `TestBackend`, and the end-to-end CLI execution producing the file on disk and the mail in the orchestrator inbox.
  - Write zero code comments in all implementation and test files, in compliance with ADR 0004 (`scripts/comments/comment_lint.py`).
- **Modules to be tested**:
  - `horch_core::review` (or `crates/horch/src/cmd/review/`):
    - Pure notes file writer (`format_notes_file`): exact match against the example in `docs/specs/task-ledger.md` section 5.5b; 80-character quote truncation; quote escaping; CRLF stripping; empty notes list for `approved` and `dismissed`; multi-note ordering and `N1..N<k>` numbering; git blob SHA calculation.
    - Pure path and round resolver: inferring `slug` and `target` from `docs/specs/<slug>.md` and `docs/tickets/<slug>/MAP.md`; explicit `--slug` and `--target` overrides; error on uninferrable path; round `<n>` increment when `review-spec-1.md` already exists.
    - Pure state machine (`on_key`): cursor movement bounds, range selection (`v`, cursor motion, `Esc`), adding `change`, `question`, and `delete` notes, cycling kind in the editor, rejecting empty note text, editing an existing note, deleting a note and renumbering remaining notes, refusing `annotated` with 0 notes, confirming `approved` when notes exist, and finishing with `approved`, `annotated`, or `dismissed`.
    - Frame rendering (`draw_buf` with `ratatui::backend::TestBackend`): verifying that line numbers, note gutter markers, the notes list, and the finish prompt render without panicking at minimum terminal dimensions (`20x6` and `120x40`).
  - End-to-end test (`crates/horch-e2e` or `crates/horch/tests/`):
    - Runs `horch review` on a fixture spec file in a temporary repo and workspace, drives a scripted review session to completion with `annotated`, verifies the written `ai_docs/plans/<slug>/review-spec-1.md` file contents, and verifies that `horch inbox --peek` as the orchestrator shows the 1 delivered mail line with the notes file path.
- **Prior art in the codebase**:
  - `crates/horch/src/cmd/telemetry/view.rs` and `crates/horch/src/cmd/telemetry/mod.rs`: pure `ViewState` + `on_key(state, key, rows) -> (ViewState, Action)` + `draw_buf` tested via `ratatui::backend::TestBackend` without a real terminal.
  - `crates/horch/src/cmd/messaging.rs`: `tell_as` durable mail delivery to `orchestrator`.
  - `crates/horch-e2e/tests/`: hermetic end-to-end tests with `Harness::new`, fake `herdr` / `hc`, and isolated `HORCH_STATE_DIR`.

## Out of Scope

- Any browser UI, local HTTP server, WebSocket server, or background daemon.
- Any third-party binary, plugin, or npm package (`docs/specs/external-imports.md` D1).
- Hooking Claude Code `ExitPlanMode` or running an interactive review inside a worker pane.
- Diffing two plan revisions inside `horch review` (git already diffs committed specs and tickets).
- Reviewing source code diffs or pull requests (`code-sweep` and `skills/code-review/SKILL.md` handle code review).
- Modifying the task ledger SQLite schema (`~/.horch/ledger.sqlite3`): the review gate stores its result in `ai_docs/plans/<slug>/review-<target>-<n>.md` and `ai_docs/plans/<slug>/decisions.md` as defined by ticket 01.

## Open Questions

1. **Do these seams match your expectations?**
   - **Options**:
     - **Option 1 (Recommended)**: Use the 3 seams specified in this document: (1) the notes file writer, blob hasher, and slug/round resolver as pure functions in `horch-core`, (2) the key handler and screen renderer as a terminal-free state machine tested with `KeyEvent` values and `ratatui::backend::TestBackend`, and (3) 1 hermetic end-to-end test that runs `horch review` and checks the written notes file and the orchestrator mail line.
     - **Option 2**: Keep all review code inside `crates/horch/src/cmd/review/` without exposing the pure functions in `horch-core`.
   - **Recommendation**: **Option 1**. It matches the ticket's 3 required seams and `docs/specs/architecture.md` (functional core in `horch-core`, thin CLI in `horch`), and follows the exact `telemetry/view.rs` precedent for testing `ratatui` UIs without a TTY.

2. **How should the end-to-end test drive `horch review` without an interactive operator TTY?**
   - **Options**:
     - **Option 1 (Recommended)**: Support a hidden test-only input seam on `horch review` (or an environment variable / `--keys <keys>` flag, or reading key lines from stdin when a hidden `--scripted-stdin` flag is passed) that feeds key events through the exact same `on_key` loop, write path, and `tell_as` mail path as the live `crossterm` event loop.
     - **Option 2**: Spawn `horch review` inside a real Unix pseudo-terminal (`libc::openpty`) in the e2e test and write raw ANSI key bytes to the PTY master fd.
   - **Recommendation**: **Option 1 (plus optional PTY test on Unix)**. A hidden `--scripted-stdin` (or `--keys`) flag exercises the entire binary entrypoint, file loader, state machine, `draw_buf` renderer (against a `TestBackend`), atomic file writer, and `tell_as` mail delivery deterministically on every OS (Linux, macOS, and Windows, per NFR-04) without PTY timing flake.

3. **How should `horch review` behave when the operator chooses `approved` while notes exist in the session?**
   - **Options**:
     - **Option 1 (Recommended)**: Require a confirmation keypress (`a` again to confirm `approved` with the notes included, `n` to switch to `annotated`, or `Esc` to return to editing), and if confirmed as `approved`, write `Result: approved` along with any notes `N1..N<k>` the operator left.
     - **Option 2**: Forbid `approved` whenever any note exists (`notes.len() > 0`), forcing the operator to either delete all notes or choose `annotated`.
     - **Option 3**: Automatically discard all notes when `approved` is chosen.
   - **Recommendation**: **Option 1**. It prevents accidental approval when the operator meant `annotated`, never silently destroys notes the operator typed, and allows non-blocking advisory notes on an approved spec if the operator explicitly confirms.

4. **What sender tag should the mail line to the orchestrator carry?**
   - **Options**:
     - **Option 1 (Recommended)**: Send durable mail with sender `"operator"` and line `[operator] NOTE: Review <target> round <n> is <result>: ai_docs/plans/<slug>/review-<target>-<n>.md`.
     - **Option 2**: Send durable mail with sender `"horch"` and line `[horch] NOTE: Review <target> round <n> is <result>: ai_docs/plans/<slug>/review-<target>-<n>.md`.
   - **Recommendation**: **Option 1 (`[operator]` or `[horch]`)**. Using `tell_as(ctx, Some("horch".into()), "orchestrator", ...)` or `Some("operator".into())` both route through durable mail (`is_durable` in `crates/horch/src/cmd/messaging.rs` is `true` for any `Some(_)` sender when `role == "orchestrator"`). `[horch] NOTE:` is already a recognized prefix in the orchestrator briefing for system notifications, whereas `[operator] NOTE:` emphasizes that the operator completed the review; either works, and `[horch] NOTE:` requires zero changes to `horch_line()` in `crates/horch-core/src/messaging/mail.rs`.

5. **How should `horch review` compute the `Blob:` SHA-1 header when the file has uncommitted changes?**
   - **Options**:
     - **Option 1 (Recommended)**: Compute the Git blob SHA-1 directly in pure Rust over the exact file bytes loaded into the review session (`sha1("blob <len>\0" + bytes)`), without spawning a `git` subprocess.
     - **Option 2**: Spawn `git hash-object -- <file>` via `GitCli`.
   - **Recommendation**: **Option 1**. A pure-Rust SHA-1 over `blob <len>\0<bytes>` is ~80 lines of arithmetic, needs no external crate, needs no `git` subprocess, cannot race if the file on disk changes while the TUI is open, and makes Seam 1 (`format_notes_file` + document builder) 100% pure and hermetic.

## Further Notes

- **Clean-room provenance**: This spec was written by a clean worker from the operator's requirements in ticket 01 (`docs/tickets/external-imports/01-spec-gate.md`), ticket 02 (`docs/tickets/external-imports/02-review-tool-spec.md`), and `docs/specs/external-imports.md`, alongside the existing `horch` codebase.
- **No requirement IDs in this spec before tests exist**: Per `docs/testing-and-gates.md` ("When you add a requirement table to a spec, add the tests in the same commit, or the gate fails"), this spec does not define a machine-scanned `| ID | Requirement |` table; the implementation tickets for `horch review` can add requirement IDs together with their tests if desired.
