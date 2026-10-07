//! `horch doctor` - the port of the justfile's `require-herdr` recipe.
//!
//! Fails fast with a clear message when herdr is missing or its server is not
//! reachable. The old `jq` check is gone: nothing shells out to jq any more.
//! It also checks the host tools (`requires:`) of the teammates this project
//! is offered.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};
use horch_core::harness::inventory::BinaryFacts;
use horch_core::roster::{operator_effort_warnings, Requirement, Roster};
use horch_core::runtime::bins::{host_tool_bin, BLENDER_APP, GODOT_APP};
use horch_core::runtime::{process, RuntimeContext};
use horch_core::workspace::herdr::Herdr;

pub fn doctor(ctx: &RuntimeContext) -> Result<()> {
    check(ctx)?;
    println!("herdr is installed and its server is reachable.");
    let mut roster = super::load_roster_unwarned(ctx, None)?;
    // Count and check what a fleet started here would offer (`offer_when`).
    if let Ok(project) = ctx.paths.project() {
        roster = roster.with_project_facts(horch::project::project_facts(&project));
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
        HostTools {
            blender_path: ctx.inherited.blender_path.as_deref(),
            blender_app: BLENDER_APP.map(Path::new),
            godot_path: ctx.inherited.godot_path.as_deref(),
        },
        &ctx.bins.harness.git,
        ctx.paths.project().ok().as_deref(),
    );
    for p in &tool_problems {
        eprintln!("warning: {p}");
    }
    match xcode_export_check(
        &roster,
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
    ) {
        Some(ExportCheck::Note(n)) => println!("note: {n}"),
        Some(ExportCheck::Problem(p)) => eprintln!("warning: {p}"),
        None => {}
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

/// Where doctor looks for the host tools, after PATH: the operator's
/// variables, which win over PATH, and the app bundle, tried last.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HostTools<'a> {
    /// `BLENDER_PATH`.
    pub blender_path: Option<&'a OsStr>,
    /// Blender's app bundle executable ([`BLENDER_APP`]); `None` in tests.
    pub blender_app: Option<&'a Path>,
    /// `GODOT_PATH`.
    pub godot_path: Option<&'a OsStr>,
}

/// What is missing for the `requires:` of the teammates `roster` offers, one
/// line per requirement. Nothing is checked that no offered teammate needs.
/// `git` is the fleet's git (`HORCH_GIT_BIN`, else `git`); `project` is the
/// project directory, whose `.gitattributes` the git-lfs check reads.
pub(crate) fn requirement_problems(
    roster: &Roster,
    path: Option<&OsStr>,
    pathext: Option<&str>,
    tools: HostTools<'_>,
    git: &Path,
    project: Option<&Path>,
) -> Vec<String> {
    let offered = roster.offered();
    let mut problems = Vec::new();
    for requirement in [
        Requirement::Xcode,
        Requirement::Blender,
        Requirement::Godot,
        Requirement::GitLfs,
    ] {
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
            Requirement::Blender => {
                blender_problem(path, pathext, tools.blender_path, tools.blender_app)
            }
            Requirement::Godot => {
                godot_problem(path, pathext, tools.godot_path, GODOT_APP.map(Path::new))
            }
            Requirement::GitLfs => {
                git_lfs_problem(path, pathext, git).or_else(|| project.and_then(lfs_rule_problem))
            }
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

/// `git lfs version` must run with the fleet's git, or a new worktree of an
/// Unreal project holds LFS pointer files instead of assets and `git lfs
/// lock` fails. A bare `git` is looked up on PATH; a path is used as given.
fn git_lfs_problem(path: Option<&OsStr>, pathext: Option<&str>, git: &Path) -> Option<String> {
    let bin = if git.components().count() > 1 {
        git.to_path_buf()
    } else {
        match process::which(path, pathext, &git.to_string_lossy()) {
            Some(bin) => bin,
            None => {
                return Some(format!(
                    "{} not found on PATH. Install git, or set HORCH_GIT_BIN.",
                    git.display()
                ))
            }
        }
    };
    match Command::new(&bin).args(["lfs", "version"]).output() {
        Ok(out) if out.status.success() => None,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            Some(format!(
                "`{} lfs version` failed ({}). Install Git LFS (macOS: `brew install \
                 git-lfs`), then run `git lfs install` once.{}",
                bin.display(),
                out.status,
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" git said: {}", detail.trim())
                }
            ))
        }
        Err(e) => Some(format!("could not run {}: {e}", bin.display())),
    }
}

