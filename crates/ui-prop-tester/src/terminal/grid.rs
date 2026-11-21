use unicode_width::UnicodeWidthChar;

use super::cell::{Attributes, Cell};

pub type Row = Vec<Cell>;

#[derive(Debug, Clone)]
pub struct Grid {
    columns: usize,
    rows: Vec<Row>,
}

pub fn blank_row(columns: usize, attributes: Attributes) -> Row {
    vec![Cell::blank(attributes); columns]
}

fn display_width(text: &str) -> usize {
    text.chars()
        .next()
        .and_then(UnicodeWidthChar::width)
        .unwrap_or(1)
}

pub fn repair_row(row: &mut Row) {
    let length = row.len();
    for column in 0..length {
        let cell = &row[column];
        if cell.wide_continuation {
            let has_head = column > 0
                && !row[column - 1].wide_continuation
                && display_width(&row[column - 1].text) > 1;
            if !has_head {
                let attributes = row[column].attributes;
                row[column] = Cell::blank(attributes);
            }
        } else if display_width(&cell.text) > 1 {
            let has_continuation = column + 1 < length && row[column + 1].wide_continuation;
            if !has_continuation {
                let attributes = row[column].attributes;
                row[column] = Cell::blank(attributes);
            }
        }
    }
}

impl Grid {
    pub fn new(columns: usize, rows: usize) -> Grid {
        Grid {
            columns,
            rows: (0..rows)
                .map(|_| blank_row(columns, Attributes::default()))
                .collect(),
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn row(&self, row: usize) -> &Row {
        &self.rows[row]
    }

    pub fn row_mut(&mut self, row: usize) -> &mut Row {
        &mut self.rows[row]
    }

    pub fn scroll_up(
        &mut self,
        top: usize,
        bottom: usize,
        count: usize,
        fill: Attributes,
    ) -> Vec<Row> {
        let span = bottom + 1 - top;
        let count = count.min(span);
        let removed: Vec<Row> = self.rows.drain(top..top + count).collect();
        for _ in 0..count {
            self.rows
                .insert(bottom + 1 - count, blank_row(self.columns, fill));
        }
        removed
    }

    pub fn scroll_down(&mut self, top: usize, bottom: usize, count: usize, fill: Attributes) {
        let span = bottom + 1 - top;
        let count = count.min(span);
        self.rows.drain(bottom + 1 - count..=bottom);
        for _ in 0..count {
            self.rows.insert(top, blank_row(self.columns, fill));
        }
    }

    pub fn erase(&mut self, row: usize, start: usize, end: usize, fill: Attributes) {
        let end = end.min(self.columns);
        if start >= end {
            return;
        }
        let cells = &mut self.rows[row];
        for cell in &mut cells[start..end] {
            *cell = Cell::blank(fill);
        }
        repair_row(cells);
    }

    pub fn erase_rows(&mut self, start: usize, end: usize, fill: Attributes) {
        for row in start..end.min(self.rows.len()) {
            self.rows[row] = blank_row(self.columns, fill);
        }
    }

    pub fn insert_cells(&mut self, row: usize, column: usize, count: usize, fill: Attributes) {
        let columns = self.columns;
        let cells = &mut self.rows[row];
        let count = count.min(columns - column);
        for _ in 0..count {
            cells.insert(column, Cell::blank(fill));
        }
        cells.truncate(columns);
        repair_row(cells);
    }

    pub fn delete_cells(&mut self, row: usize, column: usize, count: usize, fill: Attributes) {
        let columns = self.columns;
        let cells = &mut self.rows[row];
        let count = count.min(columns - column);
        cells.drain(column..column + count);
        cells.resize(columns, Cell::blank(fill));
        repair_row(cells);
    }

    pub fn resize_columns(&mut self, columns: usize) {
        self.columns = columns;
        for row in &mut self.rows {
            row.resize(columns, Cell::default());
            repair_row(row);
        }
    }

    pub fn remove_top_rows(&mut self, count: usize) -> Vec<Row> {
        self.rows.drain(..count).collect()
    }

    pub fn remove_bottom_rows(&mut self, count: usize) {
        let length = self.rows.len();
        self.rows.truncate(length - count);
    }

    pub fn add_bottom_rows(&mut self, count: usize) {
        for _ in 0..count {
            self.rows
                .push(blank_row(self.columns, Attributes::default()));
        }
    }

    pub fn clear(&mut self, fill: Attributes) {
        for row in &mut self.rows {
            *row = blank_row(self.columns, fill);
        }
    }
}
