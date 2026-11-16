#![cfg(unix)]

mod terminal_support;

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use terminal_support::{
    cell, lines, pid_after, process_exists, shell_config, wait_for_process_gone, Session,
};
use ui_prop_tester::{DriverError, DriverKind};

fn finished(script: &str) -> Value {
    let mut session = Session::shell(script);
    session.until_ended();
    let state = session.state().clone();
    session.shutdown();
    state
}

#[test]
fn the_child_sees_the_configured_terminal_size() {
    let mut config = shell_config("stty size");
    config.columns = 37;
    config.rows = 13;
    let mut session = Session::start(&config);
    session.until_ended();
    assert_eq!(lines(session.state())[0], "13 37");
    assert_eq!(session.state()["columns"], 37);
    assert_eq!(session.state()["rows"], 13);
    assert_eq!(session.driver.kind(), DriverKind::Terminal);
    session.shutdown();
}

#[test]
fn the_child_runs_with_xterm_256color() {
    let state = finished("printf '%s' \"$TERM\"");
    assert_eq!(lines(&state)[0], "xterm-256color");
}

#[test]
fn indexed_and_rgb_colours_reach_the_cells() {
    let state = finished(
        r"printf '\033[31mA\033[38;5;200mB\033[38;2;1;2;3mC\033[48;5;7mD\033[0;44mE\033[0;97mF\033[0;105mG\033[0m'",
    );
    assert_eq!(cell(&state, 0, 0)["fg"], json!({"indexed": 1}));
    assert_eq!(cell(&state, 0, 1)["fg"], json!({"indexed": 200}));
    assert_eq!(cell(&state, 0, 2)["fg"], json!({"rgb": [1, 2, 3]}));
    assert_eq!(cell(&state, 0, 3)["bg"], json!({"indexed": 7}));
    assert_eq!(cell(&state, 0, 3)["fg"], json!({"rgb": [1, 2, 3]}));
    assert_eq!(cell(&state, 0, 4)["bg"], json!({"indexed": 4}));
    assert_eq!(cell(&state, 0, 4).get("fg"), None);
    assert_eq!(cell(&state, 0, 5)["fg"], json!({"indexed": 15}));
    assert_eq!(cell(&state, 0, 6)["bg"], json!({"indexed": 13}));
    assert_eq!(lines(&state)[0], "ABCDEFG");
}

#[test]
fn every_text_attribute_reaches_its_cell() {
    let state = finished(
        r"printf '\033[1ma\033[0m\033[3mb\033[0m\033[5mc\033[0m\033[7md\033[0m\033[9me\033[0m\033[2mf\033[0m\033[8mg\033[0m\033[53mh\033[0mi'",
    );
    let flags = [
        "bold",
        "italic",
        "blink",
        "inverse",
        "strikethrough",
        "dim",
        "invisible",
        "overline",
    ];
    for (column, flag) in flags.iter().enumerate() {
        let found = cell(&state, 0, column as u64);
        assert_eq!(found[*flag], true, "{flag} at column {column}: {found}");
        let others: Vec<&&str> = flags.iter().filter(|other| *other != flag).collect();
        for other in others {
            assert_eq!(found.get(*other), None, "{other} leaked onto {flag}");
        }
    }
    assert_eq!(
        cell(&state, 0, 8),
        json!({"row": 0, "column": 8, "text": "i"})
    );
}

#[test]
fn underline_styles_and_underline_colour_reach_the_cells() {
    let state = finished(
        r"printf '\033[4ma\033[4:2mb\033[4:3mc\033[4:4md\033[4:5me\033[24;21mf\033[4;58;2;9;8;7mg\033[59mh\033[24mi'",
    );
    let styles = ["single", "double", "curly", "dotted", "dashed", "double"];
    for (column, style) in styles.iter().enumerate() {
        assert_eq!(cell(&state, 0, column as u64)["underline"], *style);
    }
    assert_eq!(
        cell(&state, 0, 6)["underline_color"],
        json!({"rgb": [9, 8, 7]})
    );
    assert_eq!(cell(&state, 0, 7).get("underline_color"), None);
    assert_eq!(cell(&state, 0, 8).get("underline"), None);
}