/// In an Unreal project (a `*.uproject` at the top), `.gitattributes` must
/// send `*.uasset` to LFS, or the first asset commit puts binary bytes in
/// git history. `None` outside an Unreal project.
fn lfs_rule_problem(project: &Path) -> Option<String> {
    let unreal = std::fs::read_dir(project)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "uproject"));
    if !unreal {
        return None;
    }
    let attributes = project.join(".gitattributes");
    let text = std::fs::read_to_string(&attributes).unwrap_or_default();
    let has_rule = text.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next().is_some_and(|p| p.ends_with("*.uasset")) && fields.any(|f| f == "filter=lfs")
    });
    if has_rule {
        return None;
    }
    Some(format!(
        "{} has no Git LFS rule for *.uasset. Run `git lfs track \"*.uasset\" \
         \"*.umap\" --lockable` and commit .gitattributes; see \
         skills/ue-build-verify/references/git-lfs.md.",
        attributes.display()
    ))
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

/// The first Xcode with `xcrun agent skills export`.
const XCODE_EXPORT_MIN: u32 = 27;

/// What doctor says about the Xcode agent-skills export. A note is not a
/// failure: it says the check cannot run on this host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportCheck {
    Note(String),
    Problem(String),
}

/// U-54: an offered Apple teammate (`requires: xcode`) with `operator_skills:`
/// gets them from `xcrun agent skills export --output-dir <dir>`
/// (docs/skills-and-teams.md). On Xcode 27 or later, `--help` must run and
/// name `--output-dir`. Below 27 the command does not exist, so this is a
/// note. `None` when no such teammate is offered, when all is well, or when
/// `xcodebuild` is missing ([`xcode_problem`] reports that).
pub(crate) fn xcode_export_check(
    roster: &Roster,
    path: Option<&OsStr>,
    pathext: Option<&str>,
) -> Option<ExportCheck> {
    let needed_by: Vec<&str> = roster
        .offered()
        .iter()
        .filter(|t| t.requires.contains(&Requirement::Xcode) && t.operator_skills.is_some())
        .map(|t| t.name.as_str())
        .collect();
    if needed_by.is_empty() {
        return None;
    }
    let head = format!("xcode skills export (needed by {})", needed_by.join(", "));
    let xcodebuild = process::which(path, pathext, "xcodebuild")?;
    let version = Command::new(&xcodebuild)
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| xcode_version(&String::from_utf8_lossy(&o.stdout)));
    let Some((major, text)) = version else {
        return Some(ExportCheck::Note(format!(
            "{head}: `xcodebuild -version` gave no Xcode version, so \
             `xcrun agent skills export` is not verifiable here."
        )));
    };
    if major < XCODE_EXPORT_MIN {
        return Some(ExportCheck::Note(format!(
            "{head}: Xcode {text} has no `xcrun agent skills export` (Xcode \
             {XCODE_EXPORT_MIN} or later), so it is not verifiable here."
        )));
    }
    let Some(xcrun) = process::which(path, pathext, "xcrun") else {
        return Some(ExportCheck::Problem(format!(
            "{head}: xcrun not found on PATH beside Xcode {text}."
        )));
    };
    let args = ["agent", "skills", "export", "--help"];
    match Command::new(&xcrun).args(args).output() {
        Ok(out) => {
            let help = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if out.status.success() && help.contains("--output-dir") {
                None
            } else {
                let detail = help.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                Some(ExportCheck::Problem(format!(
                    "{head}: `{} {}` on Xcode {text} {} or does not name --output-dir. \
                     Check docs/skills-and-teams.md against it.{}",
                    xcrun.display(),
                    args.join(" "),
                    if out.status.success() {
                        "ran"
                    } else {
                        "failed"
                    },
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(" xcrun said: {}", detail.trim())
                    }
                )))
            }
        }
        Err(e) => Some(ExportCheck::Problem(format!(
            "{head}: could not run {}: {e}",
            xcrun.display()
        ))),
    }
}

