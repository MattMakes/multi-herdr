use serde_json::{json, Value};
use ui_prop_tester::terminal::emulator::Emulator;

fn screen(columns: u16, rows: u16, bytes: &[u8]) -> Emulator {
    let mut emulator = Emulator::new(columns, rows, 50);
    emulator.feed(bytes);
    emulator
}

fn state(emulator: &Emulator) -> Value {
    Value::Object(emulator.snapshot())
}

fn cell(emulator: &Emulator, row: u64, column: u64) -> Value {
    state(emulator)["cells"]
        .as_array()
        .expect("cells")
        .iter()
        .find(|cell| cell["row"] == row && cell["column"] == column)
        .cloned()
        .unwrap_or(Value::Null)
}

fn cursor(emulator: &Emulator) -> (u64, u64) {
    let state = state(emulator);
    (
        state["cursor"]["row"].as_u64().expect("row"),
        state["cursor"]["column"].as_u64().expect("column"),
    )
}

fn scrollback_lines(emulator: &Emulator) -> Vec<String> {
    state(emulator)["scrollback"]["lines"]
        .as_array()
        .expect("scrollback lines")
        .iter()
        .map(|line| line.as_str().expect("line").to_string())
        .collect()
}

#[test]
fn plain_text_fills_lines_with_trailing_blanks_trimmed() {
    let emulator = screen(10, 3, b"ab  \r\ncd");
    assert_eq!(emulator.lines(), ["ab", "cd", ""]);
    assert_eq!(cursor(&emulator), (1, 2));
    assert_eq!(
        state(&emulator)["cells"].as_array().expect("cells").len(),
        4
    );
}

#[test]
fn colons_and_semicolons_both_select_extended_colours() {
    let emulator = screen(
        10,
        1,
        b"\x1b[38:2::10:20:30ma\x1b[38:2:1:2:3mb\x1b[48:5:9mc\x1b[58:5:4md\x1b[38;5;300me",
    );
    assert_eq!(cell(&emulator, 0, 0)["fg"], json!({"rgb": [10, 20, 30]}));
    assert_eq!(cell(&emulator, 0, 1)["fg"], json!({"rgb": [1, 2, 3]}));
    assert_eq!(cell(&emulator, 0, 2)["bg"], json!({"indexed": 9}));
    assert_eq!(
        cell(&emulator, 0, 3)["underline_color"],
        json!({"indexed": 4})
    );
    assert_eq!(cell(&emulator, 0, 4)["fg"], json!({"indexed": 255}));
}

#[test]
fn attribute_resets_clear_only_their_own_attribute() {
    let emulator = screen(
        20,
        1,
        b"\x1b[1;2;3;4;5;7;8;9;53ma\x1b[22mb\x1b[23mc\x1b[24md\x1b[25me\x1b[27mf\x1b[28mg\x1b[29mh\x1b[55mi\x1b[39;49mj",
    );
    assert_eq!(
        cell(&emulator, 0, 0),
        json!({"row": 0, "column": 0, "text": "a", "bold": true, "dim": true, "italic": true,
               "underline": "single", "blink": true, "inverse": true, "invisible": true,
               "strikethrough": true, "overline": true})
    );
    let b = cell(&emulator, 0, 1);
    assert_eq!((b.get("bold"), b.get("dim")), (None, None));
    assert_eq!(cell(&emulator, 0, 2).get("italic"), None);
    assert_eq!(cell(&emulator, 0, 3).get("underline"), None);
    assert_eq!(cell(&emulator, 0, 4).get("blink"), None);
    assert_eq!(cell(&emulator, 0, 5).get("inverse"), None);
    assert_eq!(cell(&emulator, 0, 6).get("invisible"), None);
    assert_eq!(cell(&emulator, 0, 7).get("strikethrough"), None);
    assert_eq!(
        cell(&emulator, 0, 8),
        json!({"row": 0, "column": 8, "text": "i"})
    );
}