#[test]
fn wide_characters_take_a_continuation_cell() {
    let state = finished("printf '漢字x🙂'");
    assert_eq!(cell(&state, 0, 0)["text"], "漢");
    assert_eq!(
        cell(&state, 0, 1),
        json!({"row": 0, "column": 1, "text": "", "wide_continuation": true})
    );
    assert_eq!(cell(&state, 0, 2)["text"], "字");
    assert_eq!(cell(&state, 0, 3)["wide_continuation"], true);
    assert_eq!(cell(&state, 0, 4)["text"], "x");
    assert_eq!(cell(&state, 0, 5)["text"], "🙂");
    assert_eq!(cell(&state, 0, 6)["wide_continuation"], true);
    assert_eq!(lines(&state)[0], "漢字x🙂");
}

#[test]
fn cursor_position_visibility_shape_and_colour_are_reported() {
    let mut session = Session::shell(
        r"printf '\033[3;5H\033[?25l\033[6 q\033]12;#ff0010\007'; echo; printf 'ready\033[3;5H'; exec sleep 600",
    );
    session.until(|observation| lines(&observation.state).iter().any(|line| line == "ready"));
    let cursor = session.state()["cursor"].clone();
    assert_eq!(
        cursor,
        json!({"row": 2, "column": 4, "visible": false, "blink": false, "shape": "bar", "color": {"rgb": [255, 0, 16]}})
    );
    session.shutdown();
}

#[test]
fn the_default_cursor_is_a_visible_blinking_block() {
    let state = finished("printf ab");
    assert_eq!(
        state["cursor"],
        json!({"row": 0, "column": 2, "visible": true, "blink": true, "shape": "block", "color": null})
    );
}

#[test]
fn malformed_utf8_shows_the_replacement_character() {
    let state = finished(r"printf 'a\377b\342\202c'");
    assert_eq!(lines(&state)[0], "a\u{FFFD}b\u{FFFD}c");
    assert_eq!(cell(&state, 0, 1)["text"], "\u{FFFD}");
}

#[test]
fn scrollback_keeps_lines_and_the_viewport_jumps() {
    let mut session = Session::shell("i=1; while [ $i -le 30 ]; do echo line$i; i=$((i+1)); done");
    session.until_ended();
    let state = session.state().clone();
    let scrollback: Vec<&str> = state["scrollback"]["lines"]
        .as_array()
        .expect("scrollback lines")
        .iter()
        .map(|line| line.as_str().expect("line"))
        .collect();
    assert_eq!(scrollback.len(), 21);
    assert_eq!(scrollback[0], "line1");
    assert_eq!(scrollback[20], "line21");
    assert_eq!(lines(&state)[0], "line22");
    assert_eq!(state["scroll_offset"], 0);

    let top = session
        .apply(json!({"kind": "scroll_to_top"}))
        .state
        .clone();
    assert_eq!(top["scroll_offset"], 21);
    assert_eq!(lines(&top)[0], "line1");
    assert_eq!(lines(&top)[9], "line10");
    assert_eq!(top["last_action"], json!({"kind": "scroll_to_top"}));

    let bottom = session
        .apply(json!({"kind": "scroll_to_bottom"}))
        .state
        .clone();
    assert_eq!(bottom["scroll_offset"], 0);
    assert_eq!(lines(&bottom)[0], "line22");
    session.shutdown();
}

#[test]
fn scrollback_is_capped_at_the_configured_line_count() {
    let mut config = shell_config("i=1; while [ $i -le 30 ]; do echo line$i; i=$((i+1)); done");
    config.scrollback_lines_max = 5;
    let mut session = Session::start(&config);
    session.until_ended();
    let scrollback = session.state()["scrollback"]["lines"].clone();
    assert_eq!(
        scrollback,
        json!(["line17", "line18", "line19", "line20", "line21"])
    );
    session.shutdown();
}

#[test]
fn scrollback_cells_carry_their_attributes() {
    let state = finished(
        r"printf '\033[1mbold\033[0m\n'; i=1; while [ $i -le 12 ]; do echo $i; i=$((i+1)); done",
    );
    let first = state["scrollback"]["cells"]
        .as_array()
        .expect("scrollback cells")
        .iter()
        .find(|cell| cell["row"] == 0 && cell["column"] == 0)
        .cloned()
        .expect("first scrollback cell");
    assert_eq!(
        first,
        json!({"row": 0, "column": 0, "text": "b", "bold": true})
    );
}

