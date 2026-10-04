//! `horch doctor` - the port of the justfile's `require-herdr` recipe.
//!
//! Fails fast with a clear message when herdr is missing or its server is not
//! reachable. The old `jq` check is gone: nothing shells out to jq any more.
//! It also checks the host tools (`requires:`) of the teammates this project
//! is offered.

use std::ffi::OsStr;
use std::process::Command;

use anyhow::{bail, Result};
use horch_core::harness::inventory::BinaryFacts;
use horch_core::roster::{operator_effort_warnings, Requirement, Roster};
use horch_core::runtime::{process, RuntimeContext};
use horch_core::workspace::herdr::Herdr;

pub fn doctor(ctx: &RuntimeContext) -> Result<()> {
    check(ctx)?;
    println!("herdr is installed and its server is reachable.");
    let mut roster = super::load_roster(ctx, None)?;
    // Count and check what a fleet started here would offer (`offer_when`).
    if let Ok(project) = ctx.paths.project() {
        roster = roster.with_project_facts(super::recipes::project_facts(&project));
    }
    let problems = roster.check();
    if problems.is_empty() {
        println!(
            "roster: {} teammates, {} offered to the orchestrator.",
            roster.names().len(),
            roster.offered().len()
        );
    } else {
        // Not fatal: a fleet still launches, but at least one teammate would
        // misbehave in a pane nobody is watching.
        eprintln!(
            "roster has {} problem(s) (see horch teammates --check):",
            problems.len()
        );
        for p in &problems {
            eprintln!("  {p}");
        }
    }
    // Not fatal either: the fleet launches, but the teammate that needs the
    // tool stalls in its pane.
    let tool_problems = requirement_problems(
        &roster,
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
    );
    for p in &tool_problems {
        eprintln!("warning: {p}");
    }
    // Not fatal: the harness's teammates are unusable, the rest work.
    for w in broken_harness_warnings(&super::agentlist::gather(ctx, true)) {
        eprintln!("warning: {w}");
    }
    // Settings that quietly override the effort in every teammate file.
    let home = ctx.inherited.home_var.as_deref().map(std::path::Path::new);
    let codex_home = horch_core::harness::codex::codex_home(
        &ctx.paths.home,
        ctx.inherited.codex_home.as_deref(),
    );
    for w in operator_effort_warnings(
        home,
        &codex_home,
        ctx.inherited.claude_code_effort_level.as_deref(),
    ) {
        eprintln!("warning: {w}");
    }
    Ok(())
}

/// The precondition every recipe shares. Deliberately does NOT validate the
/// roster: a roster warning must not stop a fleet from launching.
pub fn check(ctx: &RuntimeContext) -> Result<()> {
    let herdr_bin = ctx.bins.harness.herdr.clone();
    let found = if herdr_bin.components().count() > 1 {
        herdr_bin.is_file()
    } else {
        process::which(
            ctx.inherited.path.as_deref(),
            ctx.inherited.pathext.as_deref(),
            &herdr_bin.to_string_lossy(),
        )
        .is_some()
    };
    if !found {
        bail!(
            "herdr CLI not found on PATH.\n\
             Install it with `herdr-install`, or see https://herdr.dev/docs/install/"
        );
    }
    if !Herdr::with_bin(&ctx.bins.harness.herdr).server_reachable() {
        bail!(
            "herdr server is not reachable (herdr workspace list failed). Checks:\n\
             \x20 - is a herdr session running? (launch the herdr app, or `herdr server` headless)\n\
             \x20 - `herdr status` shows the expected socket path; export HERDR_SESSION /\n\
             \x20   HERDR_SOCKET_PATH if you run a non-default session"
        );
    }
    Ok(())
}

/// One line per harness whose `--version` ran and failed.
fn broken_harness_warnings(
    facts: &std::collections::BTreeMap<&'static str, BinaryFacts>,
) -> Vec<String> {
    facts
        .iter()
        .filter_map(|(name, f)| {
            let why = f.broken.as_deref()?.trim_end_matches('.');
            Some(format!(
                "harness {name} is broken: {why}. The quota probe marks it broken and \
                 routing then does not choose its teammates. Fix `{name} --version` first."
            ))
        })
        .collect()
}

/// What is missing for the `requires:` of the teammates `roster` offers, one
/// line per requirement. Nothing is checked that no offered teammate needs.
pub(crate) fn requirement_problems(
    roster: &Roster,
    path: Option<&OsStr>,
    pathext: Option<&str>,
) -> Vec<String> {
    let offered = roster.offered();
    let mut problems = Vec::new();
    for requirement in [Requirement::Xcode] {
        let needed_by: Vec<&str> = offered
            .iter()
            .filter(|t| t.requires.contains(&requirement))
            .map(|t| t.name.as_str())
            .collect();
        if needed_by.is_empty() {
            continue;
        }
        let problem = match requirement {
            Requirement::Xcode => xcode_problem(path, pathext),
        };
        if let Some(problem) = problem {
            problems.push(format!(
                "{} (needed by {}): {problem}",
                requirement.as_str(),
                needed_by.join(", ")
            ));
        }
    }
    problems
}

