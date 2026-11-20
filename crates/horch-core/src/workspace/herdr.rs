//! Typed wrapper around the `herdr` CLI, replacing the shell's `herdr ... | jq`
//! pipelines.
//!
//! Verified against herdr 0.6.1 and re-checked against 0.8.0. The splitting and
//! messaging surface is deliberately limited to what 0.6.1 advertises: no `pane
//! current`, and no `--env` or `--ratio` on `pane split`.
//!
//! The tab and `pane move` calls that `horch tile` needs arrived later and are
//! verified against 0.8.2. They
//! are additions, so a caller that never tiles still only needs 0.6.1.

use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::workspace::exec::{is_timeout, output_within};
use crate::workspace::model::*;

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct PaneResult {
    pane: Pane,
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<Pane>,
}

#[derive(Debug, Deserialize)]
struct LayoutResult {
    layout: Layout,
}

#[derive(Debug, Deserialize)]
struct ResizeResult {
    resize: Resize,
}

#[derive(Debug, Deserialize)]
struct TabListResult {
    tabs: Vec<Tab>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct FocusResult {
    focus: Focus,
}

#[derive(Debug, Deserialize)]
struct MoveResult {
    move_result: Move,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreateResult {
    workspace: Workspace,
    root_pane: Pane,
}

/// A `herdr` command that exited non-zero, with herdr's own stderr, which is
/// what actually explains a failure. Typed so the CLI can tell a herdr failure
/// from any other without reading the message (ARC-22).
#[derive(Debug)]
pub struct HerdrFailed {
    pub command: String,
    pub status: std::process::ExitStatus,
    pub stderr: String,
}

impl fmt::Display for HerdrFailed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self {
            command,
            status,
            stderr,
        } = self;
        write!(f, "herdr {command} failed ({status}): {stderr}")
    }
}

impl std::error::Error for HerdrFailed {}

/// Handle to the `herdr` executable.
#[derive(Debug, Clone)]
pub struct Herdr {
    bin: PathBuf,
    /// When set, a call that has not finished by then is killed and fails.
    timeout: Option<Duration>,
    /// When set, a call still running at this instant is killed and fails.
    deadline: Option<Instant>,
}

