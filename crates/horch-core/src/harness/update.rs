//! The agent CLI update step: `horch update`, and the step `horch fleet` and
//! `horch orchestration` run before their first pane starts (ARC-32 to
//! ARC-35).
//!
//! Each CLI has its own update command, and each command finds its own
//! install method (native, Homebrew, npm), so horch only runs it. The panes
//! themselves keep their auto-update switched off, so a fleet runs 1 build of
//! each CLI from start to end.
//!
//! herdr is not in the default list: `herdr update` can stop the herdr
//! server, and that closes every pane. [`run_herdr_update`] runs it only on
//! the operator's request, attached to the operator's terminal.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::HarnessKind;
use crate::routing::quota_probe::probe_version;
use crate::runtime::{process, RuntimeContext};

/// Every agent CLI the update step updates, in the order of its results.
pub const ORDER: [HarnessKind; 6] = [
    HarnessKind::Claude,
    HarnessKind::Codex,
    HarnessKind::OpenCode,
    HarnessKind::Antigravity,
    HarnessKind::Pi,
    HarnessKind::Prime,
];

/// The longest failure reason, in characters.
const REASON_MAX: usize = 200;

/// How long an updater's output pipes can stay open after it exits. A
/// process it started in the background can hold them.
const PIPE_GRACE: Duration = Duration::from_secs(5);

/// What the update step did for 1 harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The binary is not on `PATH`. Nothing ran.
    NotInstalled,
    /// The updater exited 0. The first `--version` line before and after it.
    Updated {
        before: Option<String>,
        after: Option<String>,
    },
    /// The updater failed or timed out: why.
    Failed { reason: String },
}

/// The update step's result for 1 harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateResult {
    pub harness: HarnessKind,
    /// The program that ran, when it was found.
    pub bin: Option<PathBuf>,
    pub outcome: Outcome,
}

impl UpdateResult {
    /// The updater failed or timed out.
    pub fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Failed { .. })
    }
}

/// The arguments of `kind`'s own update command. `None` for the agentless
/// smoke harness.
pub fn update_args(kind: HarnessKind) -> Option<&'static [&'static str]> {
    match kind {
        HarnessKind::Claude => Some(&["update"]),
        HarnessKind::Codex => Some(&["update"]),
        HarnessKind::OpenCode => Some(&["upgrade"]),
        HarnessKind::Antigravity => Some(&["update"]),
        HarnessKind::Pi => Some(&["update", "--self"]),
        HarnessKind::Prime => Some(&["update"]),
        HarnessKind::None => None,
    }
}

/// Update every harness in `harnesses` at the same time, each within
/// `ctx.settings.update_timeout`. The results come in [`ORDER`], 1 per
/// harness, whatever the order of `harnesses`.
pub fn update_all(ctx: &RuntimeContext, harnesses: &[HarnessKind]) -> Vec<UpdateResult> {
    let timeout = ctx.settings.update_timeout;
    let wanted: Vec<HarnessKind> = ORDER
        .iter()
        .copied()
        .filter(|k| harnesses.contains(k))
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = wanted
            .iter()
            .map(|&kind| (kind, s.spawn(move || update_one(ctx, kind, timeout))))
            .collect();
        handles
            .into_iter()
            .map(|(kind, handle)| {
                handle.join().unwrap_or_else(|_| UpdateResult {
                    harness: kind,
                    bin: None,
                    outcome: Outcome::Failed {
                        reason: "the updater thread panicked".into(),
                    },
                })
            })
            .collect()
    })
}

fn update_one(ctx: &RuntimeContext, kind: HarnessKind, timeout: Duration) -> UpdateResult {
    let not_installed = UpdateResult {
        harness: kind,
        bin: None,
        outcome: Outcome::NotInstalled,
    };
    let (Some(args), Some(launched)) = (update_args(kind), kind.binary(&ctx.bins.harness)) else {
        return not_installed;
    };
    let Some(bin) = resolve(ctx, &update_bin(kind, &launched)) else {
        return not_installed;
    };
    let before = probe_version(&bin).version();
    let mut cmd = Command::new(&bin);
    cmd.args(args);
    let outcome = match run_updater(cmd, timeout) {
        Ok(()) => Outcome::Updated {
            before,
            after: probe_version(&bin).version(),
        },
        Err(reason) => Outcome::Failed { reason },
    };
    UpdateResult {
        harness: kind,
        bin: Some(bin),
        outcome,
    }
}

/// The program that updates `kind`, given the one horch launches. `cpx` is a
/// proxy that launches `claude` and has no `update` command, so a `cpx`
/// launch updates `claude` from `PATH`.
pub fn update_bin(kind: HarnessKind, launched: &Path) -> PathBuf {
    let is_cpx = launched.file_stem().is_some_and(|s| s == "cpx");
    if kind == HarnessKind::Claude && is_cpx {
        PathBuf::from("claude")
    } else {
        launched.to_path_buf()
    }
}

/// The binary a name or path stands for, when it exists.
fn resolve(ctx: &RuntimeContext, bin: &Path) -> Option<PathBuf> {
    if bin.components().count() > 1 || bin.is_absolute() {
        return bin.is_file().then(|| bin.to_path_buf());
    }
    process::which(
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
        &bin.to_string_lossy(),
    )
}

