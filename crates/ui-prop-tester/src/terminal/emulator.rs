use std::collections::VecDeque;

use serde_json::{json, Map, Value};
use unicode_width::UnicodeWidthChar;

use super::cell::{Attributes, Cell, Color};
use super::grid::{Grid, Row};
use super::sgr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

impl CursorShape {
    fn name(self) -> &'static str {
        match self {
            CursorShape::Block => "block",
            CursorShape::Underline => "underline",
            CursorShape::Bar => "bar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Ascii,
    LineDrawing,
}

#[derive(Debug, Clone, Copy)]
struct SavedCursor {
    row: usize,
    column: usize,
    pending_wrap: bool,
    attributes: Attributes,
    origin: bool,
    charsets: [Charset; 2],
    shift: usize,
}

#[derive(Debug, Clone)]
struct Modes {
    autowrap: bool,
    origin: bool,
    insert: bool,
    newline: bool,
    cursor_visible: bool,
    cursor_blink: bool,
    cursor_shape: CursorShape,
}

impl Default for Modes {
    fn default() -> Self {
        Modes {
            autowrap: true,
            origin: false,
            insert: false,
            newline: false,
            cursor_visible: true,
            cursor_blink: true,
            cursor_shape: CursorShape::Block,
        }
    }
}

struct Screen {
    columns: usize,
    rows: usize,
    primary: Grid,
    alternate: Grid,
    alternate_active: bool,
    scrollback: VecDeque<Row>,
    scrollback_max: usize,
    scroll_offset: usize,
    row: usize,
    column: usize,
    pending_wrap: bool,
    attributes: Attributes,
    saved_primary: Option<SavedCursor>,
    saved_alternate: Option<SavedCursor>,
    scroll_top: usize,
    scroll_bottom: usize,
    tab_stops: Vec<bool>,
    modes: Modes,
    cursor_color: Option<Color>,
    charsets: [Charset; 2],
    shift: usize,
    last_printed: Option<char>,
    responses: Vec<u8>,
}

pub struct Emulator {
    parser: vte::Parser,
    screen: Screen,
}

fn default_tab_stops(columns: usize) -> Vec<bool> {
    (0..columns)
        .map(|column| column > 0 && column % 8 == 0)
        .collect()
}

fn line_drawing(character: char) -> char {
    match character {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        other => other,
    }
}

fn parse_color_spec(spec: &str) -> Option<Color> {
    if let Some(hex) = spec.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let component = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
        return Some(Color::Rgb(component(0)?, component(2)?, component(4)?));
    }
    let body = spec.strip_prefix("rgb:")?;
    let parts: Vec<&str> = body.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let scale = |part: &str| -> Option<u8> {
        if part.is_empty() || part.len() > 4 {
            return None;
        }
        let value = u32::from_str_radix(part, 16).ok()?;
        let maximum = (1u32 << (4 * part.len() as u32)) - 1;
        Some(((value * 255 + maximum / 2) / maximum) as u8)
    };
    Some(Color::Rgb(
        scale(parts[0])?,
        scale(parts[1])?,
        scale(parts[2])?,
    ))
}

fn trimmed_line(row: &Row) -> String {
    let text: String = row.iter().map(|cell| cell.text.as_str()).collect();
    text.trim_end_matches(' ').to_string()
}

fn sparse_cells(rows: &[&Row]) -> Vec<Value> {
    let mut cells = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column, cell) in row.iter().enumerate() {
            if !cell.is_default_blank() {
                cells.push(cell.to_json(row_index, column));
            }
        }
    }
    cells
}

