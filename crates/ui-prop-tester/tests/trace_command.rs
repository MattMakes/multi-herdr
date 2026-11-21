#[path = "trace_fixtures.rs"]
mod trace_fixtures;

use std::fs;

use serde_json::Value;
use trace_fixtures::{browser_trace, path_text, run_cli, terminal_trace, write_trace};

#[test]
fn the_default_output_has_a_header_and_one_block_per_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let result = run_cli(&["trace", path_text(&path)]);
    assert_eq!(result.code, 0, "{}", result.err);
    assert!(result.err.is_empty(), "{}", result.err);
    let out = &result.out;
    for expected in [
        "driver         browser",
        "target         http://localhost:8080/  (320x240)",
        "seed           42",
        "specification  specs/app.js",
        "states         4",
        "outcome        violation (1 violation: savedMessageAppears)",
        "state 0  +0 ms  [screenshot]",
        "  action  (initial state)",
        "state 1  +140 ms  [screenshot]",
        "  action  click [test_id=save] role=button \"Save note\" at (160, 135) [clicks]",
        "state 2  +420 ms\n",
        "  action  type text=\"hello\" [inputs]",
        "state 3  +1900 ms  [screenshot]  [1 violation]",
        "  VIOLATION savedMessageAppears",
        "    formula  always > implies > eventually",
        "    message  the saved banner never appeared",
        "    cell     banner = null",
        "    cell     saving = true",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    assert!(
        out.contains("    banner  null\n    saving  true\n"),
        "{out}"
    );
    assert_eq!(out.matches("\nstate ").count(), 4, "{out}");
}

#[test]
fn a_terminal_trace_names_its_command_and_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let result = run_cli(&["trace", path_text(&path)]);
    assert_eq!(result.code, 0, "{}", result.err);
    assert!(
        result.out.contains("driver         terminal"),
        "{}",
        result.out
    );
    assert!(
        result
            .out
            .contains("target         sh -c 'echo ok'  (12x3)"),
        "{}",
        result.out
    );
    assert!(
        result.out.contains("specification  driver defaults"),
        "{}",
        result.out
    );
    assert!(
        result
            .out
            .contains("action  write text=\"exit 3\\r\" [typing]"),
        "{}",
        result.out
    );
}

fn shown_states(out: &str) -> Vec<u64> {
    out.lines()
        .filter_map(|line| line.strip_prefix("state "))
        .map(|rest| rest.split_whitespace().next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn state_selects_one_index_or_a_half_open_range() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let cases: [(&str, Vec<u64>); 7] = [
        ("2", vec![2]),
        ("0", vec![0]),
        ("1..3", vec![1, 2]),
        ("2..", vec![2, 3]),
        ("..2", vec![0, 1]),
        ("..", vec![0, 1, 2, 3]),
        ("4..", vec![]),
    ];
    for (selection, expected) in cases {
        let result = run_cli(&["trace", path_text(&path), "--state", selection]);
        assert_eq!(result.code, 0, "{selection}: {}", result.err);
        assert_eq!(shown_states(&result.out), expected, "{selection}");
    }
    let one = run_cli(&["trace", path_text(&path), "--state", "1..3"]);
    assert!(one.out.contains("showing        1 to 2"), "{}", one.out);
}

#[test]
fn bad_state_selections_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let cases = [
        (
            "4",
            "state 4 does not exist; the trace has 4 states (0 to 3)",
        ),
        ("0..5", "ends past the last state"),
        ("3..1", "starts after it ends"),
        ("x", "is not an index or a START..END range"),
        ("1..b", "is not an index or a START..END range"),
        ("-1", "is not an index or a START..END range"),
        ("1...3", "is not an index or a START..END range"),
    ];
    for (selection, message) in cases {
        let flag = format!("--state={selection}");
        let result = run_cli(&["trace", path_text(&path), &flag]);
        assert_eq!(result.code, 1, "{selection}");
        assert!(result.out.is_empty(), "{selection}: {}", result.out);
        assert!(
            result.err.starts_with("ui-prop-tester: error: ") && result.err.contains(message),
            "{selection}: {}",
            result.err
        );
    }
}

