use std::collections::VecDeque;

use serde_json::{json, Map, Value};
use unicode_width::UnicodeWidthChar;

mod control;

use super::cell::{Attributes, Cell, Color};
use super::grid::{Grid, Row};

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