#[test]
fn text_wraps_at_the_right_margin_only_when_the_next_character_arrives() {
    let mut emulator = screen(4, 2, b"abcd");
    assert_eq!(cursor(&emulator), (0, 3));
    assert_eq!(emulator.lines(), ["abcd", ""]);
    emulator.feed(b"e");
    assert_eq!(emulator.lines(), ["abcd", "e"]);
}

#[test]
fn autowrap_off_overwrites_the_last_column() {
    let emulator = screen(4, 2, b"\x1b[?7labcdef");
    assert_eq!(emulator.lines(), ["abcf", ""]);
}

#[test]
fn a_wide_character_that_does_not_fit_moves_to_the_next_line() {
    let emulator = screen(5, 2, "abcd漢".as_bytes());
    assert_eq!(emulator.lines(), ["abcd", "漢"]);
    assert_eq!(cell(&emulator, 1, 1)["wide_continuation"], true);
}

#[test]
fn overwriting_half_of_a_wide_character_blanks_the_other_half() {
    let emulator = screen(6, 1, "漢字\x1b[1;2Hx\x1b[1;3Hy".as_bytes());
    assert_eq!(emulator.lines(), [" xy"]);
    assert_eq!(cell(&emulator, 0, 3), Value::Null);
}

#[test]
fn combining_marks_join_the_previous_cell() {
    let emulator = screen(6, 1, "e\u{301}x".as_bytes());
    assert_eq!(cell(&emulator, 0, 0)["text"], "e\u{301}");
    assert_eq!(cell(&emulator, 0, 1)["text"], "x");
}

#[test]
fn malformed_utf8_becomes_replacement_characters() {
    let emulator = screen(10, 1, b"a\xffb\xe2\x82c\xc3");
    assert_eq!(emulator.lines(), ["a\u{FFFD}b\u{FFFD}c"]);
}

#[test]
fn cursor_movement_sequences_move_the_cursor() {
    let mut emulator = screen(20, 10, b"\x1b[5;10H");
    assert_eq!(cursor(&emulator), (4, 9));
    emulator.feed(b"\x1b[2A");
    assert_eq!(cursor(&emulator), (2, 9));
    emulator.feed(b"\x1b[3B");
    assert_eq!(cursor(&emulator), (5, 9));
    emulator.feed(b"\x1b[4C");
    assert_eq!(cursor(&emulator), (5, 13));
    emulator.feed(b"\x1b[20D");
    assert_eq!(cursor(&emulator), (5, 0));
    emulator.feed(b"\x1b[7G");
    assert_eq!(cursor(&emulator), (5, 6));
    emulator.feed(b"\x1b[2d");
    assert_eq!(cursor(&emulator), (1, 6));
    emulator.feed(b"\x1b[2E");
    assert_eq!(cursor(&emulator), (3, 0));
    emulator.feed(b"\x1b[F");
    assert_eq!(cursor(&emulator), (2, 0));
    emulator.feed(b"\x1b[99;99H");
    assert_eq!(cursor(&emulator), (9, 19));
    emulator.feed(b"\x1b[H");
    assert_eq!(cursor(&emulator), (0, 0));
}

#[test]
fn save_and_restore_keep_position_and_attributes() {
    let emulator = screen(
        10,
        3,
        b"\x1b[2;3H\x1b[1m\x1b7\x1b[0m\x1b[H\x1b8x\x1b[s\x1b[3;1H\x1b[uy",
    );
    assert_eq!(cell(&emulator, 1, 2)["bold"], true);
    assert_eq!(cell(&emulator, 1, 3)["bold"], true);
    assert_eq!(emulator.lines(), ["", "  xy", ""]);
}