impl Herdr {
    /// A handle that runs `bin`. The CLI passes `ctx.bins.harness.herdr`.
    pub fn with_bin(bin: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            timeout: None,
            deadline: None,
        }
    }

    /// The same handle, with the calls that go through `output` and `json`
    /// bounded: a herdr that has not answered after `timeout` is killed and
    /// the call fails. `server_reachable`, `wait_output` and
    /// `integration_status` are not bounded.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The same handle, with the same calls as [`Herdr::with_timeout`] cut
    /// off at `deadline`. With a timeout too, a call stops at the earlier of
    /// the two. `None`: no deadline.
    pub fn until(mut self, deadline: Option<Instant>) -> Self {
        self.deadline = deadline;
        self
    }

    fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> std::io::Result<Output> {
        let mut command = Command::new(&self.bin);
        command.args(args);
        match (self.timeout, self.deadline) {
            (None, None) => command.output(),
            (Some(timeout), deadline) => output_within(command, timeout, deadline),
            (None, Some(deadline)) => output_within(
                command,
                deadline.saturating_duration_since(Instant::now()),
                Some(deadline),
            ),
        }
    }

    /// Run `herdr <args>`, returning stdout. Errors carry herdr's own stderr,
    /// which is what actually explains a failure.
    pub(crate) fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = self.run(args).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("herdr CLI not found on PATH. Install it with `herdr-install`, or see https://herdr.dev/docs/install/")
            } else {
                anyhow!(e)
            }
        })?;
        if !out.status.success() {
            let rendered: Vec<String> = args
                .iter()
                .map(|a| a.as_ref().to_string_lossy().into_owned())
                .collect();
            return Err(HerdrFailed {
                command: rendered.join(" "),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }
            .into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run `herdr <args>` and decode the `{"result": ...}` envelope it prints.
    fn json<T: for<'de> Deserialize<'de>, S: AsRef<OsStr>>(&self, args: &[S]) -> Result<T> {
        let stdout = self.output(args)?;
        let envelope: Envelope<T> = serde_json::from_str(stdout.trim())
            .with_context(|| format!("could not parse herdr response: {}", stdout.trim()))?;
        Ok(envelope.result)
    }

    /// True when the herdr server answers at all. Cheap reachability probe.
    pub fn server_reachable(&self) -> bool {
        Command::new(&self.bin)
            .args(["workspace", "list"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// `herdr pane get <id>`. Accepts the internal id herdr exports as
    /// `HERDR_PANE_ID` (e.g. `p_2`) and upgrades it to the public ids.
    pub fn pane_get(&self, id: &str) -> Result<Pane> {
        let r: PaneResult = self.json(&["pane", "get", id])?;
        Ok(r.pane)
    }

    pub fn pane_list(&self, workspace_id: &str) -> Result<Vec<Pane>> {
        let r: PaneListResult = self.json(&["pane", "list", "--workspace", workspace_id])?;
        Ok(r.panes)
    }

    pub fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        let r: PaneResult = self.json(&[
            "pane",
            "split",
            from,
            "--direction",
            direction.as_str(),
            "--no-focus",
        ])?;
        Ok(r.pane.pane_id)
    }

    /// `herdr pane run` submits text and Enter atomically. Safe for launching a
    /// command into a fresh shell; not safe for a TUI in raw mode, which is why
    /// [`crate::messaging::delivery::send_line`] exists.
    pub fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        self.output(&["pane", "run", pane, command])?;
        Ok(())
    }

    pub fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.output(&["pane", "send-text", pane, text])?;
        Ok(())
    }

    pub fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        self.output(&["pane", "send-keys", pane, keys])?;
        Ok(())
    }

    pub fn pane_close(&self, pane: &str) -> Result<()> {
        self.output(&["pane", "close", pane])?;
        Ok(())
    }

    /// True when the pane still exists.
    pub fn pane_exists(&self, pane: &str) -> bool {
        self.pane_get(pane).is_ok()
    }

    pub fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        self.output(&["pane", "read", pane, "--source", source])
    }

    pub fn pane_layout(&self, pane: Option<&str>) -> Result<Layout> {
        let r: LayoutResult = match pane {
            Some(p) => self.json(&["pane", "layout", "--pane", p])?,
            None => self.json(&["pane", "layout", "--current"])?,
        };
        Ok(r.layout)
    }

    /// `herdr tab list --workspace <ws>`, in tab-strip order.
    ///
    /// Always scoped to a workspace: without `--workspace` herdr returns the tabs
    /// of every workspace it has open.
    pub fn tab_list(&self, workspace_id: &str) -> Result<Vec<Tab>> {
        let r: TabListResult = self.json(&["tab", "list", "--workspace", workspace_id])?;
        let mut tabs = r.tabs;
        tabs.sort_by_key(|t| t.number);
        Ok(tabs)
    }

    /// `herdr` creates tabs by moving an existing pane (`pane_move_new_tab`),
    /// not by opening a fresh empty tab.
    pub fn tab_new(
        &self,
        _workspace_id: &str,
        _label: &str,
        _cwd: Option<&str>,
    ) -> Result<(String, String)> {
        Err(crate::workspace::host::Unsupported {
            call: "tab_new",
            backend: crate::runtime::Backend::Herdr,
        }
        .into())
    }

    /// `herdr workspace list`.
    pub fn workspace_list(&self) -> Result<Vec<Workspace>> {
        let r: WorkspaceListResult = self.json(&["workspace", "list"])?;
        Ok(r.workspaces)
    }

    /// `herdr tab focus <tab>`. The only way to focus a specific pane's tab:
    /// `pane focus` moves to a NEIGHBOUR, not to an id.
    pub(crate) fn tab_focus(&self, tab_id: &str) -> Result<()> {
        self.output(&["tab", "focus", tab_id])?;
        Ok(())
    }

    /// `herdr pane focus --direction <d> --pane <origin>`.
    ///
    /// One step to a neighbour. herdr answers with exit 0 and `changed: false,
    /// reason: "no_neighbor"` when nothing sits on that side, so a caller learns
    /// it has run out of room without an error.
    pub(crate) fn pane_focus(&self, origin: &str, direction: FocusDir) -> Result<Focus> {
        let r: FocusResult = self.json(&[
            "pane",
            "focus",
            "--direction",
            direction.as_str(),
            "--pane",
            origin,
        ])?;
        Ok(r.focus)
    }

    /// Walk the focus of `tab` to `target`, one neighbour at a time.
    ///
    /// Returns false rather than erroring when the focus cannot be placed: the
    /// caller is restoring a view, and a view that cannot be restored must never
    /// fail the operation that moved it.
    ///
    /// WARNING: like `tab focus`, this pulls the whole workspace into view.
    ///
    /// The step budget is the pane count, which is more than any walk needs: each
    /// step closes the larger of the two axis gaps, so the focus reaches any pane
    /// of a two-row grid in at most two moves. The budget only has to stop a walk
    /// that oscillates because herdr picked a different neighbour than the
    /// geometry suggested.
    pub fn pane_focus_walk(&self, tab: &str, target: &str) -> Result<bool> {
        // `pane layout` reports the whole tab that holds the pane it is asked
        // about, so the target doubles as the probe for its own tab.
        let layout = self.pane_layout(Some(target))?;
        if layout.tab_id != tab {
            Ok(false)
        } else {
            self.walk(layout, target)
        }
    }

    /// The loop of [`Herdr::pane_focus_walk`] inside the target's tab: at most
    /// 1 step per pane of `layout`.
    fn walk(&self, mut layout: Layout, target: &str) -> Result<bool> {
        for _ in 0..layout.panes.len() {
            // `Arrived` and `Stuck` both end the walk; the check below tells
            // them apart.
            let Walk::Step { from, direction } = next_step(&layout, target) else {
                break;
            };
            let step = self.pane_focus(&from, direction)?;
            if stalled(&step, &from) {
                return Ok(false);
            }
            layout = step.layout;
        }
        Ok(layout.focused_pane_id.as_deref() == Some(target))
    }

    /// `herdr pane move <pane> --tab <tab> --split <d> [--target-pane <target>]
    /// [--ratio <r>] --no-focus`.
    ///
    /// Keeps the pane id and the running process (verified against herdr 0.8.2).
    /// `ratio` is the share
    /// the TARGET pane keeps, so the moved pane gets `1 - ratio`.
    ///
    /// A move into the tab the pane already occupies is refused with
    /// `changed: false, reason: "same_tab"`: repositioning a pane inside its own
    /// tab means moving it out and back.
    pub(crate) fn pane_move(
        &self,
        pane: &str,
        tab: &str,
        direction: Direction,
        target: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Move> {
        let mut args: Vec<String> = vec![
            "pane".into(),
            "move".into(),
            pane.into(),
            "--tab".into(),
            tab.into(),
            "--split".into(),
            direction.as_str().into(),
        ];
        if let Some(target) = target {
            args.push("--target-pane".into());
            args.push(target.into());
        }
        if let Some(ratio) = ratio {
            args.push("--ratio".into());
            // Match herdr's own four-decimal storage of split ratios.
            args.push(format!("{ratio:.4}"));
        }
        args.push("--no-focus".into());
        let r: MoveResult = self.json(&args)?;
        Ok(r.move_result)
    }

    /// `herdr pane move <pane> --new-tab --label <label> --no-focus`.
    ///
    /// The new tab holds only the moved pane, and its id comes back as
    /// `created_tab`. This form takes neither `--split` nor `--ratio`; the label
    /// flag is `--label`, not `--tab-label`.
    pub(crate) fn pane_move_new_tab(&self, pane: &str, label: &str) -> Result<Move> {
        let r: MoveResult = self.json(&[
            "pane",
            "move",
            pane,
            "--new-tab",
            "--label",
            label,
            "--no-focus",
        ])?;
        Ok(r.move_result)
    }

    /// Move the divider on `direction` side of `pane` by `amount`, a fraction of
    /// the split container that owns it.
    ///
    /// The reply carries the post-resize layout, so a caller adjusting several
    /// dividers can re-plan against real geometry instead of predicting it. It
    /// also carries `changed`, which goes false when herdr refuses the move -
    /// that is how a caller discovers the minimum pane width rather than
    /// hard-coding one.
    pub(crate) fn pane_resize(&self, pane: &str, direction: &str, amount: f64) -> Result<Resize> {
        // herdr stores split ratios to four decimal places; matching that here
        // keeps the request and what it can actually honour in step.
        let amount = format!("{amount:.4}");
        let r: ResizeResult = self.json(&[
            "pane",
            "resize",
            "--pane",
            pane,
            "--direction",
            direction,
            "--amount",
            &amount,
        ])?;
        Ok(r.resize)
    }

    pub fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        let mut args: Vec<&str> = vec!["workspace", "create", "--label", label];
        if let Some(cwd) = cwd {
            args.push("--cwd");
            args.push(cwd);
        }
        if !focus {
            args.push("--no-focus");
        }
        let r: WorkspaceCreateResult = self.json(&args)?;
        Ok(NewWorkspace {
            workspace_id: r.workspace.workspace_id,
            tab_id: r.root_pane.tab_id.clone(),
            root_pane_id: r.root_pane.pane_id,
        })
    }

    pub fn workspace_close(&self, workspace_id: &str) -> Result<()> {
        self.output(&["workspace", "close", workspace_id])?;
        Ok(())
    }

    /// Show an operator notification through the existing herdr boundary.
    pub fn notification_show(&self, title: &str, body: &str) -> Result<()> {
        self.output(&["notification", "show", title, "--body", body])?;
        Ok(())
    }

    /// `herdr pane wait-output <pane> --match <needle> --source recent-unwrapped
    /// --timeout <ms>`. Returns false on timeout rather than erroring.
    ///
    /// herdr 0.8.2 has no top-level `wait`: the old `herdr wait output` exits 2
    /// with `unknown command: wait`, which read as a timeout and failed every
    /// `horch smoke messaging`. Any failure other than herdr's `timeout` error
    /// is now an error. The source is the one the smoke failure dump prints.
    pub fn wait_output(&self, pane: &str, needle: &str, timeout_ms: u64) -> Result<bool> {
        let timeout = timeout_ms.to_string();
        let args = [
            "pane",
            "wait-output",
            pane,
            "--match",
            needle,
            "--source",
            "recent-unwrapped",
            "--timeout",
            &timeout,
        ];
        let out = Command::new(&self.bin)
            .args(args)
            .output()
            .context("running herdr pane wait-output")?;
        wait_outcome(out.status.success(), &String::from_utf8_lossy(&out.stderr))
            .with_context(|| format!("herdr {} failed ({})", args.join(" "), out.status))
    }

    /// `herdr integration status`, or None when the subcommand is unavailable.
    pub fn integration_status(&self) -> Option<String> {
        Command::new(&self.bin)
            .args(["integration", "status"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// `herdr agent prompt <target> <text>`: herdr writes the text as one
    /// bracketed paste plus an encoded Enter, atomically, even while the agent
    /// is mid-turn. Fails when herdr sees no agent in the pane.
    pub fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
        self.output(&["agent", "prompt", pane, text])?;
        Ok(())
    }

    /// `herdr pane report-agent <pane> --source <source> --agent <agent>
    /// --state <state> --seq <seq>` (herdr 0.8.2): `state` is `idle`,
    /// `working`, `blocked` or `unknown`. herdr keeps the report with the
    /// highest `seq` of a source, so a report with a lower one is stale.
    /// For a harness herdr does not detect itself (Prime). The pane id
    /// comes first: herdr 0.8.2 answers `unknown option: <source>` when it
    /// comes last, as its `--help` shows it (live check X2, 2026-10-07).
    pub fn report_agent(
        &self,
        pane: &str,
        source: &str,
        agent: &str,
        state: &str,
        seq: u64,
    ) -> Result<()> {
        let seq = seq.to_string();
        self.output(&[
            "pane",
            "report-agent",
            pane,
            "--source",
            source,
            "--agent",
            agent,
            "--state",
            state,
            "--seq",
            &seq,
        ])?;
        Ok(())
    }

    /// `herdr pane release-agent <pane> --source <source> --agent <agent>
    /// --seq <seq>`: the end of [`Herdr::report_agent`]'s reports, so herdr
    /// stops showing that agent in the pane.
    pub fn release_agent(&self, pane: &str, source: &str, agent: &str, seq: u64) -> Result<()> {
        let seq = seq.to_string();
        self.output(&[
            "pane",
            "release-agent",
            pane,
            "--source",
            source,
            "--agent",
            agent,
            "--seq",
            &seq,
        ])?;
        Ok(())
    }
}

/// What a focus walk does next, read from the layout alone.
#[derive(Debug, PartialEq, Eq)]
enum Walk {
    /// The target has the focus.
    Arrived,
    /// No focus, a pane missing from the layout, or no direction leads on.
    Stuck,
    /// Move the focus from pane `from` toward `direction`.
    Step { from: String, direction: FocusDir },
}

/// The next move of a walk of the focus of `layout` to `target`.
fn next_step(layout: &Layout, target: &str) -> Walk {
    let Some(from) = layout.focused_pane_id.as_deref() else {
        return Walk::Stuck;
    };
    if from == target {
        Walk::Arrived
    } else {
        step_between(layout, from, target).map_or(Walk::Stuck, |direction| Walk::Step {
            from: from.to_owned(),
            direction,
        })
    }
}

/// The direction of 1 step from pane `from` toward pane `to` of `layout`.
/// `None` when either pane is not in the layout, or no direction leads on.
fn step_between(layout: &Layout, from: &str, to: &str) -> Option<FocusDir> {
    let rect = |id: &str| {
        layout
            .panes
            .iter()
            .find(|p| p.pane_id == id)
            .map(|p| p.rect)
    };
    crate::workspace::tile::step_toward(&rect(from)?, &rect(to)?)
}

/// `no_neighbor`, or a step that landed back on `from`: either way the walk
/// is not converging, so it stops instead of spinning.
fn stalled(step: &Focus, from: &str) -> bool {
    !step.changed || step.focused_pane_id.as_deref() == Some(from)
}

/// Read a `herdr pane wait-output` result: success is a match; herdr's
/// `{"error":{"code":"timeout",...}}` on stderr is no match; anything else
/// (a missing pane, an unknown command) is an error.
fn wait_outcome(success: bool, stderr: &str) -> Result<bool> {
    if success {
        Ok(true)
    } else if is_timeout(stderr) {
        Ok(false)
    } else {
        Err(anyhow!("{}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake herdr that runs `body`, already run once. macOS scans a new
    /// executable at its first exec for seconds (up to 35 s under load),
    /// which would race every timed call. The warm run exits before `body`.
    #[cfg(unix)]
    fn script_in(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("herdr");
        let warm_guard = "if [ \"$1\" = warm ]; then\n  exit 0\nfi";
        let warm = crate::test_support::write_warmed(
            &script,
            &format!("#!/bin/sh\n{warm_guard}\n{body}\n"),
            &["warm"],
        );
        assert!(warm.status.success(), "warming {}", script.display());
        (dir, script)
    }

    /// The three answers herdr 0.8.2 gives, verbatim.
    #[test]
    fn wait_outcome_tells_a_timeout_from_an_error() {
        assert!(wait_outcome(true, "").unwrap());
        let timeout = r#"{"error":{"code":"timeout","message":"timed out waiting for output match"},"id":"cli:pane:wait-output"}"#;
        assert!(!wait_outcome(false, timeout).unwrap());
        let missing = r#"{"error":{"code":"pane_not_found","message":"pane w999:p9 not found"},"id":"cli:pane:wait-output"}"#;
        assert!(wait_outcome(false, missing).is_err());
        let unknown = "unknown command: wait\nrun 'herdr --help' for usage";
        let err = wait_outcome(false, unknown).unwrap_err().to_string();
        assert!(err.contains("unknown command: wait"), "{err}");
    }

    #[test]
    fn parses_pane_get_envelope() {
        let raw = r#"{"result":{"pane":{"pane_id":"w1-3","workspace_id":"w1"}}}"#;
        let r: Envelope<PaneResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.pane.pane_id, "w1-3");
        assert_eq!(r.result.pane.workspace_id.as_deref(), Some("w1"));
        assert_eq!(r.result.pane.agent_session_id(), None);
    }

    #[test]
    fn parses_workspace_create_envelope() {
        let raw = r#"{"result":{"workspace":{"workspace_id":"w7"},
                                 "root_pane":{"pane_id":"w7-1"}}}"#;
        let r: Envelope<WorkspaceCreateResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.workspace.workspace_id, "w7");
        assert_eq!(r.result.root_pane.pane_id, "w7-1");
    }

    /// herdr has reported the agent session under several shapes across
    /// versions; all of them must resolve.
    #[test]
    fn agent_session_id_handles_every_known_shape() {
        let cases = [
            (r#""abc-123""#, Some("abc-123")),
            (r#"{"session_id":"s1"}"#, Some("s1")),
            (r#"{"id":"s2"}"#, Some("s2")),
            (r#"{"ref":"s3"}"#, Some("s3")),
            (r#"{"other":"s4"}"#, None),
            (r#"null"#, None),
            (r#""""#, None),
        ];
        for (json, expected) in cases {
            let pane: Pane =
                serde_json::from_str(&format!(r#"{{"pane_id":"p","agent_session":{json}}}"#))
                    .unwrap();
            assert_eq!(pane.agent_session_id().as_deref(), expected, "for {json}");
        }
    }

    /// The real 0.8.2 reply, trimmed. Two fields are conditional and must not be
    /// required: `source_layout` is absent when the move emptied and so closed the
    /// source tab, and `created_tab` is present only for `--new-tab`.
    #[test]
    fn parses_a_pane_move_that_closed_its_source_tab() {
        let raw = r#"{"result":{"type":"pane_move","move_result":{
            "changed":true,
            "pane":{"pane_id":"w19:p3","tab_id":"w19:t3","workspace_id":"w19"},
            "previous_pane_id":"w19:p3","previous_tab_id":"w19:t1",
            "created_tab":{"tab_id":"w19:t3","workspace_id":"w19","label":"workers 2",
                           "number":3,"pane_count":1},
            "focused_pane_id":"w19:p3"}}}"#;
        let r: Envelope<MoveResult> = serde_json::from_str(raw).unwrap();
        let m = r.result.move_result;
        assert!(m.changed);
        assert_eq!(m.pane.tab_id.as_deref(), Some("w19:t3"));
        assert_eq!(m.created_tab.unwrap().tab_id, "w19:t3");
        assert!(m.source_layout.is_none(), "the source tab was closed");
        assert!(m.reason.is_none());
    }

    /// herdr declines a move rather than failing the call, so the reason is the
    /// only way a caller learns that nothing happened.
    #[test]
    fn parses_a_declined_pane_move_with_its_reason() {
        let raw = r#"{"result":{"move_result":{"changed":false,"reason":"same_tab",
            "pane":{"pane_id":"w1:p2"}}}}"#;
        let r: Envelope<MoveResult> = serde_json::from_str(raw).unwrap();
        assert!(!r.result.move_result.changed);
        assert_eq!(r.result.move_result.reason.as_deref(), Some("same_tab"));
    }

    #[test]
    fn parses_a_tab_list_and_keeps_tab_strip_order() {
        let raw = r#"{"result":{"type":"tab_list","tabs":[
            {"tab_id":"w0:t2","workspace_id":"w0","label":"workers 2","number":2,"pane_count":6},
            {"tab_id":"w0:t1","workspace_id":"w0","label":"1","number":1,"pane_count":5}]}}"#;
        let r: Envelope<TabListResult> = serde_json::from_str(raw).unwrap();
        let mut tabs = r.result.tabs;
        tabs.sort_by_key(|t| t.number);
        assert_eq!(
            tabs.iter().map(|t| t.tab_id.as_str()).collect::<Vec<_>>(),
            ["w0:t1", "w0:t2"]
        );
    }

    /// A tiler has to know the focused pane to hand focus back, and the zoom
    /// state because herdr refuses to move panes in a zoomed tab.
    #[test]
    fn a_layout_reports_the_focused_pane_and_the_zoom_state() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w0","tab_id":"w0:t1",
            "area":{"x":26,"y":1,"width":184,"height":53},
            "focused_pane_id":"w0:p1","zoomed":true,
            "panes":[{"pane_id":"w0:p1","rect":{"x":26,"y":1,"width":184,"height":53}}],
            "splits":[]}}}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.focused_pane_id.as_deref(), Some("w0:p1"));
        assert!(r.result.layout.zoomed);
    }

    /// The real 0.8.2 `pane focus --direction` reply, trimmed. The walk reads
    /// three things from it: whether the focus moved, where it moved to, and
    /// the layout, which saves a `pane layout` call between steps.
    #[test]
    fn parses_a_pane_focus_reply_with_its_layout() {
        let raw = r#"{"result":{"type":"pane_focus_direction","focus":{
            "changed":true,"focused_pane_id":"w1R:p2","source_pane_id":"w1R:p1",
            "layout":{"workspace_id":"w1R","tab_id":"w1R:t1",
              "area":{"x":26,"y":1,"width":184,"height":53},
              "focused_pane_id":"w1R:p2",
              "panes":[{"focused":false,"pane_id":"w1R:p1",
                        "rect":{"x":26,"y":1,"width":92,"height":53}},
                       {"focused":true,"pane_id":"w1R:p2",
                        "rect":{"x":118,"y":1,"width":92,"height":27}}],
              "splits":[]}}}}"#;
        let r: Envelope<FocusResult> = serde_json::from_str(raw).unwrap();
        assert!(r.result.focus.changed);
        assert_eq!(r.result.focus.focused_pane_id.as_deref(), Some("w1R:p2"));
        assert_eq!(r.result.focus.layout.panes.len(), 2);
        assert!(r.result.focus.reason.is_none());
    }

    /// At the edge of a tab herdr declines with exit 0 rather than failing, so
    /// `reason` is the only way the walk learns it has run out of room.
    #[test]
    fn parses_a_declined_pane_focus_at_the_edge_of_a_tab() {
        let raw = r#"{"result":{"focus":{"changed":false,"reason":"no_neighbor",
            "focused_pane_id":"w1R:p1",
            "layout":{"workspace_id":"w1R","tab_id":"w1R:t1",
              "area":{"x":26,"y":1,"width":184,"height":53},"panes":[]}}}}"#;
        let r: Envelope<FocusResult> = serde_json::from_str(raw).unwrap();
        assert!(!r.result.focus.changed);
        assert_eq!(r.result.focus.reason.as_deref(), Some("no_neighbor"));
    }

    /// Older herdr omits both, and the wrapper still has to parse the reply.
    #[test]
    fn a_layout_without_focus_or_zoom_still_parses() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w0","tab_id":"w0:t1",
            "area":{"x":0,"y":0,"width":80,"height":24},"panes":[]}}}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.focused_pane_id, None);
        assert!(!r.result.layout.zoomed);
    }

    #[test]
    fn unknown_fields_in_herdr_responses_are_ignored() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w1","tab_id":"t1",
            "area":{"x":0,"y":0,"width":200,"height":50},
            "panes":[{"pane_id":"w1-1","rect":{"x":0,"y":0,"width":50,"height":50},
                      "title":"zsh"}],
            "extra":true}},"jsonrpc":"2.0"}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.panes.len(), 1);
        assert_eq!(r.result.layout.area.width, 200);
    }

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/herdr")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    /// Every wire shape that `Herdr` parses has a JSON sample under
    /// `tests/fixtures/herdr/`; each one parses into the model types.
    #[test]
    fn arc_19_herdr_parsing_contract() {
        let r: Envelope<PaneResult> = serde_json::from_str(&fixture("pane_get.json")).unwrap();
        let pane = r.result.pane;
        assert_eq!(pane.pane_id, "w1:p3");
        assert_eq!(pane.workspace_id.as_deref(), Some("w1"));
        assert_eq!(pane.tab_id.as_deref(), Some("w1:t1"));
        assert_eq!(pane.agent_session_id().as_deref(), Some("s-42"));

        let r: Envelope<PaneListResult> = serde_json::from_str(&fixture("pane_list.json")).unwrap();
        let ids: Vec<&str> = r.result.panes.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, ["w1:p1", "w1:p2"]);
        assert_eq!(r.result.panes[0].agent_session_id(), None);
        assert_eq!(
            r.result.panes[1].agent_session_id().as_deref(),
            Some("abc-123")
        );

        let r: Envelope<LayoutResult> = serde_json::from_str(&fixture("layout.json")).unwrap();
        let layout = r.result.layout;
        assert_eq!(layout.tab_id, "w0:t1");
        assert_eq!(layout.area.width, 184);
        assert_eq!(layout.panes.len(), 2);
        assert_eq!(layout.panes[1].rect.x, 118);
        assert_eq!(layout.focused_pane_id.as_deref(), Some("w0:p2"));
        assert!(!layout.zoomed);

        let r: Envelope<TabListResult> = serde_json::from_str(&fixture("tab_list.json")).unwrap();
        assert_eq!(r.result.tabs.len(), 2);
        assert_eq!(r.result.tabs[0].number, 2);
        assert_eq!(r.result.tabs[0].label.as_deref(), Some("workers 2"));
        assert_eq!(r.result.tabs[1].pane_count, Some(5));

        let r: Envelope<WorkspaceListResult> =
            serde_json::from_str(&fixture("workspace_list.json")).unwrap();
        assert_eq!(
            r.result.workspaces[0].label.as_deref(),
            Some("horch telemetry")
        );
        assert_eq!(
            r.result.workspaces[0].active_tab_id.as_deref(),
            Some("w0:t1")
        );
        assert_eq!(r.result.workspaces[1].workspace_id, "w1");
        assert_eq!(r.result.workspaces[1].label, None);

        let r: Envelope<PaneResult> = serde_json::from_str(&fixture("pane_split.json")).unwrap();
        assert_eq!(r.result.pane.pane_id, "w1:p4");

        let r: Envelope<WorkspaceCreateResult> =
            serde_json::from_str(&fixture("workspace_create.json")).unwrap();
        assert_eq!(r.result.workspace.workspace_id, "w7");
        assert_eq!(r.result.workspace.label.as_deref(), Some("fleet"));
        assert_eq!(r.result.root_pane.pane_id, "w7:p1");
        assert_eq!(r.result.root_pane.tab_id.as_deref(), Some("w7:t1"));
    }

    /// A fake herdr for [`Herdr::pane_focus_walk`]: tab `w1:t1` holds `panes`
    /// (id, x) side by side, `focus` is the focused pane, and `moves` maps
    /// the origin of each `pane focus` call to the pane herdr then focuses.
    /// An origin with no move is `no_neighbor`. Each focus call appends a
    /// line to `calls`.
    #[cfg(unix)]
    fn walk_herdr(
        panes: &[(&str, i64)],
        focus: Option<&str>,
        moves: &[(&str, &str)],
    ) -> (tempfile::TempDir, PathBuf) {
        let panes = panes
            .iter()
            .map(|(id, x)| {
                format!(r#"{{"pane_id":"{id}","rect":{{"x":{x},"y":0,"width":10,"height":10}}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let moves = moves
            .iter()
            .map(|(from, to)| format!("  {from}) to={to};;\n"))
            .collect::<String>();
        let body = format!(
            r#"d=$(dirname "$0")
f=$(cat "$d/focus")
if [ "$f" = none ]; then fj=null; else fj="\"$f\""; fi
layout() {{
  printf '{{"workspace_id":"w1","tab_id":"w1:t1","area":{{"x":0,"y":0,"width":100,"height":10}},"panes":[{panes}],"focused_pane_id":%s}}' "$1"
}}
case "$1 $2" in
"pane layout")
  printf '{{"result":{{"layout":%s}}}}\n' "$(layout "$fj")";;
"pane focus")
  echo "$6 $4" >> "$d/calls"
  to=
  case "$6" in
{moves}  esac
  if [ -z "$to" ]; then
    printf '{{"result":{{"focus":{{"changed":false,"reason":"no_neighbor","focused_pane_id":%s,"layout":%s}}}}}}\n' "$fj" "$(layout "$fj")"
  else
    echo "$to" > "$d/focus"
    printf '{{"result":{{"focus":{{"changed":true,"focused_pane_id":"%s","layout":%s}}}}}}\n' "$to" "$(layout "\"$to\"")"
  fi;;
*) exit 1;;
esac"#
        );
        let (dir, script) = script_in(&body);
        std::fs::write(dir.path().join("focus"), focus.unwrap_or("none")).unwrap();
        (dir, script)
    }

    #[cfg(unix)]
    fn focus_calls(dir: &tempfile::TempDir) -> Vec<String> {
        std::fs::read_to_string(dir.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn layout_of(panes: &[(&str, i64)], focused: Option<&str>) -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            area: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 10,
            },
            panes: panes
                .iter()
                .map(|(id, x)| LayoutPane {
                    pane_id: (*id).into(),
                    rect: Rect {
                        x: *x,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                })
                .collect(),
            focused_pane_id: focused.map(str::to_owned),
            zoomed: false,
        }
    }

    #[test]
    fn next_step_reads_each_stop_reason_from_the_layout() {
        let row = [("p1", 0), ("p2", 10)];
        assert_eq!(next_step(&layout_of(&row, Some("p2")), "p2"), Walk::Arrived);
        assert_eq!(
            next_step(&layout_of(&row, Some("p2")), "p1"),
            Walk::Step {
                from: "p2".into(),
                direction: FocusDir::Left
            }
        );
        assert_eq!(next_step(&layout_of(&row, None), "p2"), Walk::Stuck);
        assert_eq!(next_step(&layout_of(&row, Some("p9")), "p2"), Walk::Stuck);
        assert_eq!(next_step(&layout_of(&row, Some("p1")), "p9"), Walk::Stuck);
        let stacked = [("p1", 0), ("p2", 0)];
        assert_eq!(
            next_step(&layout_of(&stacked, Some("p1")), "p2"),
            Walk::Stuck
        );
    }

    #[cfg(unix)]
    #[test]
    fn pane_focus_walk_steps_to_the_target() {
        let panes = [("p1", 0), ("p2", 10), ("p3", 20)];
        let (dir, script) = walk_herdr(&panes, Some("p1"), &[("p1", "p2"), ("p2", "p3")]);
        let herdr = Herdr::with_bin(&script);
        assert!(herdr.pane_focus_walk("w1:t1", "p3").unwrap());
        assert_eq!(focus_calls(&dir), ["p1 right", "p2 right"]);
        assert!(
            herdr.pane_focus_walk("w1:t1", "p3").unwrap(),
            "already there"
        );
        assert_eq!(focus_calls(&dir).len(), 2, "no step when already there");
    }

    #[cfg(unix)]
    #[test]
    fn pane_focus_walk_stops_without_a_step() {
        let panes = [("p1", 0), ("p2", 10)];
        let (dir, script) = walk_herdr(&panes, Some("p1"), &[("p1", "p2")]);
        let herdr = Herdr::with_bin(&script);
        assert!(
            !herdr.pane_focus_walk("w1:t9", "p2").unwrap(),
            "another tab"
        );
        let (none, script) = walk_herdr(&panes, None, &[("p1", "p2")]);
        assert!(
            !Herdr::with_bin(&script)
                .pane_focus_walk("w1:t1", "p2")
                .unwrap(),
            "no focus"
        );
        let (ghost, script) = walk_herdr(&panes, Some("p9"), &[("p9", "p2")]);
        assert!(
            !Herdr::with_bin(&script)
                .pane_focus_walk("w1:t1", "p2")
                .unwrap(),
            "focus off the layout"
        );
        let same = [("p1", 0), ("p2", 0)];
        let (overlap, script) = walk_herdr(&same, Some("p1"), &[("p1", "p2")]);
        assert!(
            !Herdr::with_bin(&script)
                .pane_focus_walk("w1:t1", "p2")
                .unwrap(),
            "no direction"
        );
        for d in [&dir, &none, &ghost, &overlap] {
            assert!(focus_calls(d).is_empty(), "no focus call");
        }
    }

    #[cfg(unix)]
    #[test]
    fn pane_focus_walk_stops_when_herdr_does_not_converge() {
        let panes = [("p1", 0), ("p2", 10), ("p3", 20)];
        let (stuck, script) = walk_herdr(&panes, Some("p1"), &[]);
        assert!(!Herdr::with_bin(&script)
            .pane_focus_walk("w1:t1", "p3")
            .unwrap());
        assert_eq!(
            focus_calls(&stuck),
            ["p1 right"],
            "no_neighbor stops the walk"
        );
        let (back, script) = walk_herdr(&panes, Some("p1"), &[("p1", "p1")]);
        assert!(!Herdr::with_bin(&script)
            .pane_focus_walk("w1:t1", "p3")
            .unwrap());
        assert_eq!(
            focus_calls(&back),
            ["p1 right"],
            "a step back to the origin stops it"
        );
        let (ping, script) = walk_herdr(&panes, Some("p1"), &[("p1", "p2"), ("p2", "p1")]);
        assert!(!Herdr::with_bin(&script)
            .pane_focus_walk("w1:t1", "p3")
            .unwrap());
        assert_eq!(
            focus_calls(&ping),
            ["p1 right", "p2 right", "p1 right"],
            "the pane count bounds an oscillating walk"
        );
    }
}