#[test]
fn json_prints_one_state_object_per_line() {
    let directory = tempfile::tempdir().unwrap();
    let trace = browser_trace();
    let path = write_trace(directory.path(), "run.trace", &trace);
    let result = run_cli(&["trace", path_text(&path), "--json"]);
    assert_eq!(result.code, 0, "{}", result.err);
    let lines: Vec<&str> = result.out.lines().collect();
    assert_eq!(lines.len(), 4);
    for (line, state) in lines.iter().zip(&trace.states) {
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["index"], state.index);
        assert_eq!(value["timestamp_ms"], state.timestamp_ms);
        assert_eq!(value["state"], state.state);
        assert_eq!(value["cells"], state.cells);
        assert_eq!(value["action"], state.action.clone().unwrap_or(Value::Null));
        assert_eq!(value["has_screenshot"], state.screenshot.is_some());
        assert_eq!(
            value["violations"].as_array().unwrap().len(),
            state.violations.len()
        );
    }
    let last: Value = serde_json::from_str(lines[3]).unwrap();
    assert_eq!(last["violations"][0]["property"], "savedMessageAppears");
    let ranged = run_cli(&["trace", path_text(&path), "--json", "--state", "1..3"]);
    let indexes: Vec<u64> = ranged
        .out
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["index"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(indexes, vec![1, 2]);
}

#[test]
fn screen_draws_a_framed_plain_grid_with_wide_characters() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let result = run_cli(&["trace", path_text(&path), "--screen", "--state", "0"]);
    assert_eq!(result.code, 0, "{}", result.err);
    let expected = "state 0  +0 ms  (initial state)\n\
12x3  cursor row 1 column 4 (visible bar)\n\
\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}\n\
\u{2502}$ ok        \u{2502}\n\
\u{2502}\u{4f60}\u{597d} wide   \u{2502}\n\
\u{2502}            \u{2502}\n\
\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}\n";
    assert_eq!(result.out, expected);
    assert!(!result.out.contains('\x1b'));
}

#[test]
fn screen_notes_the_exit_and_lists_violations() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let result = run_cli(&["trace", path_text(&path), "--screen", "--state", "1.."]);
    assert_eq!(result.code, 0, "{}", result.err);
    assert!(result.out.contains("exited with code 3"), "{}", result.out);
    assert!(
        result.out.contains("\u{2502}bye         \u{2502}"),
        "{}",
        result.out
    );
    assert!(
        result.out.contains("VIOLATION exitsCleanly"),
        "{}",
        result.out
    );
}

#[test]
fn color_adds_sgr_sequences_for_every_attribute() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let result = run_cli(&[
        "trace",
        path_text(&path),
        "--screen",
        "--color",
        "--state",
        "0",
    ]);
    assert_eq!(result.code, 0, "{}", result.err);
    let out = &result.out;
    for sequence in [
        "\x1b[1;32m$",
        "\x1b[3;4:3;58;2;255;0;0mo",
        "\x1b[7;9;48;2;10;20;30mk",
        "\x1b[2;38;5;200m\u{4f60}",
        "\x1b[4:2;5;53m\u{597d}",
        "\x1b[8;91;44mw",
        "\x1b[4;58;5;33mi",
        "\x1b[4:4md",
        "\x1b[4:5;38;2;1;2;3me",
    ] {
        assert!(out.contains(sequence), "missing {sequence:?} in {out:?}");
    }
    let rows: Vec<&str> = out
        .lines()
        .filter(|line| line.starts_with('\u{2502}'))
        .collect();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let visible = strip_sgr(row);
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(visible.as_str()),
            14,
            "{row:?}"
        );
        let last_escape = row.rfind('\x1b').map(|position| &row[position..]);
        assert!(
            last_escape.is_none_or(|tail| tail.starts_with("\x1b[0m")),
            "{row:?}"
        );
    }
}

fn strip_sgr(text: &str) -> String {
    let mut plain = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            for inner in characters.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
        } else {
            plain.push(character);
        }
    }
    plain
}

#[test]
fn screen_on_a_browser_trace_names_the_right_flag() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let result = run_cli(&["trace", path_text(&path), "--screen"]);
    assert_eq!(result.code, 1);
    assert!(result.err.contains("--screenshots"), "{}", result.err);
    assert!(result.out.is_empty());
}