#[test]
fn erase_in_display_and_line_clear_the_right_cells() {
    let mut emulator = screen(5, 3, b"aaaaa\r\nbbbbb\r\nccccc\x1b[2;3H\x1b[K");
    assert_eq!(emulator.lines(), ["aaaaa", "bb", "ccccc"]);
    emulator.feed(b"\x1b[1K");
    assert_eq!(emulator.lines(), ["aaaaa", "", "ccccc"]);
    emulator.feed(b"\x1b[J");
    assert_eq!(emulator.lines(), ["aaaaa", "", ""]);
    emulator.feed(b"\x1b[1;3H\x1b[1J");
    assert_eq!(emulator.lines(), ["   aa", "", ""]);
    emulator.feed(b"\x1b[2J");
    assert_eq!(emulator.lines(), ["", "", ""]);
}

#[test]
fn erasing_uses_the_current_background() {
    let emulator = screen(3, 1, b"\x1b[44m\x1b[2K");
    assert_eq!(
        cell(&emulator, 0, 2),
        json!({"row": 0, "column": 2, "text": " ", "bg": {"indexed": 4}})
    );
}

#[test]
fn insert_delete_and_erase_characters_shift_the_line() {
    let mut emulator = screen(6, 1, b"abcdef\x1b[1;2H\x1b[2@");
    assert_eq!(emulator.lines(), ["a  bcd"]);
    emulator.feed(b"\x1b[3P");
    assert_eq!(emulator.lines(), ["acd"]);
    emulator.feed(b"\x1b[1;1H\x1b[2X");
    assert_eq!(emulator.lines(), ["  d"]);
}

#[test]
fn insert_mode_pushes_text_right() {
    let emulator = screen(6, 1, b"abc\x1b[1;2H\x1b[4hXY\x1b[4lZ");
    assert_eq!(emulator.lines(), ["aXYZc"]);
}

#[test]
fn insert_and_delete_lines_work_inside_the_scroll_region() {
    let mut emulator = screen(3, 4, b"1\r\n2\r\n3\r\n4\x1b[2;1H\x1b[L");
    assert_eq!(emulator.lines(), ["1", "", "2", "3"]);
    emulator.feed(b"\x1b[2M");
    assert_eq!(emulator.lines(), ["1", "3", "", ""]);
}

#[test]
fn a_scroll_region_scrolls_only_its_rows() {
    let emulator = screen(3, 4, b"top\r\n1\r\n2\r\nbot\x1b[2;3r\x1b[3;1H\nx");
    assert_eq!(emulator.lines(), ["top", "2", "x", "bot"]);
    assert!(scrollback_lines(&emulator).is_empty());
}

#[test]
fn reverse_index_at_the_top_scrolls_down() {
    let emulator = screen(3, 3, b"a\r\nb\r\nc\x1b[H\x1bM");
    assert_eq!(emulator.lines(), ["", "a", "b"]);
}

#[test]
fn scroll_up_and_down_sequences_move_the_region() {
    let mut emulator = screen(3, 3, b"a\r\nb\r\nc\x1b[S");
    assert_eq!(emulator.lines(), ["b", "c", ""]);
    assert_eq!(scrollback_lines(&emulator), ["a"]);
    emulator.feed(b"\x1b[2T");
    assert_eq!(emulator.lines(), ["", "", "b"]);
}

#[test]
fn tab_stops_default_to_every_eight_columns_and_can_change() {
    let mut emulator = screen(30, 1, b"a\tb");
    assert_eq!(cell(&emulator, 0, 8)["text"], "b");
    emulator.feed(b"\r\x1b[3g\x1b[1;5H\x1bH\r\tc");
    assert_eq!(cell(&emulator, 0, 4)["text"], "c");
    emulator.feed(b"\x1b[1;20H\x1b[Zd");
    assert_eq!(cell(&emulator, 0, 4)["text"], "d");
}

#[test]
fn the_alternate_screen_keeps_the_primary_screen() {
    let mut emulator = screen(5, 2, b"main\x1b[?1049h\x1b[HALT");
    assert_eq!(emulator.lines(), ["ALT", ""]);
    emulator.feed(b"\x1b[?1049l");
    assert_eq!(emulator.lines(), ["main", ""]);
    assert_eq!(cursor(&emulator), (0, 4));
}

