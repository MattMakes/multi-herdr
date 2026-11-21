//! `horch idea` and `horch ideas` (ticket 05 of fleetd-p6): each subcommand
//! prints its help with its flags, and a bad flag exits 2.
//!
//! Each test runs the real binary with its own home, data, state and ledger
//! folders, and the watchdog off.

use std::process::Command;

struct Out {
    code: i32,
    out: String,
    err: String,
}

fn horch(args: &[&str]) -> Out {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_horch"))
        .args(args)
        .current_dir(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_DATA_DIR", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
        .env("HORCH_PROJECT_DIR", &root)
        .env("FLEET_ROOT", root.join("fleet"))
        .env("HORCH_NO_WATCHDOG", "1")
        .output()
        .unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn assert_help(args: &[&str], words: &[&str]) {
    let o = horch(args);
    assert_eq!(
        o.code, 0,
        "{args:?}\nstdout:\n{}\nstderr:\n{}",
        o.out, o.err
    );
    for w in words {
        assert!(o.out.contains(w), "{args:?} help has no {w:?}:\n{}", o.out);
    }
}

#[test]
fn idea_help_lists_the_8_subcommands() {
    assert_help(
        &["idea", "--help"],
        &[
            "add", "amend", "show", "note", "approve", "revise", "park", "cancel",
        ],
    );
}

#[test]
fn each_idea_subcommand_prints_its_help_with_its_flags() {
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["idea", "add", "--help"],
            &[
                "TEXT",
                "--file",
                "-f",
                "--direct",
                "--to",
                "--priority",
                "--json",
            ],
        ),
        (&["idea", "amend", "--help"], &["<ID>", "<TEXT>"]),
        (
            &["idea", "show", "--help"],
            &["<ID>", "--in-flight", "--json"],
        ),
        (
            &["idea", "note", "--help"],
            &[
                "<ID>",
                "<TEXT>",
                "--question",
                "--case",
                "covered",
                "revise",
                "replan",
                "own_decision",
                "--cite",
                "--dismiss",
            ],
        ),
        (&["idea", "approve", "--help"], &["<ID>"]),
        (&["idea", "revise", "--help"], &["<ID>", "<TEXT>"]),
        (&["idea", "park", "--help"], &["<ID>", "[REASON]"]),
        (&["idea", "cancel", "--help"], &["<ID>", "<REASON>"]),
        (&["ideas", "--help"], &["--json"]),
    ];
    for (args, words) in cases {
        assert_help(args, words);
    }
}

#[test]
fn a_bad_flag_exits_2() {
    for args in [
        &["idea", "add", "x", "--bogus"][..],
        &["idea", "add", "x", "--priority", "high"],
        &["idea", "add", "x", "--direct", "t.md"],
        &["idea", "add"],
        &["idea", "amend", "I-1"],
        &["idea", "show", "I-1", "--bogus"],
        &[
            "idea",
            "note",
            "I-1",
            "x",
            "--case",
            "maybe",
            "--question",
            "Q-1",
        ],
        &["idea", "note", "I-1", "x", "--case", "covered"],
        &["idea", "note", "I-1", "x", "--cite", "a.rs:1"],
        &["idea", "approve"],
        &["idea", "revise", "I-1"],
        &["idea", "park"],
        &["idea", "cancel", "I-1"],
        &["idea", "bogus"],
        &["ideas", "--bogus"],
    ] {
        let o = horch(args);
        assert_eq!(
            o.code, 2,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
    }
}