/// Run 1 updater with stdin null, both pipes captured and no forbidden
/// variable, in its own process group. At `timeout` the whole group is
/// killed, so a `brew` or `npm` child does not outlive it. `Err` is the
/// failure reason.
fn run_updater(mut cmd: Command, timeout: Duration) -> Result<(), String> {
    process::scrub_child_env(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .spawn()
        .map_err(|e| cut(&format!("{name} did not start: {e}")))?;
    let out = read_on_thread(child.stdout.take());
    let err = read_on_thread(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                kill_group(&mut child);
                let _ = child.wait();
                return Err(timeout_reason(timeout));
            }
            Err(e) => {
                kill_group(&mut child);
                let _ = child.wait();
                return Err(cut(&format!("waiting for {name}: {e}")));
            }
        }
    };
    if status.success() {
        return Ok(());
    }
    let text = |rx: mpsc::Receiver<String>| rx.recv_timeout(PIPE_GRACE).unwrap_or_default();
    let (out, err) = (text(out), text(err));
    Err(reason_line(&err)
        .or_else(|| reason_line(&out))
        .unwrap_or_else(|| exited(&name, status)))
}

/// `timed out after <N> s`.
fn timeout_reason(timeout: Duration) -> String {
    format!("timed out after {} s", timeout.as_secs())
}

/// `<name> exited <code>` for a failure that printed nothing.
fn exited(name: &str, status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("{name} exited {code}"),
        None => format!("{name} exited on a signal"),
    }
}

/// Read `pipe` to its end on a new thread. The thread is never joined: a
/// process the child started can hold the pipe open.
fn read_on_thread(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut bytes);
        }
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    rx
}

#[cfg(unix)]
fn kill_group(child: &mut Child) {
    // The group id is the child's pid (`process_group(0)`). The child is not
    // reaped yet, so its pid still names its group. A group that is already
    // gone gives ESRCH, which is fine.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(child: &mut Child) {
    let _ = child.kill();
}

/// The failure reason in `text`: its last non-empty line, with ANSI escape
/// codes removed, cut to 200 characters. A carriage return starts a new
/// line, so a spinner's last frame is a line of its own.
pub fn reason_line(text: &str) -> Option<String> {
    let plain = strip_ansi(text);
    plain
        .split(['\n', '\r'])
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(cut)
}

fn cut(line: &str) -> String {
    match line.char_indices().nth(REASON_MAX) {
        Some((at, _)) => line[..at].to_string(),
        None => line.to_string(),
    }
}

/// `text` without ANSI escape sequences: CSI (`ESC [ ... final`), OSC
/// (`ESC ] ... BEL` or `ESC ] ... ESC \`) and 2-character escapes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates, then 1 final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Run `<herdr> update` attached to the operator's terminal, with no time
/// limit: herdr can ask the operator whether to stop its server. The exit
/// status is herdr's.
pub fn run_herdr_update(ctx: &RuntimeContext) -> Result<ExitStatus> {
    let bin = &ctx.bins.harness.herdr;
    let mut cmd = Command::new(bin);
    cmd.arg("update");
    process::scrub_child_env(&mut cmd);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("running {} update", bin.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_33_strip_ansi_removes_csi_osc_and_short_escapes() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("\u{1b}[2K\u{1b}[1Gdone"), "done");
        assert_eq!(strip_ansi("\u{1b}[?25lhidden\u{1b}[?25h"), "hidden");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(strip_ansi("\u{1b}]8;;url\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("\u{1b}7saved\u{1b}8"), "saved");
        assert_eq!(strip_ansi("plain ü text"), "plain ü text");
    }

    #[test]
    fn arc_33_the_reason_is_the_last_plain_line_cut_to_200_characters() {
        let spinner = "\u{1b}[?25l⠋ Upgrading\r\u{1b}[2K⠙ Upgrading\r\u{1b}[2K\
                       \u{1b}[31mError: network down\u{1b}[0m\n\n  \n";
        assert_eq!(reason_line(spinner).as_deref(), Some("Error: network down"));
        assert_eq!(reason_line("first\nlast\n").as_deref(), Some("last"));
        assert_eq!(reason_line(" \n\u{1b}[0m\n"), None);
        assert_eq!(reason_line(""), None);

        let long = "é".repeat(250);
        let reason = reason_line(&long).unwrap();
        assert_eq!(reason.chars().count(), 200);
    }

    #[test]
    fn arc_32_update_bin_maps_cpx_to_claude() {
        let claude = HarnessKind::Claude;
        assert_eq!(
            update_bin(claude, Path::new("cpx")),
            PathBuf::from("claude")
        );
        assert_eq!(
            update_bin(claude, Path::new("/opt/proxy/bin/cpx")),
            PathBuf::from("claude")
        );
        assert_eq!(
            update_bin(claude, Path::new("/opt/bin/claude")),
            PathBuf::from("/opt/bin/claude")
        );
        // Only claude has the proxy.
        assert_eq!(
            update_bin(HarnessKind::Codex, Path::new("cpx")),
            PathBuf::from("cpx")
        );
    }

    #[test]
    fn arc_32_every_agent_cli_has_its_update_command() {
        let args: Vec<_> = ORDER.iter().map(|k| update_args(*k).unwrap()).collect();
        assert_eq!(
            args,
            [
                &["update"][..],
                &["update"],
                &["upgrade"],
                &["update"],
                &["update", "--self"],
                &["update"],
            ]
        );
        assert_eq!(update_args(HarnessKind::None), None);
        assert_eq!(
            timeout_reason(Duration::from_millis(5000)),
            "timed out after 5 s"
        );
    }
}