#[test]
fn conflicting_output_flags_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let shots = directory.path().join("shots");
    for args in [
        vec!["--json", "--screen"],
        vec!["--color"],
        vec!["--screenshots", path_text(&shots), "--json"],
    ] {
        let mut full = vec!["trace", path_text(&path)];
        full.extend(args.iter());
        let result = run_cli(&full);
        assert_eq!(result.code, 1, "{args:?}");
        assert!(
            result.err.starts_with("ui-prop-tester: error: "),
            "{}",
            result.err
        );
    }
}

#[test]
fn screenshots_are_written_kept_and_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let trace = browser_trace();
    let path = write_trace(directory.path(), "run.trace", &trace);
    let shots = directory.path().join("nested").join("shots");
    let result = run_cli(&[
        "trace",
        path_text(&path),
        "--screenshots",
        path_text(&shots),
    ]);
    assert_eq!(result.code, 0, "{}", result.err);
    let mut names: Vec<String> = fs::read_dir(&shots)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["state-000000.png", "state-000001.png", "state-000003.png"]
    );
    assert_eq!(
        fs::read(shots.join("state-000003.png")).unwrap(),
        trace.states[3].screenshot.clone().unwrap()
    );
    assert!(
        result
            .out
            .contains("3 written, 0 already present, 1 states without a screenshot"),
        "{}",
        result.out
    );

    let again = run_cli(&[
        "trace",
        path_text(&path),
        "--screenshots",
        path_text(&shots),
    ]);
    assert_eq!(again.code, 0, "{}", again.err);
    assert!(
        again.out.contains("0 written, 3 already present"),
        "{}",
        again.out
    );

    fs::remove_file(shots.join("state-000000.png")).unwrap();
    fs::write(shots.join("state-000001.png"), b"not the same").unwrap();
    let conflict = run_cli(&[
        "trace",
        path_text(&path),
        "--screenshots",
        path_text(&shots),
    ]);
    assert_eq!(conflict.code, 1);
    assert!(
        conflict.err.contains("state-000001.png") && conflict.err.contains("different content"),
        "{}",
        conflict.err
    );
    assert_eq!(
        fs::read(shots.join("state-000001.png")).unwrap(),
        b"not the same"
    );
    assert!(!shots.join("state-000000.png").exists());

    let selected = run_cli(&[
        "trace",
        path_text(&path),
        "--screenshots",
        path_text(&shots),
        "--state",
        "0",
    ]);
    assert_eq!(selected.code, 0, "{}", selected.err);
    assert!(shots.join("state-000000.png").exists());
}

#[test]
fn screenshots_refuse_a_terminal_trace() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let shots = directory.path().join("shots");
    let result = run_cli(&[
        "trace",
        path_text(&path),
        "--screenshots",
        path_text(&shots),
    ]);
    assert_eq!(result.code, 1);
    assert!(result.err.contains("terminal trace"), "{}", result.err);
    assert!(result.err.contains("--screen"), "{}", result.err);
    assert!(!shots.exists());
}

#[test]
fn missing_and_foreign_files_are_errors() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.trace");
    let result = run_cli(&["trace", path_text(&missing)]);
    assert_eq!(result.code, 1);
    assert!(
        result.err.starts_with("ui-prop-tester: error: ") && result.err.contains("does not exist"),
        "{}",
        result.err
    );

    let foreign = directory.path().join("notes.txt");
    fs::write(&foreign, "hello, this is not a trace").unwrap();
    let result = run_cli(&["trace", path_text(&foreign)]);
    assert_eq!(result.code, 1);
    assert!(
        result.err.contains("not a ui-prop-tester trace"),
        "{}",
        result.err
    );

    let mut truncated =
        fs::read(write_trace(directory.path(), "cut.trace", &browser_trace())).unwrap();
    truncated.truncate(truncated.len() - 10);
    let cut = directory.path().join("cut.trace");
    fs::write(&cut, truncated).unwrap();
    let result = run_cli(&["trace", path_text(&cut)]);
    assert_eq!(result.code, 1);
    assert!(result.err.contains("cannot read trace"), "{}", result.err);
    assert!(result.out.is_empty());
}