#[test]
fn a_click_sends_sgr_press_and_release_at_one_based_coordinates() {
    let mut session = Session::shell(
        r"stty -echo -icanon; echo ready; head -c 18 | tr '\033' E; echo; exec sleep 600",
    );
    session.until_line("ready");
    session.apply(json!({"kind": "click", "row": 1, "column": 2}));
    session.until_line("E[<0;3;2ME[<0;3;2m");
    assert_eq!(
        session.state()["last_action"],
        json!({"kind": "click", "row": 1, "column": 2})
    );
    session.shutdown();
}

#[test]
fn resize_changes_the_emulator_and_the_child_sees_it() {
    let mut session =
        Session::shell("trap 'stty size' WINCH; echo ready; while :; do sleep 1 & wait $!; done");
    session.until_line("ready");
    let resized = session
        .apply(json!({"kind": "resize", "columns": 50, "rows": 12}))
        .state
        .clone();
    assert_eq!(resized["columns"], 50);
    assert_eq!(resized["rows"], 12);
    assert_eq!(lines(&resized).len(), 12);
    session.until_line("12 50");
    session.shutdown();
}

#[test]
fn written_text_reaches_the_child() {
    let mut session = Session::shell("IFS= read -r line; echo \"got:$line\"");
    session.write("hello wörld\r");
    session.until_ended();
    assert!(lines(session.state()).contains(&"got:hello wörld".to_string()));
    session.shutdown();
}

#[test]
fn written_bytes_reach_the_child() {
    let mut session = Session::shell("IFS= read -r line; echo \"got:$line\"");
    session.apply(json!({"kind": "write", "bytes": [104, 105, 13]}));
    session.until_ended();
    assert!(lines(session.state()).contains(&"got:hi".to_string()));
    session.shutdown();
}

#[test]
fn a_state_waits_for_output_to_be_quiet() {
    let mut config = shell_config("printf a; sleep 0.1; printf b; exec sleep 600");
    config.quiescence_timeout_ms = 400;
    let mut session = Session::start(&config);
    session.until(|observation| !lines(&observation.state)[0].is_empty());
    assert_eq!(lines(session.state())[0], "ab");
    session.shutdown();
}

#[test]
fn a_short_quiet_window_captures_before_a_later_burst() {
    let mut session = Session::shell("printf a; sleep 0.3; printf b; exec sleep 600");
    assert!(!lines(session.state())[0].contains('b'));
    session.until(|observation| lines(&observation.state)[0] == "ab");
    session.shutdown();
}

#[test]
fn timestamps_count_from_start_and_never_go_back() {
    let mut session = Session::shell("exec sleep 600");
    let first = session.last.timestamp_ms;
    let second = session
        .apply(json!({"kind": "scroll_to_bottom"}))
        .timestamp_ms;
    assert!(first < 10_000);
    assert!(second >= first);
    session.shutdown();
}

