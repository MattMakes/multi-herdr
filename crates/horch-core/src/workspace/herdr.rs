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
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

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
    fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
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
        let mut layout = self.pane_layout(Some(target))?;
        if layout.tab_id != tab {
            return Ok(false);
        }
        let budget = layout.panes.len();
        for _ in 0..budget {
            let Some(focused) = layout.focused_pane_id.clone() else {
                return Ok(false);
            };
            if focused == target {
                return Ok(true);
            }
            let rect = |id: &str| {
                layout
                    .panes
                    .iter()
                    .find(|p| p.pane_id == id)
                    .map(|p| p.rect)
            };
            let (Some(from), Some(to)) = (rect(&focused), rect(target)) else {
                return Ok(false);
            };
            let Some(direction) = crate::workspace::tile::step_toward(&from, &to) else {
                return Ok(false);
            };
            let step = self.pane_focus(&focused, direction)?;
            // `no_neighbor`, or a step that landed back where it started: either
            // way the walk is not converging, so stop instead of spinning.
            if !step.changed || step.focused_pane_id.as_deref() == Some(focused.as_str()) {
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

/// Read a `herdr pane wait-output` result: success is a match; herdr's
/// `{"error":{"code":"timeout",...}}` on stderr is no match; anything else
/// (a missing pane, an unknown command) is an error.
fn wait_outcome(success: bool, stderr: &str) -> Result<bool> {
    if success {
        return Ok(true);
    }
    let code = serde_json::from_str::<serde_json::Value>(stderr.trim())
        .ok()
        .and_then(|v| v["error"]["code"].as_str().map(str::to_owned));
    if code.as_deref() == Some("timeout") {
        return Ok(false);
    }
    bail!("{}", stderr.trim())
}

/// The most a bounded call reads from each of stdout and stderr.
const OUTPUT_CAP: u64 = 1024 * 1024;

/// The error for a call that reached its stop: the push deadline when
/// `at_deadline`, else `timeout`.
fn stop_error(at_deadline: bool, timeout: Duration) -> std::io::Error {
    let message = if at_deadline {
        "herdr did not answer inside the push budget".to_string()
    } else {
        format!("herdr did not answer within {} ms", timeout.as_millis())
    };
    limit_error(message, std::io::ErrorKind::TimedOut)
}

fn limit_error(message: String, kind: std::io::ErrorKind) -> std::io::Error {
    std::io::Error::new(kind, message)
}

/// A reader's channel and its thread.
type Reader = (mpsc::Receiver<std::io::Result<Vec<u8>>>, JoinHandle<()>);

/// Read at most [`OUTPUT_CAP`] + 1 bytes of `pipe`, then send them. The
/// reader ends when the pipe closes or the cap is hit; a caller waits for it
/// only for [`READER_GRACE`].
fn read_capped(pipe: Option<impl Read + Send + 'static>) -> std::io::Result<Reader> {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::Builder::new().spawn(move || {
        let mut bytes = Vec::new();
        let read = match pipe {
            Some(pipe) => pipe
                .take(OUTPUT_CAP + 1)
                .read_to_end(&mut bytes)
                .map(|_| ()),
            None => Ok(()),
        };
        let _ = sender.send(read.map(|()| bytes));
    })?;
    Ok((receiver, handle))
}

/// On Windows, the herdr process tree. The job kills every process in it
/// when the last handle closes, so a descendant that holds a pipe cannot
/// outlive the call on any path.
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    fn contain(child: &Child) -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: a null name and attributes make an unnamed job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Job(handle);
        // SAFETY: an all-zero limit structure is valid.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is live and its size is passed with it.
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        };
        // SAFETY: the child handle stays valid while `child` is alive.
        if set == 0 || unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as _) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    fn terminate(&self) {
        // SAFETY: `self.0` is a live job handle.
        if unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) } == 0 {
            eprintln!("horch: could not kill the herdr process tree");
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own; kill-on-close ends the tree.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// The most a finished call waits for its reader threads after the child is
/// killed. A reader still blocked then is left to end with its pipe: a
/// descendant that escaped containment can hold the pipe open.
const READER_GRACE: Duration = Duration::from_millis(200);

fn join_readers(handles: Vec<JoinHandle<()>>) {
    let until = Instant::now() + READER_GRACE;
    for handle in handles {
        while !handle.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

/// Let the suspended `child` run: resume its only thread. std keeps the main
/// thread handle to itself, so find the thread by its owner process.
#[cfg(windows)]
fn resume_main_thread(child: &Child) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    // SAFETY: a snapshot of all threads; the handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: an all-zero entry is valid; `dwSize` is set before use.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut resumed = false;
    // SAFETY: `snapshot` is live and `entry` is sized.
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == child.id() {
            // SAFETY: opens the thread for resume only; closed right after.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: `thread` is a live handle we own.
                resumed |= unsafe { ResumeThread(thread) } != u32::MAX;
                unsafe { CloseHandle(thread) };
            }
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: closing the snapshot we own.
    unsafe { CloseHandle(snapshot) };
    if resumed {
        Ok(())
    } else {
        Err(std::io::Error::other("could not resume herdr"))
    }
}

/// Kill and reap `child`. A failure of either is a warning, not an error:
/// the call has already failed and the caller goes on.
///
/// The leader is still unreaped here, so its pid cannot belong to another
/// process: the group signal reaches only this call's descendants.
fn kill_and_reap(child: &mut Child) -> std::io::Result<()> {
    // The child leads its own process group, so one signal also ends every
    // descendant that holds a pipe.
    #[cfg(unix)]
    // SAFETY: `kill` only signals; the leader is unreaped, so the group is ours.
    let killed = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) } == 0;
    #[cfg(not(unix))]
    let killed = child.kill().is_ok();
    if !killed {
        eprintln!("horch: could not kill herdr");
    }
    reap_within(child, REAP_GRACE).inspect_err(|e| eprintln!("horch: could not reap herdr: {e}"))
}