/// The major version and the full version from `xcodebuild -version`, whose
/// first line is `Xcode 26.6`.
fn xcode_version(stdout: &str) -> Option<(u32, String)> {
    let text = stdout.lines().next()?.strip_prefix("Xcode ")?.trim();
    let major = text.split('.').next()?.parse().ok()?;
    Some((major, text.to_string()))
}

/// The oldest Blender the live MCP tools run on.
const BLENDER_LIVE_MIN: (u32, u32) = (5, 1);

/// Blender must run, or the blender-artist reports `BLOCKED:` at its first
/// step. The Blender Lab MCP server runs `BLENDER_PATH` when it is set, else
/// `blender` on PATH; a launch sets `BLENDER_PATH` from the same lookup
/// ([`host_tool_bin`]), which tries `app` last. Its `*_for_cli` tools run any
/// Blender; the live tools need [`BLENDER_LIVE_MIN`] and the add-on.
fn blender_problem(
    path: Option<&OsStr>,
    pathext: Option<&str>,
    blender_path: Option<&OsStr>,
    app: Option<&Path>,
) -> Option<String> {
    let Some(bin) = host_tool_bin(blender_path, path, pathext, "blender", app) else {
        return Some(
            "blender not found on PATH and BLENDER_PATH is not set. Install Blender \
             5.1 or later, then put `blender` on PATH or set BLENDER_PATH (macOS: \
             /Applications/Blender.app/Contents/MacOS/Blender)."
                .into(),
        );
    };
    let out = match Command::new(&bin).arg("--version").output() {
        Ok(out) => out,
        Err(e) => {
            return Some(format!(
                "could not run {}: {e}. Set BLENDER_PATH to the Blender executable \
                 (macOS: /Applications/Blender.app/Contents/MacOS/Blender).",
                bin.display()
            ))
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        return Some(format!(
            "`{} --version` failed ({}). Reinstall Blender 5.1 or later, or point \
             BLENDER_PATH at a working one.{}",
            bin.display(),
            out.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" blender said: {}", detail.trim())
            }
        ));
    }
    match blender_version(&stdout) {
        Some(v) if v < BLENDER_LIVE_MIN => Some(format!(
            "{} is Blender {}.{}. The live MCP tools need {}.{} or later; only the \
             `*_for_cli` tools work. Install Blender {}.{} or later.",
            bin.display(),
            v.0,
            v.1,
            BLENDER_LIVE_MIN.0,
            BLENDER_LIVE_MIN.1,
            BLENDER_LIVE_MIN.0,
            BLENDER_LIVE_MIN.1
        )),
        _ => None,
    }
}

/// `(major, minor)` from the first `Blender X.Y[.Z]` line of `--version`.
fn blender_version(stdout: &str) -> Option<(u32, u32)> {
    let rest = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("Blender "))?;
    let mut parts = rest.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The oldest Godot the godot-* teammates and their skills support.
const GODOT_MIN: (u32, u32) = (4, 3);

/// Godot must run headless, or every godot-* teammate reports `BLOCKED:` at
/// its first parse check. Looks at `GODOT_PATH`, then `godot` on PATH, then
/// `app` (the macOS bundle) when it exists.
fn godot_problem(
    path: Option<&OsStr>,
    pathext: Option<&str>,
    godot_path: Option<&OsStr>,
    app: Option<&Path>,
) -> Option<String> {
    let Some(bin) = host_tool_bin(godot_path, path, pathext, "godot", app) else {
        return Some(
            "godot not found on PATH and GODOT_PATH is not set. Install Godot 4.3 \
             or later, then put `godot` on PATH or set GODOT_PATH (macOS: \
             /Applications/Godot.app/Contents/MacOS/Godot)."
                .into(),
        );
    };
    let out = match Command::new(&bin).arg("--version").output() {
        Ok(out) => out,
        Err(e) => {
            return Some(format!(
                "could not run {}: {e}. Set GODOT_PATH to the Godot executable \
                 (macOS: /Applications/Godot.app/Contents/MacOS/Godot).",
                bin.display()
            ))
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        return Some(format!(
            "`{} --version` failed ({}). Reinstall Godot 4.3 or later, or point \
             GODOT_PATH at a working one.{}",
            bin.display(),
            out.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" godot said: {}", detail.trim())
            }
        ));
    }
    match godot_version(&stdout) {
        Some(v) if v < GODOT_MIN => Some(format!(
            "{} is Godot {}.{}. The godot-* teammates need {}.{} or later. Install \
             Godot {}.{} or later.",
            bin.display(),
            v.0,
            v.1,
            GODOT_MIN.0,
            GODOT_MIN.1,
            GODOT_MIN.0,
            GODOT_MIN.1
        )),
        Some(_) => None,
        None => Some(format!(
            "`{} --version` printed no Godot version (want `4.3.stable...` or later). \
             Point GODOT_PATH at the Godot executable.",
            bin.display()
        )),
    }
}