#[test]
fn draining_a_program_that_prints_forever_stops_after_one_second() {
    let mut config = shell_config("while :; do echo x; done");
    config.quiescence_timeout_ms = 200;
    let began = Instant::now();
    let mut driver = ui_prop_tester::terminal::open(&config).expect("open");
    let observation = driver.start().expect("start");
    let elapsed = began.elapsed();
    assert!(!observation.ended);
    assert!(elapsed >= Duration::from_millis(1000), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    let applied = Instant::now();
    driver
        .apply(&json!({"kind": "scroll_to_bottom"}))
        .expect("apply");
    assert!(applied.elapsed() < Duration::from_secs(5));
    driver.shutdown().expect("shutdown");
}

#[test]
fn a_normal_exit_reports_its_code_and_ends() {
    let mut session = Session::shell("echo bye; exit 3");
    let observation = session.until_ended().clone();
    assert_eq!(
        observation.state["exit"],
        json!({"code": 3, "signal": null})
    );
    assert_eq!(lines(&observation.state)[0], "bye");
    session.shutdown();
}

#[test]
fn a_running_child_has_no_exit_and_has_not_ended() {
    let mut session = Session::shell("echo up; exec sleep 600");
    session.until_line("up");
    assert_eq!(session.state()["exit"], Value::Null);
    assert!(!session.last.ended);
    session.shutdown();
}

#[test]
fn death_by_signal_reports_the_signal_and_ends() {
    let mut session = Session::shell("kill -TERM $$");
    let observation = session.until_ended().clone();
    assert_eq!(
        observation.state["exit"],
        json!({"code": null, "signal": 15})
    );
    session.shutdown();
}

#[test]
fn shutdown_terminates_the_child_and_its_process_group() {
    let mut session = Session::shell("sleep 600 & echo child:$$; echo grandchild:$!; wait");
    session.until(|observation| pid_after(&observation.state, "grandchild:").is_some());
    let child = pid_after(session.state(), "child:").expect("child pid");
    let grandchild = pid_after(session.state(), "grandchild:").expect("grandchild pid");
    assert!(process_exists(child));
    session.shutdown();
    assert!(!process_exists(child));
    assert!(wait_for_process_gone(grandchild));
}

#[test]
fn dropping_the_driver_terminates_the_child() {
    let mut session = Session::shell("echo child:$$; exec sleep 600");
    session.until(|observation| pid_after(&observation.state, "child:").is_some());
    let child = pid_after(session.state(), "child:").expect("child pid");
    drop(session);
    assert!(!process_exists(child));
}

#[test]
fn a_child_that_ignores_hangup_and_terminate_is_killed() {
    let mut session = Session::shell("trap '' HUP TERM; echo child:$$; while :; do sleep 1; done");
    session.until(|observation| pid_after(&observation.state, "child:").is_some());
    let child = pid_after(session.state(), "child:").expect("child pid");
    session.shutdown();
    assert!(!process_exists(child));
}

#[test]
fn input_dropped_by_a_child_that_never_reads_is_a_stall() {
    let mut session = Session::shell("stty raw -echo; echo ready; exec sleep 600");
    session.until_line("ready");
    let flood = "x".repeat(256 * 1024);
    let began = Instant::now();
    let outcome = session.try_apply(json!({"kind": "write", "text": flood}));
    match outcome {
        Err(DriverError::Stalled(message)) => assert!(message.contains("/bin/sh"), "{message}"),
        other => panic!("expected a stall, got {other:?}"),
    }
    assert!(began.elapsed() > Duration::from_secs(5));
    session.shutdown();
}

#[test]
fn an_unknown_action_kind_fails() {
    let mut session = Session::shell("exec sleep 600");
    match session.try_apply(json!({"kind": "dance"})) {
        Err(DriverError::Failed(message)) => assert!(message.contains("dance"), "{message}"),
        other => panic!("expected a failure, got {other:?}"),
    }
    session.shutdown();
}

#[test]
fn a_missing_program_fails_at_start() {
    let mut config = shell_config("true");
    config.program = "/nonexistent/ui-prop-tester-program".into();
    let mut driver = ui_prop_tester::terminal::open(&config).expect("open");
    match driver.start() {
        Err(DriverError::Setup(message)) => {
            assert!(
                message.contains("/nonexistent/ui-prop-tester-program"),
                "{message}"
            )
        }
        other => panic!("expected a setup error, got {:?}", other.map(|o| o.state)),
    }
}

#[test]
fn the_state_has_every_contract_field() {
    let state = finished("printf hi");
    let mut keys: Vec<&String> = state.as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "cells",
            "columns",
            "cursor",
            "exit",
            "last_action",
            "lines",
            "rows",
            "scroll_offset",
            "scrollback"
        ]
    );
    assert_eq!(lines(&state).len(), 10);
}

#[test]
fn the_child_answers_to_a_cursor_position_query() {
    let mut session = Session::shell(
        r"stty raw -echo; printf '\033[4;7H\033[6n'; head -c 6 | tr '\033' E > /dev/null; printf '\033[1;1Hgot-reply\r\n'; exec sleep 600",
    );
    session.until_line("got-reply");
    session.shutdown();
}