#[test]
fn the_alternate_screen_adds_nothing_to_scrollback() {
    let emulator = screen(5, 2, b"\x1b[?1049h1\r\n2\r\n3\r\n4");
    assert!(scrollback_lines(&emulator).is_empty());
}

#[test]
fn cursor_style_and_visibility_sequences_change_the_cursor() {
    let mut emulator = screen(5, 2, b"\x1b[?25l\x1b[4 q");
    let cursor = state(&emulator)["cursor"].clone();
    assert_eq!(cursor["visible"], false);
    assert_eq!(cursor["shape"], "underline");
    assert_eq!(cursor["blink"], false);
    emulator.feed(b"\x1b[?25h\x1b[5 q\x1b]12;rgb:ff/80/00\x1b\\");
    let cursor = state(&emulator)["cursor"].clone();
    assert_eq!(cursor["visible"], true);
    assert_eq!(cursor["shape"], "bar");
    assert_eq!(cursor["blink"], true);
    assert_eq!(cursor["color"], json!({"rgb": [255, 128, 0]}));
    emulator.feed(b"\x1b]112\x07\x1b[0 q");
    let cursor = state(&emulator)["cursor"].clone();
    assert_eq!(cursor["color"], Value::Null);
    assert_eq!(cursor["shape"], "block");
}

#[test]
fn queries_produce_replies_for_the_program() {
    let mut emulator = screen(20, 10, b"\x1b[3;4H\x1b[6n\x1b[5n\x1b[c\x1b[>c\x1b[18t");
    let replies = String::from_utf8(emulator.take_responses()).expect("utf8");
    assert_eq!(
        replies,
        "\x1b[3;4R\x1b[0n\x1b[?62;22c\x1b[>1;10;0c\x1b[8;10;20t"
    );
    assert!(emulator.take_responses().is_empty());
    emulator.feed(b"\x1b]11;?\x07");
    assert_eq!(
        emulator.take_responses(),
        b"\x1b]11;rgb:0000/0000/0000\x07".to_vec()
    );
}

#[test]
fn origin_mode_makes_positions_relative_to_the_region() {
    let mut emulator = screen(10, 6, b"\x1b[2;5r\x1b[?6h\x1b[2;3H");
    assert_eq!(cursor(&emulator), (2, 2));
    emulator.feed(b"\x1b[6n");
    assert_eq!(emulator.take_responses(), b"\x1b[2;3R".to_vec());
    emulator.feed(b"\x1b[99;1H");
    assert_eq!(cursor(&emulator), (4, 0));
}

#[test]
fn line_drawing_characters_are_translated() {
    let emulator = screen(10, 1, b"\x1b(0lqk\x1b(Bq");
    assert_eq!(emulator.lines(), ["┌─┐q"]);
}

#[test]
fn repeat_prints_the_last_character_again() {
    let emulator = screen(10, 1, b"a\x1b[3b");
    assert_eq!(emulator.lines(), ["aaaa"]);
}

#[test]
fn scrollback_keeps_old_lines_up_to_the_limit() {
    let mut emulator = Emulator::new(5, 2, 3);
    emulator.feed(b"1\r\n2\r\n3\r\n4\r\n5\r\n6");
    assert_eq!(scrollback_lines(&emulator), ["2", "3", "4"]);
    assert_eq!(emulator.lines(), ["5", "6"]);
}

#[test]
fn a_zero_scrollback_limit_keeps_nothing() {
    let mut emulator = Emulator::new(5, 2, 0);
    emulator.feed(b"1\r\n2\r\n3");
    assert!(scrollback_lines(&emulator).is_empty());
    emulator.scroll_to_top();
    assert_eq!(emulator.scroll_offset(), 0);
}