/// The most a failed call waits for its killed child to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(1);

/// Reap `child` by polling, never by a blocking wait: a child that does not
/// exit within `grace` is an error, so the call still returns.
fn reap_within(child: &mut Child, grace: Duration) -> std::io::Result<()> {
    let until = Instant::now() + grace;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(limit_error(
                format!(
                    "herdr did not exit within {} ms of the kill",
                    grace.as_millis()
                ),
                std::io::ErrorKind::TimedOut,
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// True once the child has exited. It stays unreaped, a zombie, so its pid
/// and process group cannot be reused before the caller kills and reaps it.
fn has_exited(child: &mut Child) -> std::io::Result<bool> {
    #[cfg(unix)]
    {
        // SAFETY: an all-zero `siginfo_t` is valid; `waitid` fills it in.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits on our own unreaped child without reaping it.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `waitid` succeeded, so the pid field is initialised.
        #[cfg(target_os = "linux")]
        let pid = unsafe { info.si_pid() };
        #[cfg(not(target_os = "linux"))]
        let pid = info.si_pid;
        Ok(pid != 0)
    }
    #[cfg(not(unix))]
    Ok(child.try_wait()?.is_some())
}

/// Run `command` with one `timeout` for the exit and for both pipes. The
/// call stops at the earlier of `timeout` and the absolute `deadline`. On
/// that stop, or when either pipe passes the cap, the child is killed and the
/// call fails. On Windows the child starts suspended inside a job, and the
/// whole tree is killed. Every platform then waits at most [`READER_GRACE`]
/// for the readers.
fn output_within(
    mut command: Command,
    timeout: Duration,
    deadline: Option<Instant>,
) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let timeout_end = Instant::now().checked_add(timeout).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "herdr timeout is too large",
        )
    })?;
    let stop = deadline.map_or(timeout_end, |deadline| deadline.min(timeout_end));
    // The child starts suspended, so it cannot start a descendant before the
    // job holds it.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        windows_sys::Win32::System::Threading::CREATE_SUSPENDED,
    );
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let job = match Job::contain(&child).and_then(|job| resume_main_thread(&child).map(|()| job)) {
        Ok(job) => job,
        Err(error) => {
            let _ = kill_and_reap(&mut child);
            return Err(error);
        }
    };
    let ((stdout_rx, stdout_thread), (stderr_rx, stderr_thread)) = match (
        read_capped(child.stdout.take()),
        read_capped(child.stderr.take()),
    ) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => {
            #[cfg(windows)]
            job.terminate();
            let _ = kill_and_reap(&mut child);
            return Err(error);
        }
    };
    let readers = [stdout_rx, stderr_rx];
    let threads = vec![stdout_thread, stderr_thread];
    let mut collected: [Option<Vec<u8>>; 2] = [None, None];
    let mut exited = false;
    let failure = loop {
        let mut read_failure = None;
        for (slot, reader) in collected.iter_mut().zip(&readers) {
            match reader.try_recv() {
                Ok(Ok(bytes)) if slot.is_none() => *slot = Some(bytes),
                Ok(Err(error)) => read_failure = Some(error),
                _ => {}
            }
        }
        if read_failure.is_some() {
            break read_failure;
        }
        if collected
            .iter()
            .flatten()
            .any(|b| b.len() as u64 > OUTPUT_CAP)
        {
            break Some(limit_error(
                format!("herdr printed more than {OUTPUT_CAP} bytes"),
                std::io::ErrorKind::InvalidData,
            ));
        }
        if !exited {
            exited = match has_exited(&mut child) {
                Ok(exited) => exited,
                Err(error) => {
                    #[cfg(windows)]
                    job.terminate();
                    let _ = kill_and_reap(&mut child);
                    join_readers(threads);
                    return Err(error);
                }
            };
        }
        if exited && collected.iter().all(Option::is_some) {
            break None;
        }
        if Instant::now() >= stop {
            break Some(stop_error(stop < timeout_end, timeout));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if let Some(error) = failure {
        #[cfg(windows)]
        job.terminate();
        let _ = kill_and_reap(&mut child);
        join_readers(threads);
        return Err(error);
    }
    join_readers(threads);
    let [stdout, stderr] = collected;
    Ok(Output {
        status: child.wait()?,
        stdout: stdout.unwrap_or_default(),
        stderr: stderr.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn script_in(body: &str) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("herdr");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, script)
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_that_keeps_stdout_open_past_the_deadline_fails_the_call() {
        let (_dir, script) = script_in("sleep 4 &\nexit 0");
        let started = Instant::now();
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(1000));
        assert!(herdr.pane_list("w1").is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_stdout_is_killed_with_the_call() {
        let (dir, script) = script_in("sleep 30 &\necho $! > \"$(dirname \"$0\")/pid\"\nexit 0");
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(1500));
        assert!(herdr.pane_list("w1").is_err());
        // A loaded machine can kill the script before it writes its pid:
        // then no process was left to leak.
        let Ok(pid) = std::fs::read_to_string(dir.path().join("pid")) else {
            return;
        };
        let gone = (0..40).any(|_| {
            let dead = !Command::new("kill")
                .args(["-0", pid.trim()])
                .output()
                .unwrap()
                .status
                .success();
            if !dead {
                std::thread::sleep(Duration::from_millis(50));
            }
            dead
        });
        assert!(gone, "descendant {pid} is still running");
    }

    #[cfg(unix)]
    #[test]
    fn a_herdr_that_prints_more_than_the_cap_fails_the_call() {
        for stream in ["", ">&2"] {
            let (_dir, script) = script_in(&format!("head -c 3000000 /dev/zero {stream}"));
            let started = Instant::now();
            let error =
                output_within(Command::new(&script), Duration::from_secs(5), None).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{stream}");
            assert!(error.to_string().contains("more than"), "{error}");
            assert!(started.elapsed() < Duration::from_secs(10));
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_descendant_holding_stdout_does_not_outlive_the_timeout() {
        let mut command = Command::new("cmd");
        command.args(["/c", "start /b ping -n 30 127.0.0.1 & exit 0"]);
        let started = Instant::now();
        let error = output_within(command, Duration::from_millis(1000), None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        // The call joins its readers: it returns only once the tree is dead.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn reaping_a_child_that_does_not_exit_returns_inside_the_grace() {
        let mut command = Command::new("sleep");
        command.arg("30");
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().unwrap();
        let started = Instant::now();
        let error = reap_within(&mut child, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
        kill_and_reap(&mut child).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_stalled_herdr_fails_with_timed_out_after_the_deadline() {
        let (_dir, script) = script_in("exec sleep 30");
        let started = Instant::now();
        let error =
            output_within(Command::new(&script), Duration::from_millis(500), None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("did not answer"), "{error}");
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    #[cfg(unix)]
    #[test]
    fn a_timeout_too_large_for_an_instant_is_rejected_before_spawning() {
        let (_dir, script) = script_in("exit 0");
        let error = output_within(Command::new(&script), Duration::MAX, None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn a_herdr_that_outlasts_the_timeout_is_killed() {
        let (dir, script) = script_in("echo $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 60");
        let pidfile = dir.path().join("pid");

        let started = Instant::now();
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(2000));
        assert!(herdr.pane_list("w1").is_err());

        assert!(started.elapsed() < Duration::from_secs(10));
        let Ok(pid) = std::fs::read_to_string(&pidfile) else {
            return;
        };
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .unwrap();
        assert!(!alive.status.success(), "herdr {pid} is still running");
    }

    /// A herdr call that outlives the deadline is killed and fails at once;
    /// one that answers in time returns its output (#67).
    #[cfg(unix)]
    #[test]
    fn a_call_past_the_deadline_is_killed() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let script = |name: &str, body: &str| {
            let path = tmp.path().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let stuck = Herdr::with_bin(script("stuck", "sleep 10"))
            .until(Some(Instant::now() + Duration::from_millis(300)));
        let started = Instant::now();
        let err = stuck.pane_get("w1:p1").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(3), "{err}");
        assert!(err.contains("push budget"), "{err}");
        let quick = Herdr::with_bin(script("quick", "echo hi"))
            .until(Some(Instant::now() + Duration::from_secs(10)));
        assert_eq!(quick.output(&["x"]).unwrap(), "hi\n");
    }

    /// With a timeout and a deadline, the earlier one stops the call.
    #[cfg(unix)]
    #[test]
    fn a_deadline_before_the_timeout_stops_the_call() {
        let (_dir, script) = script_in("sleep 10");
        let herdr = Herdr::with_bin(&script)
            .with_timeout(Duration::from_secs(10))
            .until(Some(Instant::now() + Duration::from_millis(300)));
        let started = Instant::now();
        let err = herdr.pane_get("w1:p1").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(3), "{err}");
        assert!(err.contains("push budget"), "{err}");
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
}