/// `xcodebuild` must be on PATH and past its first launch, or the first build
/// in a pane stalls at the licence prompt. `-checkFirstLaunchStatus` exits 0
/// when nothing is left to do. With only the Command Line Tools selected, the
/// `/usr/bin/xcodebuild` shim fails and says so on stderr.
fn xcode_problem(path: Option<&OsStr>, pathext: Option<&str>) -> Option<String> {
    let Some(bin) = process::which(path, pathext, "xcodebuild") else {
        return Some("xcodebuild not found on PATH. Install Xcode 26 or later.".into());
    };
    match Command::new(&bin).arg("-checkFirstLaunchStatus").output() {
        Ok(out) if out.status.success() => None,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            Some(format!(
                "`{} -checkFirstLaunchStatus` failed ({}). Run `sudo xcodebuild -runFirstLaunch`, \
                 and `sudo xcode-select -s /Applications/Xcode.app` if only the Command Line \
                 Tools are selected.{}",
                bin.display(),
                out.status,
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" xcodebuild said: {}", detail.trim())
                }
            ))
        }
        Err(e) => Some(format!("could not run {}: {e}", bin.display())),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use horch_core::roster::Teammate;

    fn roster_requiring_xcode() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(Teammate {
            name: "swift-developer".into(),
            brief_description: "Swift".into(),
            requires: vec![Requirement::Xcode],
            ..Teammate::default()
        });
        r
    }

    /// A fake `xcodebuild` in its own PATH directory that exits with `code`.
    fn fake_xcodebuild(code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("xcodebuild");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\n[ \"$1\" = -checkFirstLaunchStatus ] || exit 99\necho 'first launch pending' >&2\nexit {code}\n"),
        )
        .unwrap();
        process::make_executable(&bin).unwrap();
        dir
    }

    #[test]
    fn xcode_is_fine_when_first_launch_is_done() {
        let dir = fake_xcodebuild(0);
        let problems = requirement_problems(
            &roster_requiring_xcode(),
            Some(dir.path().as_os_str()),
            None,
        );
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn xcode_first_launch_pending_is_reported() {
        let dir = fake_xcodebuild(69);
        let problems = requirement_problems(
            &roster_requiring_xcode(),
            Some(dir.path().as_os_str()),
            None,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        // The built-in Swift teammates need Xcode too, so they share the line.
        assert!(problems[0].contains("(needed by "), "{}", problems[0]);
        assert!(problems[0].contains("swift-developer"), "{}", problems[0]);
        assert!(problems[0].contains("-runFirstLaunch"), "{}", problems[0]);
        assert!(
            problems[0].contains("first launch pending"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn missing_xcodebuild_is_reported() {
        let empty = tempfile::tempdir().unwrap();
        let problems = requirement_problems(
            &roster_requiring_xcode(),
            Some(empty.path().as_os_str()),
            None,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("xcodebuild not found"),
            "{}",
            problems[0]
        );
    }

    /// Nothing is checked when no offered teammate needs Xcode: the built-in
    /// Swift teammates need it, but a project without Apple files is not
    /// offered them. An Apple project is, so xcodebuild is checked.
    #[test]
    fn xcode_is_not_checked_unless_an_offered_teammate_needs_it() {
        let empty = tempfile::tempdir().unwrap();
        let path = Some(empty.path().as_os_str());
        let facts = |names: [&str; 1]| {
            Roster::builtin()
                .unwrap()
                .with_project_facts(horch_core::roster::ProjectFacts::from_names(names))
        };
        assert!(requirement_problems(&facts(["Cargo.toml"]), path, None).is_empty());

        let problems = requirement_problems(&facts(["Package.swift"]), path, None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("xcodebuild not found"),
            "{}",
            problems[0]
        );
    }

    #[test]
    fn a_broken_harness_is_a_doctor_warning() {
        let why = "pi --version exited 1: Error [ERR_REQUIRE_ESM]: require() of ES Module.";
        let facts = std::collections::BTreeMap::from([
            (
                "claude",
                BinaryFacts {
                    path: Some("/bin/claude".into()),
                    version: Some("2.1.0".into()),
                    broken: None,
                },
            ),
            (
                "pi",
                BinaryFacts {
                    path: Some("/bin/pi".into()),
                    version: None,
                    broken: Some(why.into()),
                },
            ),
        ]);
        let warnings = broken_harness_warnings(&facts);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with(
                "harness pi is broken: pi --version exited 1: Error [ERR_REQUIRE_ESM]: \
                 require() of ES Module. The quota probe"
            ),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains("Fix `pi --version` first."));
    }
}
