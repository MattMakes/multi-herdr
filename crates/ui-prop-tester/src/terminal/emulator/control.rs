use super::{Charset, CursorShape, Modes, Screen};
use crate::terminal::cell::{Attributes, Color};
use crate::terminal::sgr;

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

impl Screen {
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
