# Terminal runs

```
ui-prop-tester terminal test [OPTIONS] <PROGRAM> [ARGS...]
ui-prop-tester terminal fuzz [OPTIONS] <PROGRAM> [ARGS...]
```

The program runs in a pseudo-terminal with `TERM=xterm-256color`. Its output goes through a terminal
emulator. Options come before `<PROGRAM>`; everything after it goes to the program.

| option | default | meaning |
|---|---|---|
| `--columns`, `--rows` | 100, 40 | Screen size |
| `--scrollback-lines-max` | 100 | Lines kept above the screen |
| `--quiescence-timeout-ms` | 5 | Output must stay quiet this long before a state is captured |

A state is captured when output has been quiet for the quiescence time, or after 1 second of nonstop output
(a program that repaints all the time). Raise `--quiescence-timeout-ms` (50 to 200) for a program that
answers in bursts, or states will catch it mid-paint.

The run ends when the program exits. If input could not be delivered and the program then stays silent for
5 seconds, the run stops with exit 1 (hung target). The child is always killed when the run ends.

## Defaults (`ui-prop-tester/terminal/defaults`)

| export | kind | rule or actions |
|---|---|---|
| `exitsCleanly` | property | A normal exit has code 0. Death by signal is reported in the summary and does not fail this |
| `noReplacementCharacters` | property | No visible cell holds U+FFFD |
| `typing` | generator | Equal choice: safe Unicode text, or Enter/Tab |

The default never sends Ctrl+C, Ctrl+D, Ctrl+Z or Alt+Backspace.

## Builders and key sets (`ui-prop-tester/terminal`)

| export | gives |
|---|---|
| `write(textOrGenerator)` | Send text |
| `writeBytes(arrayOfNumbers)` | Send raw bytes, for example malformed UTF-8 or an escape sequence |
| `resize(columns, rows)` | Resize the emulator and the child's terminal |
| `click(row, column)` | Primary-button press and release at zero-based cell coordinates (SGR mouse) |
| `scrollToTop()`, `scrollToBottom()` | Move the view to the top or bottom of scrollback |
| `safeText` | Write 1 to 8 safe characters: printable ASCII, extended Latin, Greek, Cyrillic, CJK, Hangul, emoji |
| `safeCharacters` | The same text as a value, for `write(...)` or `bracketedPaste(...)` |
| `controlKeys` | Enter, Tab |
| `editingKeys` | Backspace, Delete, Insert |
| `arrowKeys`, `modifiedArrowKeys` | Arrows, and arrows with Shift, Alt or Ctrl |
| `navigationKeys` | Home, End, Page Up, Page Down |
| `functionKeys` | F1 to F12 |
| `bracketedPaste(textOrGenerator)` | Text wrapped in paste markers |

The key sets are action generators; use them directly or inside `oneOf`. The arguments of `click` and `resize`
may be generators. `cell(row, column)` returns `null` for a position off the grid.

## State seen by extractors

| field | value |
|---|---|
| `columns`, `rows` | Screen size |
| `lines` | 1 string per visible row, trailing blanks trimmed |
| `cell(row, column)` | The cell: `text`, `wide_continuation`, `fg`, `bg`, `underline_color`, `underline`, `bold`, `italic`, `blink`, `inverse`, `strikethrough`, `dim`, `invisible`, `overline` |
| `cells` | Only the cells that are not a default blank, each with `row`, `column`, `text` and its non-default attributes |
| `cursor` | `row`, `column`, `visible`, `blink`, `shape` (`block`, `underline`, `bar`), `color` |
| `scroll_offset` | 0 at the bottom |
| `scrollback` | `lines`, `cells` and `cell(row, column)` for the kept lines |
| `exit` | `null` while running, then `{ code, signal }` |
| `last_action` | The action that led to this state |

A colour is `null` (default), `{ indexed: n }` or `{ rgb: [r, g, b] }`. `underline` is `null`, `single`,
`double`, `curly`, `dotted` or `dashed`.

## Example: a full-screen TUI

```js
import { always, extract, from, integer, oneOf } from "ui-prop-tester";
import { arrowKeys, controlKeys, resize, write } from "ui-prop-tester/terminal";
export { exitsCleanly, noReplacementCharacters } from "ui-prop-tester/terminal/defaults";

const screen = extract((state) => state.lines.join("\n"));
const statusRow = extract((state) => state.lines[state.rows - 1] ?? "");
const cursorOnScreen = extract((state) => state.cursor.row < state.rows && state.cursor.column < state.columns);
const selectedRows = extract((state) => state.cells.filter((cell) => cell.inverse).map((cell) => cell.row));

export const noPanicText = always(() => !/panicked at|Traceback/.test(screen.current));
export const statusRowNeverBlank = always(() => statusRow.current.length > 0);
export const cursorStaysOnScreen = always(() => cursorOnScreen.current);
export const oneSelection = always(() => new Set(selectedRows.current).size <= 1);

export const moves = oneOf(arrowKeys, controlKeys).weight(10);
export const commands = write(from(["j", "k", "/", "q"]));
export const resizing = resize(integer(40, 160), integer(10, 50));
```

```
scripts/ui-prop-tester terminal test --quiescence-timeout-ms 50 --actions-max 500 \
  --specification tui.spec.js --output-path tui.trace ./target/debug/mytui
```

## Reading a terminal trace

`trace tui.trace --screen` draws the screen of each state; add `--color` for colours and attributes, and
`--state 40..45` for a range. See [traces](traces.md).