impl Screen {
    fn new(columns: usize, rows: usize, scrollback_max: usize) -> Screen {
        let columns = columns.max(1);
        let rows = rows.max(1);
        Screen {
            columns,
            rows,
            primary: Grid::new(columns, rows),
            alternate: Grid::new(columns, rows),
            alternate_active: false,
            scrollback: VecDeque::new(),
            scrollback_max,
            scroll_offset: 0,
            row: 0,
            column: 0,
            pending_wrap: false,
            attributes: Attributes::default(),
            saved_primary: None,
            saved_alternate: None,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            tab_stops: default_tab_stops(columns),
            modes: Modes::default(),
            cursor_color: None,
            charsets: [Charset::Ascii; 2],
            shift: 0,
            last_printed: None,
            responses: Vec::new(),
        }
    }

    fn grid(&mut self) -> &mut Grid {
        if self.alternate_active {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    fn fill(&self) -> Attributes {
        self.attributes.erased()
    }

    fn push_scrollback(&mut self, rows: Vec<Row>) {
        if self.scrollback_max == 0 {
            return;
        }
        for row in rows {
            self.scrollback.push_back(row);
            if self.scroll_offset > 0 {
                self.scroll_offset += 1;
            }
        }
        while self.scrollback.len() > self.scrollback_max {
            self.scrollback.pop_front();
        }
        self.scroll_offset = self.scroll_offset.min(self.scrollback.len());
    }

    fn scroll_region_up(&mut self, count: usize) {
        let (top, bottom, fill) = (self.scroll_top, self.scroll_bottom, self.fill());
        let removed = self.grid().scroll_up(top, bottom, count, fill);
        if top == 0 && !self.alternate_active {
            self.push_scrollback(removed);
        }
    }

    fn scroll_region_down(&mut self, count: usize) {
        let (top, bottom, fill) = (self.scroll_top, self.scroll_bottom, self.fill());
        self.grid().scroll_down(top, bottom, count, fill);
    }

    fn index(&mut self) {
        if self.row == self.scroll_bottom {
            self.scroll_region_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.row == self.scroll_top {
            self.scroll_region_down(1);
        } else if self.row > 0 {
            self.row -= 1;
        }
    }

    fn print_character(&mut self, character: char) {
        let character = if self.charsets[self.shift] == Charset::LineDrawing {
            line_drawing(character)
        } else {
            character
        };
        let width = UnicodeWidthChar::width(character).unwrap_or(1);
        if width == 0 {
            self.append_combining(character);
            return;
        }
        if self.pending_wrap && self.modes.autowrap {
            self.column = 0;
            self.index();
        }
        self.pending_wrap = false;
        if width > self.columns {
            return;
        }
        if self.column + width > self.columns {
            if self.modes.autowrap {
                let (row, column, fill) = (self.row, self.column, self.fill());
                let end = self.columns;
                self.grid().erase(row, column, end, fill);
                self.column = 0;
                self.index();
            } else {
                self.column = self.columns - width;
            }
        }
        let (row, column, attributes) = (self.row, self.column, self.attributes);
        if self.modes.insert {
            self.grid()
                .insert_cells(row, column, width, attributes.erased());
        }
        let columns = self.columns;
        let grid = self.grid();
        let cells = grid.row_mut(row);
        if cells[column].wide_continuation && column > 0 {
            let head_attributes = cells[column - 1].attributes;
            cells[column - 1] = Cell::blank(head_attributes);
        }
        let end = column + width;
        if end < columns && cells[end].wide_continuation {
            let tail_attributes = cells[end].attributes;
            cells[end] = Cell::blank(tail_attributes);
        }
        cells[column] = Cell {
            text: character.to_string(),
            wide_continuation: false,
            attributes,
        };
        if width == 2 {
            cells[column + 1] = Cell::continuation(attributes);
        }
        if end >= columns {
            self.column = columns - 1;
            self.pending_wrap = true;
        } else {
            self.column = end;
        }
        self.last_printed = Some(character);
    }

    fn append_combining(&mut self, character: char) {
        let row = self.row;
        let mut column = if self.pending_wrap {
            self.column
        } else if self.column > 0 {
            self.column - 1
        } else {
            return;
        };
        let grid = self.grid();
        if grid.row(row)[column].wide_continuation && column > 0 {
            column -= 1;
        }
        grid.row_mut(row)[column].text.push(character);
    }

    fn tab_forward(&mut self, count: usize) {
        for _ in 0..count {
            let mut column = self.column + 1;
            while column < self.columns && !self.tab_stops[column] {
                column += 1;
            }
            self.column = column.min(self.columns - 1);
        }
        self.pending_wrap = false;
    }

    fn tab_backward(&mut self, count: usize) {
        for _ in 0..count {
            let mut column = self.column;
            while column > 0 {
                column -= 1;
                if self.tab_stops[column] {
                    break;
                }
            }
            self.column = column;
        }
        self.pending_wrap = false;
    }

    fn move_to(&mut self, row: usize, column: usize) {
        let (top, bottom) = if self.modes.origin {
            (self.scroll_top, self.scroll_bottom)
        } else {
            (0, self.rows - 1)
        };
        self.row = (top + row).min(bottom);
        self.column = column.min(self.columns - 1);
        self.pending_wrap = false;
    }

    fn move_rows_up(&mut self, count: usize) {
        let limit = if self.row >= self.scroll_top {
            self.scroll_top
        } else {
            0
        };
        self.row = self.row.saturating_sub(count).max(limit);
        self.pending_wrap = false;
    }

    fn move_rows_down(&mut self, count: usize) {
        let limit = if self.row <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.rows - 1
        };
        self.row = (self.row + count).min(limit);
        self.pending_wrap = false;
    }

    fn save_cursor(&mut self) {
        let saved = SavedCursor {
            row: self.row,
            column: self.column,
            pending_wrap: self.pending_wrap,
            attributes: self.attributes,
            origin: self.modes.origin,
            charsets: self.charsets,
            shift: self.shift,
        };
        if self.alternate_active {
            self.saved_alternate = Some(saved);
        } else {
            self.saved_primary = Some(saved);
        }
    }

    fn restore_cursor(&mut self) {
        let saved = if self.alternate_active {
            self.saved_alternate
        } else {
            self.saved_primary
        };
        match saved {
            Some(saved) => {
                self.row = saved.row.min(self.rows - 1);
                self.column = saved.column.min(self.columns - 1);
                self.pending_wrap = saved.pending_wrap;
                self.attributes = saved.attributes;
                self.modes.origin = saved.origin;
                self.charsets = saved.charsets;
                self.shift = saved.shift;
            }
            None => {
                self.row = 0;
                self.column = 0;
                self.pending_wrap = false;
                self.attributes = Attributes::default();
                self.modes.origin = false;
            }
        }
    }

    fn enter_alternate(&mut self, save: bool, clear: bool) {
        if self.alternate_active {
            return;
        }
        if save {
            self.save_cursor();
        }
        self.alternate_active = true;
        self.scroll_offset = 0;
        if clear {
            let fill = self.fill();
            self.alternate.clear(fill);
        }
    }

    fn leave_alternate(&mut self, restore: bool, clear: bool) {
        if !self.alternate_active {
            return;
        }
        if clear {
            self.alternate.clear(Attributes::default());
        }
        self.alternate_active = false;
        if restore {
            self.restore_cursor();
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let (row, column, rows, columns, fill) =
            (self.row, self.column, self.rows, self.columns, self.fill());
        match mode {
            0 => {
                let grid = self.grid();
                grid.erase(row, column, columns, fill);
                grid.erase_rows(row + 1, rows, fill);
            }
            1 => {
                let grid = self.grid();
                grid.erase_rows(0, row, fill);
                grid.erase(row, 0, column + 1, fill);
            }
            2 => self.grid().erase_rows(0, rows, fill),
            3 => {
                self.scrollback.clear();
                self.scroll_offset = 0;
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let (row, column, columns, fill) = (self.row, self.column, self.columns, self.fill());
        let range = match mode {
            0 => (column, columns),
            1 => (0, column + 1),
            2 => (0, columns),
            _ => return,
        };
        self.grid().erase(row, range.0, range.1, fill);
    }

    fn set_mode(&mut self, private: bool, mode: u16, enabled: bool) {
        if !private {
            match mode {
                4 => self.modes.insert = enabled,
                20 => self.modes.newline = enabled,
                _ => {}
            }
            return;
        }
        match mode {
            6 => {
                self.modes.origin = enabled;
                self.move_to(0, 0);
            }
            7 => self.modes.autowrap = enabled,
            12 => self.modes.cursor_blink = enabled,
            25 => self.modes.cursor_visible = enabled,
            47 => {
                if enabled {
                    self.enter_alternate(false, false);
                } else {
                    self.leave_alternate(false, false);
                }
            }
            1047 => {
                if enabled {
                    self.enter_alternate(false, false);
                } else {
                    self.leave_alternate(false, true);
                }
            }
            1048 => {
                if enabled {
                    self.save_cursor();
                } else {
                    self.restore_cursor();
                }
            }
            1049 => {
                if enabled {
                    self.enter_alternate(true, true);
                } else {
                    self.leave_alternate(true, false);
                }
            }
            _ => {}
        }
    }

    fn soft_reset(&mut self) {
        self.modes = Modes::default();
        self.attributes = Attributes::default();
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.charsets = [Charset::Ascii; 2];
        self.shift = 0;
        self.saved_primary = None;
        self.saved_alternate = None;
        self.pending_wrap = false;
    }

    fn full_reset(&mut self) {
        let mut fresh = Screen::new(self.columns, self.rows, self.scrollback_max);
        fresh.scrollback = std::mem::take(&mut self.scrollback);
        fresh.responses = std::mem::take(&mut self.responses);
        *self = fresh;
    }

    fn report_cursor(&mut self, private: bool) {
        let row = if self.modes.origin {
            self.row.saturating_sub(self.scroll_top)
        } else {
            self.row
        };
        let marker = if private { "?" } else { "" };
        let reply = format!("\x1b[{marker}{};{}R", row + 1, self.column + 1);
        self.responses.extend_from_slice(reply.as_bytes());
    }

    fn set_cursor_style(&mut self, style: u16) {
        let (shape, blink) = match style {
            0 | 1 => (CursorShape::Block, true),
            2 => (CursorShape::Block, false),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Bar, true),
            6 => (CursorShape::Bar, false),
            _ => return,
        };
        self.modes.cursor_shape = shape;
        self.modes.cursor_blink = blink;
    }

    fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(1);
        let rows = rows.max(1);
        if columns != self.columns {
            self.primary.resize_columns(columns);
            self.alternate.resize_columns(columns);
            self.scrollback
                .iter_mut()
                .for_each(|row| row.truncate(columns));
            let mut stops = default_tab_stops(columns);
            let kept = self.tab_stops.len().min(columns);
            stops[..kept].copy_from_slice(&self.tab_stops[..kept]);
            self.tab_stops = stops;
            self.columns = columns;
        }
        if rows != self.rows {
            let old_rows = self.rows;
            let primary_cursor = if self.alternate_active {
                self.saved_primary
                    .map(|saved| saved.row)
                    .unwrap_or(old_rows - 1)
            } else {
                self.row
            };
            let alternate_cursor = if self.alternate_active {
                self.row
            } else {
                old_rows - 1
            };
            let (primary_shift, removed) =
                Self::resize_rows(&mut self.primary, rows, primary_cursor);
            let (alternate_shift, _) =
                Self::resize_rows(&mut self.alternate, rows, alternate_cursor);
            self.push_scrollback(removed);
            let shift = if self.alternate_active {
                alternate_shift
            } else {
                primary_shift
            };
            self.row = self.row.saturating_sub(shift);
            if let Some(saved) = self.saved_primary.as_mut() {
                saved.row = saved.row.saturating_sub(primary_shift).min(rows - 1);
            }
            self.rows = rows;
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.row = self.row.min(self.rows - 1);
        self.column = self.column.min(self.columns - 1);
        self.pending_wrap = false;
        self.scroll_offset = self.scroll_offset.min(self.scrollback.len());
    }

    fn resize_rows(grid: &mut Grid, rows: usize, cursor_row: usize) -> (usize, Vec<Row>) {
        let old_rows = grid.row_count();
        if rows >= old_rows {
            grid.add_bottom_rows(rows - old_rows);
            return (0, Vec::new());
        }
        let excess = old_rows - rows;
        let below_cursor = old_rows - 1 - cursor_row.min(old_rows - 1);
        let blank_below = grid.rows()[old_rows - below_cursor..]
            .iter()
            .rev()
            .take_while(|row| row.iter().all(Cell::is_default_blank))
            .count();
        let from_bottom = excess.min(blank_below);
        grid.remove_bottom_rows(from_bottom);
        let from_top = excess - from_bottom;
        let removed = grid.remove_top_rows(from_top);
        (from_top, removed)
    }

    fn visible_rows(&self) -> Vec<&Row> {
        let grid = if self.alternate_active {
            &self.alternate
        } else {
            &self.primary
        };
        let offset = if self.alternate_active {
            0
        } else {
            self.scroll_offset.min(self.scrollback.len())
        };
        let from_scrollback = offset.min(self.rows);
        let start = self.scrollback.len() - offset;
        let mut rows: Vec<&Row> = self
            .scrollback
            .range(start..start + from_scrollback)
            .collect();
        rows.extend(grid.rows().iter().take(self.rows - from_scrollback));
        rows
    }

    fn csi(&mut self, params: &[Vec<u16>], intermediates: &[u8], action: char) {
        let first = |default: u16| -> u16 {
            match params.first().and_then(|param| param.first()).copied() {
                None | Some(0) => default,
                Some(value) => value,
            }
        };
        let raw = |index: usize| -> u16 {
            params
                .get(index)
                .and_then(|param| param.first())
                .copied()
                .unwrap_or(0)
        };
        let count = first(1) as usize;
        let private = intermediates.first() == Some(&b'?');
        match (intermediates, action) {
            ([], '@') => {
                let (row, column, fill) = (self.row, self.column, self.fill());
                self.grid().insert_cells(row, column, count, fill);
            }
            ([], 'A') => self.move_rows_up(count),
            ([], 'B') | ([], 'e') => self.move_rows_down(count),
            ([], 'C') | ([], 'a') => {
                self.column = (self.column + count).min(self.columns - 1);
                self.pending_wrap = false;
            }
            ([], 'D') => {
                self.column = self.column.saturating_sub(count);
                self.pending_wrap = false;
            }
            ([], 'E') => {
                self.move_rows_down(count);
                self.column = 0;
            }
            ([], 'F') => {
                self.move_rows_up(count);
                self.column = 0;
            }
            ([], 'G') | ([], '`') => {
                self.column = (count - 1).min(self.columns - 1);
                self.pending_wrap = false;
            }
            ([], 'H') | ([], 'f') => {
                let row = first(1) as usize - 1;
                let column = match raw(1) {
                    0 => 0,
                    value => value as usize - 1,
                };
                self.move_to(row, column);
            }
            ([], 'I') => self.tab_forward(count),
            ([], 'J') | ([b'?'], 'J') => self.erase_display(raw(0)),
            ([], 'K') | ([b'?'], 'K') => self.erase_line(raw(0)),
            ([], 'L') => {
                if (self.scroll_top..=self.scroll_bottom).contains(&self.row) {
                    let (row, bottom, fill) = (self.row, self.scroll_bottom, self.fill());
                    self.grid().scroll_down(row, bottom, count, fill);
                    self.column = 0;
                    self.pending_wrap = false;
                }
            }
            ([], 'M') => {
                if (self.scroll_top..=self.scroll_bottom).contains(&self.row) {
                    let (row, bottom, fill) = (self.row, self.scroll_bottom, self.fill());
                    self.grid().scroll_up(row, bottom, count, fill);
                    self.column = 0;
                    self.pending_wrap = false;
                }
            }
            ([], 'P') => {
                let (row, column, fill) = (self.row, self.column, self.fill());
                self.grid().delete_cells(row, column, count, fill);
            }
            ([], 'S') => self.scroll_region_up(count),
            ([], 'T') => self.scroll_region_down(count),
            ([], 'X') => {
                let (row, column, fill) = (self.row, self.column, self.fill());
                self.grid().erase(row, column, column + count, fill);
            }
            ([], 'Z') => self.tab_backward(count),
            ([], 'b') => {
                if let Some(character) = self.last_printed {
                    for _ in 0..count.min(65535) {
                        self.print_character(character);
                    }
                }
            }
            ([], 'c') if raw(0) == 0 => self.responses.extend_from_slice(b"\x1b[?62;22c"),
            ([b'>'], 'c') => self.responses.extend_from_slice(b"\x1b[>1;10;0c"),
            ([], 'd') => {
                let column = self.column;
                self.move_to(count - 1, column);
            }
            ([], 'g') => match raw(0) {
                0 => {
                    let column = self.column;
                    self.tab_stops[column] = false;
                }
                3 => self.tab_stops.iter_mut().for_each(|stop| *stop = false),
                _ => {}
            },
            ([], 'h') | ([b'?'], 'h') | ([], 'l') | ([b'?'], 'l') => {
                for param in params {
                    let mode = param.first().copied().unwrap_or(0);
                    self.set_mode(private, mode, action == 'h');
                }
            }
            ([], 'm') => sgr::apply(&mut self.attributes, params),
            ([], 'n') => match raw(0) {
                5 => self.responses.extend_from_slice(b"\x1b[0n"),
                6 => self.report_cursor(false),
                _ => {}
            },
            ([b'?'], 'n') if raw(0) == 6 => self.report_cursor(true),
            ([b' '], 'q') => self.set_cursor_style(raw(0)),
            ([b'!'], 'p') => self.soft_reset(),
            ([], 'r') => {
                let top = first(1) as usize - 1;
                let bottom = match raw(1) {
                    0 => self.rows - 1,
                    value => (value as usize - 1).min(self.rows - 1),
                };
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            ([], 's') => self.save_cursor(),
            ([], 'u') => self.restore_cursor(),
            ([], 't') if raw(0) == 18 => {
                let reply = format!("\x1b[8;{};{}t", self.rows, self.columns);
                self.responses.extend_from_slice(reply.as_bytes());
            }
            _ => {}
        }
    }

    fn osc(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let Some(command) = params.first() else {
            return;
        };
        let argument = params
            .get(1)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
        let terminator = if bell_terminated { "\x07" } else { "\x1b\\" };
        match *command {
            b"10" | b"11" if argument.as_deref() == Some("?") => {
                let value = if *command == b"10" {
                    "rgb:ffff/ffff/ffff"
                } else {
                    "rgb:0000/0000/0000"
                };
                let number = String::from_utf8_lossy(command);
                let reply = format!("\x1b]{number};{value}{terminator}");
                self.responses.extend_from_slice(reply.as_bytes());
            }
            b"12" => {
                if let Some(color) = argument.as_deref().and_then(parse_color_spec) {
                    self.cursor_color = Some(color);
                }
            }
            b"112" => self.cursor_color = None,
            _ => {}
        }
    }

    fn esc(&mut self, intermediates: &[u8], byte: u8) {
        match (intermediates, byte) {
            ([], b'7') => self.save_cursor(),
            ([], b'8') => self.restore_cursor(),
            ([], b'D') => self.index(),
            ([], b'E') => {
                self.column = 0;
                self.pending_wrap = false;
                self.index();
            }
            ([], b'M') => self.reverse_index(),
            ([], b'H') => {
                let column = self.column;
                self.tab_stops[column] = true;
            }
            ([], b'c') => self.full_reset(),
            ([b'('], b'0') => self.charsets[0] = Charset::LineDrawing,
            ([b'('], _) => self.charsets[0] = Charset::Ascii,
            ([b')'], b'0') => self.charsets[1] = Charset::LineDrawing,
            ([b')'], _) => self.charsets[1] = Charset::Ascii,
            _ => {}
        }
    }
}

impl vte::Perform for Screen {
    fn print(&mut self, character: char) {
        self.print_character(character);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                self.column = self.column.saturating_sub(1);
                self.pending_wrap = false;
            }
            0x09 => self.tab_forward(1),
            0x0a..=0x0c => {
                self.pending_wrap = false;
                self.index();
                if self.modes.newline {
                    self.column = 0;
                }
            }
            0x0d => {
                self.column = 0;
                self.pending_wrap = false;
            }
            0x0e => self.shift = 1,
            0x0f => self.shift = 0,
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        let params: Vec<Vec<u16>> = params.iter().map(<[u16]>::to_vec).collect();
        self.csi(&params, intermediates, action);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore {
            self.esc(intermediates, byte);
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.osc(params, bell_terminated);
    }
}

impl Emulator {
    pub fn new(columns: u16, rows: u16, scrollback_max: usize) -> Emulator {
        Emulator {
            parser: vte::Parser::new(),
            screen: Screen::new(columns as usize, rows as usize, scrollback_max),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.screen, bytes);
    }

    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.screen.responses)
    }

    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.screen.resize(columns as usize, rows as usize);
    }

    pub fn columns(&self) -> u16 {
        self.screen.columns as u16
    }

    pub fn rows(&self) -> u16 {
        self.screen.rows as u16
    }

    pub fn scroll_to_top(&mut self) {
        if !self.screen.alternate_active {
            self.screen.scroll_offset = self.screen.scrollback.len();
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.screen.scroll_offset = 0;
    }

    pub fn scroll_offset(&self) -> usize {
        if self.screen.alternate_active {
            0
        } else {
            self.screen.scroll_offset
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.screen
            .visible_rows()
            .into_iter()
            .map(trimmed_line)
            .collect()
    }

    pub fn snapshot(&self) -> Map<String, Value> {
        let screen = &self.screen;
        let visible = screen.visible_rows();
        let scrollback_rows: Vec<&Row> = screen.scrollback.iter().collect();
        let cursor = json!({
            "row": screen.row,
            "column": screen.column,
            "visible": screen.modes.cursor_visible,
            "blink": screen.modes.cursor_blink,
            "shape": screen.modes.cursor_shape.name(),
            "color": screen.cursor_color.map(Color::to_json),
        });
        let mut state = Map::new();
        state.insert("columns".into(), Value::from(screen.columns));
        state.insert("rows".into(), Value::from(screen.rows));
        state.insert(
            "lines".into(),
            Value::from(
                visible
                    .iter()
                    .map(|row| trimmed_line(row))
                    .collect::<Vec<_>>(),
            ),
        );
        state.insert("cells".into(), Value::from(sparse_cells(&visible)));
        state.insert("cursor".into(), cursor);
        state.insert("scroll_offset".into(), Value::from(self.scroll_offset()));
        state.insert(
            "scrollback".into(),
            json!({
                "lines": scrollback_rows.iter().map(|row| trimmed_line(row)).collect::<Vec<_>>(),
                "cells": sparse_cells(&scrollback_rows),
            }),
        );
        state
    }
}