#[test]
fn the_viewport_scrolls_into_scrollback_and_back() {
    let mut emulator = screen(5, 3, b"1\r\n2\r\n3\r\n4\r\n5");
    emulator.scroll_to_top();
    assert_eq!(emulator.scroll_offset(), 2);
    assert_eq!(emulator.lines(), ["1", "2", "3"]);
    emulator.feed(b"\r\n6");
    assert_eq!(emulator.scroll_offset(), 3);
    assert_eq!(emulator.lines(), ["1", "2", "3"]);
    emulator.scroll_to_bottom();
    assert_eq!(emulator.lines(), ["4", "5", "6"]);
}

#[test]
fn the_viewport_shows_part_scrollback_part_screen_when_scrollback_is_short() {
    let mut emulator = screen(5, 3, b"1\r\n2\r\n3\r\n4");
    emulator.scroll_to_top();
    assert_eq!(emulator.lines(), ["1", "2", "3"]);
    assert_eq!(state(&emulator)["scroll_offset"], 1);
    assert_eq!(cell(&emulator, 0, 0)["text"], "1");
}

#[test]
fn erasing_scrollback_resets_the_viewport() {
    let mut emulator = screen(5, 2, b"1\r\n2\r\n3");
    emulator.scroll_to_top();
    emulator.feed(b"\x1b[3J");
    assert!(scrollback_lines(&emulator).is_empty());
    assert_eq!(emulator.scroll_offset(), 0);
}

#[test]
fn growing_adds_blank_rows_and_columns() {
    let mut emulator = screen(3, 2, b"abc\r\nde");
    emulator.resize(5, 4);
    assert_eq!(emulator.lines(), ["abc", "de", "", ""]);
    assert_eq!((emulator.columns(), emulator.rows()), (5, 4));
    emulator.feed(b"fgh");
    assert_eq!(emulator.lines(), ["abc", "defgh", "", ""]);
}

#[test]
fn shrinking_rows_keeps_the_cursor_line_and_moves_old_lines_to_scrollback() {
    let mut emulator = screen(5, 4, b"1\r\n2\r\n3\r\n4");
    emulator.resize(5, 2);
    assert_eq!(emulator.lines(), ["3", "4"]);
    assert_eq!(scrollback_lines(&emulator), ["1", "2"]);
    assert_eq!(cursor(&emulator), (1, 1));
}

#[test]
fn shrinking_rows_drops_blank_rows_below_the_cursor_first() {
    let mut emulator = screen(5, 4, b"1\r\n2");
    emulator.resize(5, 2);
    assert_eq!(emulator.lines(), ["1", "2"]);
    assert!(scrollback_lines(&emulator).is_empty());
}

#[test]
fn shrinking_columns_cuts_lines_and_split_wide_characters() {
    let mut emulator = screen(6, 1, "ab漢字".as_bytes());
    emulator.resize(5, 1);
    assert_eq!(emulator.lines(), ["ab漢"]);
    emulator.resize(3, 1);
    assert_eq!(emulator.lines(), ["ab"]);
    assert_eq!(cursor(&emulator), (0, 2));
}

#[test]
fn full_reset_clears_the_screen_and_modes() {
    let emulator = screen(5, 2, b"\x1b[1mab\x1b[?25l\x1bcc");
    assert_eq!(emulator.lines(), ["c", ""]);
    assert_eq!(state(&emulator)["cursor"]["visible"], true);
    assert_eq!(
        cell(&emulator, 0, 0),
        json!({"row": 0, "column": 0, "text": "c"})
    );
}

#[test]
fn backspace_and_carriage_return_move_left() {
    let emulator = screen(5, 1, b"abc\x08\x08x\rY");
    assert_eq!(emulator.lines(), ["Yxc"]);
}

#[test]
fn linefeed_mode_returns_the_carriage() {
    let emulator = screen(5, 2, b"\x1b[20hab\ncd");
    assert_eq!(emulator.lines(), ["ab", "cd"]);
}