/// `(major, minor)` from the first `X.Y[.Z].<status>...` line of
/// `godot --version`, for example `4.7.2.stable.official.<hash>`.
fn godot_version(stdout: &str) -> Option<(u32, u32)> {
    stdout.lines().find_map(|l| {
        let mut parts = l.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    })
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

    /// The xcode line alone: with no project facts every built-in teammate
    /// is offered, so the blender-artist adds a Blender line.
    fn xcode_problems(path: &std::path::Path) -> Vec<String> {
        requirement_problems(
            &roster_requiring_xcode(),
            Some(path.as_os_str()),
            None,
            HostTools::default(),
            Path::new("git"),
            None,
        )
        .into_iter()
        .filter(|p| p.starts_with("xcode "))
        .collect()
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
        let problems = xcode_problems(dir.path());
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn xcode_first_launch_pending_is_reported() {
        let dir = fake_xcodebuild(69);
        let problems = xcode_problems(dir.path());
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
        let problems = xcode_problems(empty.path());
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
        assert!(requirement_problems(
            &facts(["Cargo.toml"]),
            path,
            None,
            HostTools::default(),
            Path::new("git"),
            None
        )
        .is_empty());

        let problems = requirement_problems(
            &facts(["Package.swift"]),
            path,
            None,
            HostTools::default(),
            Path::new("git"),
            None,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("xcodebuild not found"),
            "{}",
            problems[0]
        );
    }

    /// A PATH directory with a fake `xcodebuild` that prints `Xcode <version>`
    /// for `-version`, and a fake `xcrun` that prints `help` and exits with
    /// `code` for `agent skills export --help` only.
    fn fake_xcode(version: &str, help: &str, code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let xcodebuild = dir.path().join("xcodebuild");
        std::fs::write(
            &xcodebuild,
            format!("#!/bin/sh\n[ \"$1\" = -version ] || exit 99\nprintf 'Xcode {version}\\nBuild version 1A1\\n'\n"),
        )
        .unwrap();
        process::make_executable(&xcodebuild).unwrap();
        let xcrun = dir.path().join("xcrun");
        std::fs::write(
            &xcrun,
            format!("#!/bin/sh\n[ \"$*\" = 'agent skills export --help' ] || exit 99\necho '{help}'\nexit {code}\n"),
        )
        .unwrap();
        process::make_executable(&xcrun).unwrap();
        dir
    }

    /// The built-in roster, all offered: swift-developer and
    /// apple-platform-developer need Xcode and name operator skills.
    fn export_check(dir: &Path) -> Option<ExportCheck> {
        xcode_export_check(&Roster::builtin().unwrap(), Some(dir.as_os_str()), None)
    }

    /// U-54: on Xcode 26 the export command does not exist; doctor notes it
    /// and does not fail.
    #[test]
    fn xcode_export_before_27_is_a_note() {
        let dir = fake_xcode("26.6", "", 99);
        let Some(ExportCheck::Note(n)) = export_check(dir.path()) else {
            panic!("expected a note");
        };
        assert!(n.contains("not verifiable here"), "{n}");
        assert!(n.contains("Xcode 26.6"), "{n}");
        assert!(n.contains("swift-developer"), "{n}");
    }

    /// U-54: on Xcode 27 the `--help` of the export must name `--output-dir`.
    #[test]
    fn xcode_export_on_27_is_verified() {
        let dir = fake_xcode("27.0", "  --output-dir <dir>  Where to write", 0);
        assert_eq!(export_check(dir.path()), None);

        let dir = fake_xcode("27.1", "error: unknown subcommand 'agent'", 1);
        let Some(ExportCheck::Problem(p)) = export_check(dir.path()) else {
            panic!("expected a problem");
        };
        assert!(p.contains("failed"), "{p}");
        assert!(p.contains("unknown subcommand"), "{p}");

        let dir = fake_xcode("27.0", "  --dest <dir>", 0);
        let Some(ExportCheck::Problem(p)) = export_check(dir.path()) else {
            panic!("expected a problem");
        };
        assert!(p.contains("--output-dir"), "{p}");
    }

    /// U-54: no check without an offered Xcode teammate that has operator
    /// skills, and none without xcodebuild.
    #[test]
    fn xcode_export_is_not_checked_unless_needed() {
        let dir = fake_xcode("27.0", "", 1);
        let rust_project = Roster::builtin()
            .unwrap()
            .with_project_facts(horch_core::roster::ProjectFacts::from_names(["Cargo.toml"]));
        assert_eq!(
            xcode_export_check(&rust_project, Some(dir.path().as_os_str()), None),
            None
        );
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(export_check(empty.path()), None);
    }

    #[test]
    fn xcode_version_reads_the_first_line() {
        assert_eq!(
            xcode_version("Xcode 26.6\nBuild version 17F113\n"),
            Some((26, "26.6".to_string()))
        );
        assert_eq!(xcode_version("Xcode 27\n"), Some((27, "27".to_string())));
        assert_eq!(xcode_version("xcodebuild: error\n"), None);
    }

    fn roster_requiring_blender() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(Teammate {
            name: "blender-test".into(),
            brief_description: "Blender".into(),
            requires: vec![Requirement::Blender],
            ..Teammate::default()
        });
        r
    }

    /// A fake `blender` in its own directory: `--version` prints `version`
    /// and exits with `code`.
    fn fake_blender(version: &str, code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("blender");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] || exit 99\n\
                 echo 'Blender {version}'\necho '\tbuild date: 2026-09-01'\n\
                 echo 'cannot open display' >&2\nexit {code}\n"
            ),
        )
        .unwrap();
        process::make_executable(&bin).unwrap();
        dir
    }

    fn blender_problems(path: &std::path::Path, blender_path: Option<&OsStr>) -> Vec<String> {
        requirement_problems(
            &roster_requiring_blender(),
            Some(path.as_os_str()),
            None,
            HostTools {
                blender_path,
                ..HostTools::default()
            },
            Path::new("git"),
            None,
        )
        .into_iter()
        .filter(|p| p.starts_with("blender "))
        .collect()
    }

    #[test]
    fn blender_on_path_is_fine() {
        let dir = fake_blender("5.1.0", 0);
        assert!(blender_problems(dir.path(), None).is_empty());
    }

    #[test]
    fn missing_blender_names_both_fixes() {
        let empty = tempfile::tempdir().unwrap();
        let problems = blender_problems(empty.path(), None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("blender-test"), "{}", problems[0]);
        assert!(problems[0].contains("blender not found"), "{}", problems[0]);
        assert!(problems[0].contains("BLENDER_PATH"), "{}", problems[0]);
    }

    /// `BLENDER_PATH` wins over PATH, as in the MCP server: a broken one is
    /// reported even with a good `blender` on PATH.
    #[test]
    fn blender_path_wins_over_path() {
        let good = fake_blender("5.1.0", 0);
        let broken = fake_blender("5.1.0", 2);
        let bin = broken.path().join("blender");
        let problems = blender_problems(good.path(), Some(bin.as_os_str()));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("--version` failed"), "{}", problems[0]);
        assert!(
            problems[0].contains("cannot open display"),
            "{}",
            problems[0]
        );

        let empty = tempfile::tempdir().unwrap();
        let fine = good.path().join("blender");
        assert!(blender_problems(empty.path(), Some(fine.as_os_str())).is_empty());

        let gone = empty.path().join("Blender");
        let problems = blender_problems(good.path(), Some(gone.as_os_str()));
        assert!(problems[0].contains("could not run"), "{problems:?}");
    }

    /// The app bundle is the last fallback, after `BLENDER_PATH` and PATH:
    /// the same lookup that sets `BLENDER_PATH` for a launch.
    #[test]
    fn the_blender_app_is_the_last_fallback() {
        let empty = tempfile::tempdir().unwrap();
        let app = fake_blender("5.1.0", 0);
        let path = Some(empty.path().as_os_str());
        assert!(blender_problem(path, None, None, Some(&app.path().join("blender"))).is_none());
        let gone = empty.path().join("Blender");
        let problem = blender_problem(path, None, None, Some(&gone)).unwrap();
        assert!(problem.contains("blender not found"), "{problem}");
    }

    #[test]
    fn an_old_blender_is_reported() {
        let dir = fake_blender("4.2.3 LTS", 0);
        let problems = blender_problems(dir.path(), None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("Blender 4.2"), "{}", problems[0]);
        assert!(problems[0].contains("5.1 or later"), "{}", problems[0]);
    }

    #[test]
    fn blender_version_reads_the_first_line() {
        assert_eq!(
            blender_version("Blender 5.1.0\n\tbuild hash: x\n"),
            Some((5, 1))
        );
        assert_eq!(blender_version("Blender 4.2.3 LTS\n"), Some((4, 2)));
        assert_eq!(blender_version("Blender 10.0\n"), Some((10, 0)));
        assert_eq!(blender_version("hello\n"), None);
    }

    /// The built-in blender-artist needs Blender: a Blender project is
    /// offered it, so doctor checks; a Rust project is not.
    #[test]
    fn the_blender_artist_needs_blender() {
        let empty = tempfile::tempdir().unwrap();
        let path = Some(empty.path().as_os_str());
        let facts = |names: [&str; 1]| {
            Roster::builtin()
                .unwrap()
                .with_project_facts(horch_core::roster::ProjectFacts::from_names(names))
        };
        assert!(requirement_problems(
            &facts(["Cargo.toml"]),
            path,
            None,
            HostTools::default(),
            Path::new("git"),
            None
        )
        .is_empty());
        let problems: Vec<_> = requirement_problems(
            &facts(["ship.blend"]),
            path,
            None,
            HostTools::default(),
            Path::new("git"),
            None,
        )
        .into_iter()
        .filter(|p| p.starts_with("blender "))
        .collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("blender-artist"), "{}", problems[0]);
    }

    fn roster_requiring_godot() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(Teammate {
            name: "godot-test".into(),
            brief_description: "Godot".into(),
            requires: vec![Requirement::Godot],
            ..Teammate::default()
        });
        r
    }

    /// A fake Godot named `name` in its own directory: `--version` prints
    /// `version` and exits with `code`.
    fn fake_godot(name: &str, version: &str, code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(name);
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] || exit 99\n\
                 echo '{version}'\necho 'no display' >&2\nexit {code}\n"
            ),
        )
        .unwrap();
        process::make_executable(&bin).unwrap();
        dir
    }

    fn godot_at(path: &Path, godot_path: Option<&Path>, app: Option<&Path>) -> Option<String> {
        godot_problem(
            Some(path.as_os_str()),
            None,
            godot_path.map(Path::as_os_str),
            app,
        )
    }

    #[test]
    fn godot_on_path_is_fine() {
        let dir = fake_godot("godot", "4.7.2.stable.official.e4f5a6b7c", 0);
        assert_eq!(godot_at(dir.path(), None, None), None);
    }

    /// `GODOT_PATH` wins over PATH: a broken one is reported even with a
    /// good `godot` on PATH, and a good one works with nothing on PATH.
    #[test]
    fn godot_path_wins_over_path() {
        let good = fake_godot("godot", "4.7.2.stable.official.e4f5a6b7c", 0);
        let broken = fake_godot("Godot", "4.7.2.stable.official.e4f5a6b7c", 3);
        let problem = godot_at(good.path(), Some(&broken.path().join("Godot")), None).unwrap();
        assert!(problem.contains("--version` failed"), "{problem}");
        assert!(problem.contains("no display"), "{problem}");

        let empty = tempfile::tempdir().unwrap();
        let fine = good.path().join("godot");
        assert_eq!(godot_at(empty.path(), Some(&fine), None), None);

        let gone = empty.path().join("Godot");
        let problem = godot_at(good.path(), Some(&gone), None).unwrap();
        assert!(problem.contains("could not run"), "{problem}");
    }

    /// The app bundle is the last place looked: used when PATH has no
    /// `godot`, ignored when PATH has one, skipped when it does not exist.
    #[test]
    fn the_app_bundle_is_the_last_fallback() {
        let empty = tempfile::tempdir().unwrap();
        let app = fake_godot("Godot", "4.3.stable.official.77dcf97d8", 0);
        let app_bin = app.path().join("Godot");
        assert_eq!(godot_at(empty.path(), None, Some(&app_bin)), None);

        let old_app = fake_godot("Godot", "4.2.2.stable.official.15073afe3", 0);
        let old_bin = old_app.path().join("Godot");
        let problem = godot_at(empty.path(), None, Some(&old_bin)).unwrap();
        assert!(problem.contains("Godot 4.2"), "{problem}");

        let on_path = fake_godot("godot", "4.7.2.stable.official.e4f5a6b7c", 0);
        assert_eq!(godot_at(on_path.path(), None, Some(&old_bin)), None);

        let missing = empty.path().join("Godot.app/Contents/MacOS/Godot");
        let problem = godot_at(empty.path(), None, Some(&missing)).unwrap();
        assert!(problem.contains("godot not found"), "{problem}");
    }

    #[test]
    fn missing_godot_names_both_fixes() {
        let empty = tempfile::tempdir().unwrap();
        let problems = requirement_problems(
            &roster_requiring_godot(),
            Some(empty.path().as_os_str()),
            None,
            HostTools {
                godot_path: Some(empty.path().join("nope").as_os_str()),
                ..HostTools::default()
            },
            Path::new("git"),
            None,
        );
        let problems: Vec<_> = problems
            .into_iter()
            .filter(|p| p.starts_with("godot "))
            .collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("(needed by "), "{}", problems[0]);
        assert!(problems[0].contains("godot-test"), "{}", problems[0]);

        let problem = godot_at(empty.path(), None, None).unwrap();
        assert!(problem.contains("godot not found"), "{problem}");
        assert!(problem.contains("GODOT_PATH"), "{problem}");
        assert!(problem.contains("4.3 or later"), "{problem}");
    }

    #[test]
    fn godot_below_the_floor_or_without_a_version_is_reported() {
        let old = fake_godot("godot", "4.2.2.stable.official.15073afe3", 0);
        let problem = godot_at(old.path(), None, None).unwrap();
        assert!(problem.contains("Godot 4.2"), "{problem}");
        assert!(problem.contains("4.3 or later"), "{problem}");

        let floor = fake_godot("godot", "4.3.stable.official.77dcf97d8", 0);
        assert_eq!(godot_at(floor.path(), None, None), None);

        let odd = fake_godot("godot", "hello", 0);
        let problem = godot_at(odd.path(), None, None).unwrap();
        assert!(problem.contains("printed no Godot version"), "{problem}");
    }

    #[test]
    fn godot_version_reads_the_dotted_line() {
        assert_eq!(
            godot_version("4.7.2.stable.official.e4f5a6b7c\n"),
            Some((4, 7))
        );
        assert_eq!(godot_version("4.3.stable.mono.official.x\n"), Some((4, 3)));
        assert_eq!(godot_version("warning: x\n5.0.dev1.custom\n"), Some((5, 0)));
        assert_eq!(godot_version("3.6.stable\n"), Some((3, 6)));
        assert_eq!(godot_version("Godot Engine\n"), None);
    }

    fn roster_requiring_git_lfs() -> Roster {
        let mut r = Roster::builtin().unwrap();
        r.insert_for_test(Teammate {
            name: "lfs-test".into(),
            brief_description: "LFS".into(),
            requires: vec![Requirement::GitLfs],
            ..Teammate::default()
        });
        r
    }

    /// A fake `git` in its own directory: `lfs version` exits with `code`.
    fn fake_git(code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("git");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = 'lfs version' ] || exit 99\n\
                 echo 'git-lfs/3.7.0'\necho \"git: 'lfs' is not a git command.\" >&2\nexit {code}\n"
            ),
        )
        .unwrap();
        process::make_executable(&bin).unwrap();
        dir
    }

    /// The git-lfs lines for `git` (looked up on `path` when bare) in
    /// `project`.
    fn lfs_problems(path: &Path, git: &Path, project: Option<&Path>) -> Vec<String> {
        requirement_problems(
            &roster_requiring_git_lfs(),
            Some(path.as_os_str()),
            None,
            HostTools::default(),
            git,
            project,
        )
        .into_iter()
        .filter(|p| p.starts_with("git-lfs "))
        .collect()
    }

    /// U-17: `git lfs version` fails when Git LFS is not installed.
    #[test]
    fn missing_git_lfs_is_reported() {
        let dir = fake_git(1);
        let problems = lfs_problems(dir.path(), Path::new("git"), None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("lfs-test"), "{}", problems[0]);
        assert!(
            problems[0].contains("lfs version` failed"),
            "{}",
            problems[0]
        );
        assert!(problems[0].contains("not a git command"), "{}", problems[0]);
        assert!(problems[0].contains("git lfs install"), "{}", problems[0]);

        let fine = fake_git(0);
        assert!(lfs_problems(fine.path(), Path::new("git"), None).is_empty());

        let empty = tempfile::tempdir().unwrap();
        let problems = lfs_problems(empty.path(), Path::new("git"), None);
        assert!(problems[0].contains("git not found"), "{problems:?}");
    }

    /// U-17: a git path (`HORCH_GIT_BIN`) is run as given, not looked up.
    #[test]
    fn git_lfs_uses_the_fleet_git() {
        let good = fake_git(0);
        let broken = fake_git(1);
        let bin = broken.path().join("git");
        let problems = lfs_problems(good.path(), &bin, None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("lfs version` failed"),
            "{}",
            problems[0]
        );

        let empty = tempfile::tempdir().unwrap();
        assert!(lfs_problems(empty.path(), &good.path().join("git"), None).is_empty());
    }

    /// U-17: an Unreal project needs an LFS rule for `*.uasset` in
    /// `.gitattributes`; any other project does not.
    #[test]
    fn an_unreal_project_needs_an_lfs_rule_for_uasset() {
        let git = fake_git(0);
        let project = tempfile::tempdir().unwrap();
        let dir = project.path();
        let check = || lfs_problems(git.path(), Path::new("git"), Some(dir));
        assert!(check().is_empty(), "not an Unreal project");

        std::fs::write(dir.join("Game.uproject"), "{}").unwrap();
        let problems = check();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(".gitattributes has no Git LFS rule"),
            "{}",
            problems[0]
        );
        assert!(problems[0].contains("git lfs track"), "{}", problems[0]);

        std::fs::write(
            dir.join(".gitattributes"),
            "*.uasset binary\n# *.uasset filter=lfs\n",
        )
        .unwrap();
        assert_eq!(check().len(), 1, "a rule without filter=lfs, or a comment");

        std::fs::write(
            dir.join(".gitattributes"),
            "*.umap filter=lfs diff=lfs merge=lfs -text lockable\n\
             *.uasset filter=lfs diff=lfs merge=lfs -text lockable\n",
        )
        .unwrap();
        assert!(check().is_empty(), "{:?}", check());
    }

    /// U-17: every builder of the Unreal team needs Git LFS; the reviewer,
    /// which neither builds nor commits, does not.
    #[test]
    fn the_unreal_builders_need_git_lfs() {
        let empty = tempfile::tempdir().unwrap();
        let roster = Roster::builtin().unwrap().with_project_facts(
            horch_core::roster::ProjectFacts::from_names(["Game.uproject"]),
        );
        let problems: Vec<_> = requirement_problems(
            &roster,
            Some(empty.path().as_os_str()),
            None,
            HostTools::default(),
            Path::new("git"),
            None,
        )
        .into_iter()
        .filter(|p| p.starts_with("git-lfs "))
        .collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        for seat in ["ue-gameplay-engineer", "ue-tech-lead", "blender-artist"] {
            assert!(problems[0].contains(seat), "{seat}: {}", problems[0]);
        }
        assert!(!problems[0].contains("ue-code-reviewer"), "{}", problems[0]);
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
