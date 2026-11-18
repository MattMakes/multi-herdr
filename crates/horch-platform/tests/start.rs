//! The concern start: a process that `start::Command` starts never has a
//! forbidden name (SR-7), whether the parent has it or the command sets it.

use horch_platform::start::{self, FORBIDDEN_ENV};
use std::process::Stdio;

/// A name that the probe must see, so a test cannot pass because the probe
/// reads no environment at all.
const CONTROL: &str = "HORCH_START_TEST_CONTROL";

/// A child that prints its environment, then exits 3 when it has `name`.
#[cfg(unix)]
fn probe(name: &str) -> start::Command {
    let mut cmd = start::command("sh");
    cmd.arg("-c")
        .arg(format!("env; [ -z \"${{{name}+x}}\" ] || exit 3"));
    cmd
}

/// A child that prints its environment, then exits 3 when it has `name`.
#[cfg(windows)]
fn probe(name: &str) -> start::Command {
    let mut cmd = start::command("cmd");
    cmd.args(["/C", &format!("set & if defined {name} exit 3")]);
    cmd
}

/// Each forbidden name set in this process and on the command, with the
/// control name on the command. Gives the probe of `name`.
fn armed(name: &str) -> start::Command {
    for key in FORBIDDEN_ENV {
        std::env::set_var(key, "forbidden-in-test");
    }
    let mut cmd = probe(name);
    for key in FORBIDDEN_ENV {
        cmd.env(key, "forbidden-in-test");
    }
    cmd.env(CONTROL, "1");
    cmd
}

/// The child printed the control name and no forbidden name.
fn assert_clean(name: &str, out: &std::process::Output) {
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.lines().any(|l| l.starts_with(&format!("{CONTROL}="))),
        "the probe printed no environment:\n{text}"
    );
    assert!(
        !text.lines().any(|l| l.starts_with(&format!("{name}="))),
        "the child has {name}"
    );
    assert!(out.status.success(), "the child has {name}: {}", out.status);
}

#[test]
fn forbidden_name_never_reaches_the_child_of_spawn() {
    for name in FORBIDDEN_ENV {
        let child = armed(name)
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the probe");
        let out = child.wait_with_output().expect("wait for the probe");
        assert_clean(name, &out);
    }
}

#[test]
fn forbidden_name_never_reaches_the_child_of_output() {
    for name in FORBIDDEN_ENV {
        let out = armed(name).output().expect("run the probe");
        assert_clean(name, &out);
    }
}

#[test]
fn forbidden_name_never_reaches_the_child_of_status() {
    for name in FORBIDDEN_ENV {
        let status = armed(name)
            .stdout(Stdio::null())
            .status()
            .expect("run the probe");
        assert!(status.success(), "the child has {name}: {status}");
    }
}

#[test]
fn env_after_env_remove_cannot_add_a_forbidden_name() {
    for name in FORBIDDEN_ENV {
        let mut cmd = probe(name);
        cmd.env_remove(name)
            .env(name, "forbidden-in-test")
            .env(CONTROL, "1");
        let out = cmd.output().expect("run the probe");
        assert_clean(name, &out);
    }
}
