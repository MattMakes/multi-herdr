//! One handle for the pane host (`herdr` or `hc`).
//!
//! Domain and CLI code outside `crate::workspace` reaches the host exclusively
//! through [`PaneHost`]. Until Task 9 lands `HcApp`, every constructor wraps
//! [`Herdr`] while recording the resolved [`Backend`].

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::execution::store::{workspace_backend, ExecutionStore};
use crate::runtime::{Backend, RuntimeContext};
use crate::workspace::herdr::Herdr;
use crate::workspace::model::{
    Direction, Focus, FocusDir, Layout, Move, NewWorkspace, Pane, Resize, Tab, Workspace,
};

/// Capabilities of the active pane host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub geometry: bool,
}

#[derive(Debug, Clone)]
enum Inner {
    Herdr(Herdr),
}

/// Handle to the pane host that owns a pane or workspace.
#[derive(Debug, Clone)]
pub struct PaneHost {
    backend: Backend,
    inner: Inner,
}

impl PaneHost {
    /// Host for the pane this process runs in (`ctx.herdr.backend`).
    pub fn of(ctx: &RuntimeContext) -> Self {
        Self::for_backend(ctx, ctx.herdr.backend)
    }

    /// Host for a known [`Backend`] (from a worker row or execution record).
    pub fn for_backend(ctx: &RuntimeContext, b: Backend) -> Self {
        Self {
            backend: b,
            inner: Inner::Herdr(Herdr::with_bin(&ctx.bins.harness.herdr)),
        }
    }

    /// Host for a workspace id: the backend recorded for `workspace` in
    /// [`ExecutionStore`], else `ctx.herdr.backend`.
    pub fn for_workspace(ctx: &RuntimeContext, workspace: &str) -> Self {
        let recorded = ExecutionStore::open_in(ctx)
            .ok()
            .and_then(|s| s.backend_of_workspace(workspace).ok().flatten())
            .or_else(|| workspace_backend(&ctx.paths, workspace));
        Self::for_backend(ctx, recorded.unwrap_or(ctx.herdr.backend))
    }

    /// Test helper: a `herdr` host backed by `bin`.
    pub fn herdr_bin(bin: impl Into<PathBuf>) -> Self {
        Self {
            backend: Backend::Herdr,
            inner: Inner::Herdr(Herdr::with_bin(bin)),
        }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn caps(&self) -> Caps {
        match &self.inner {
            Inner::Herdr(_) => Caps { geometry: true },
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.inner = match self.inner {
            Inner::Herdr(h) => Inner::Herdr(h.with_timeout(t)),
        };
        self
    }

    pub fn until(mut self, deadline: Option<Instant>) -> Self {
        self.inner = match self.inner {
            Inner::Herdr(h) => Inner::Herdr(h.until(deadline)),
        };
        self
    }

    pub fn server_reachable(&self) -> bool {
        match &self.inner {
            Inner::Herdr(h) => h.server_reachable(),
        }
    }

    pub fn pane_get(&self, id: &str) -> Result<Pane> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_get(id),
        }
    }

    pub fn pane_list(&self, workspace_id: &str) -> Result<Vec<Pane>> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_list(workspace_id),
        }
    }

    pub fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_split(from, direction),
        }
    }

    pub fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_run(pane, command),
        }
    }

    pub fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_send_text(pane, text),
        }
    }

    pub fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_send_keys(pane, keys),
        }
    }

    pub fn pane_close(&self, pane: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_close(pane),
        }
    }

    pub fn pane_exists(&self, pane: &str) -> bool {
        match &self.inner {
            Inner::Herdr(h) => h.pane_exists(pane),
        }
    }

    pub fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_read(pane, source),
        }
    }

    pub fn pane_layout(&self, pane: Option<&str>) -> Result<Layout> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_layout(pane),
        }
    }

    pub fn tab_list(&self, workspace_id: &str) -> Result<Vec<Tab>> {
        match &self.inner {
            Inner::Herdr(h) => h.tab_list(workspace_id),
        }
    }

    pub fn workspace_list(&self) -> Result<Vec<Workspace>> {
        match &self.inner {
            Inner::Herdr(h) => h.workspace_list(),
        }
    }

    pub(crate) fn tab_focus(&self, tab_id: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.tab_focus(tab_id),
        }
    }

    pub fn pane_focus(&self, origin: &str, direction: FocusDir) -> Result<Focus> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_focus(origin, direction),
        }
    }

    pub fn pane_focus_walk(&self, tab: &str, target: &str) -> Result<bool> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_focus_walk(tab, target),
        }
    }

    pub(crate) fn pane_move(
        &self,
        pane: &str,
        tab: &str,
        direction: Direction,
        target: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Move> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_move(pane, tab, direction, target, ratio),
        }
    }

    pub(crate) fn pane_move_new_tab(&self, pane: &str, label: &str) -> Result<Move> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_move_new_tab(pane, label),
        }
    }

    pub(crate) fn pane_resize(&self, pane: &str, direction: &str, amount: f64) -> Result<Resize> {
        match &self.inner {
            Inner::Herdr(h) => h.pane_resize(pane, direction, amount),
        }
    }

    pub fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        match &self.inner {
            Inner::Herdr(h) => h.workspace_create(label, cwd, focus),
        }
    }

    pub fn workspace_close(&self, workspace_id: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.workspace_close(workspace_id),
        }
    }

    pub fn notification_show(&self, title: &str, body: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.notification_show(title, body),
        }
    }

    pub fn wait_output(&self, pane: &str, needle: &str, timeout_ms: u64) -> Result<bool> {
        match &self.inner {
            Inner::Herdr(h) => h.wait_output(pane, needle, timeout_ms),
        }
    }

    pub fn integration_status(&self) -> Option<String> {
        match &self.inner {
            Inner::Herdr(h) => h.integration_status(),
        }
    }

    pub fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.agent_prompt(pane, text),
        }
    }

    pub fn report_agent(
        &self,
        pane: &str,
        source: &str,
        agent: &str,
        state: &str,
        seq: u64,
    ) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.report_agent(pane, source, agent, state, seq),
        }
    }

    pub fn release_agent(&self, pane: &str, source: &str, agent: &str, seq: u64) -> Result<()> {
        match &self.inner {
            Inner::Herdr(h) => h.release_agent(pane, source, agent, seq),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::execution::legacy::{LedgerRecordV1, STATUS_WORKING};
    use crate::runtime::MapEnv;

    fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_rs_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn hcb_04_no_herdr_construction_outside_workspace() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest.parent().unwrap().parent().unwrap();
        let workspace_dir = manifest.join("src").join("workspace");
        let mut files = Vec::new();
        walk_rs_files(&manifest.join("src"), &mut files);
        walk_rs_files(
            &repo_root.join("crates").join("horch").join("src"),
            &mut files,
        );
        files.sort();

        let mut violations = Vec::new();
        for path in files {
            if path.starts_with(&workspace_dir) {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for (idx, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("Herdr::with_bin") || line.contains("workspace::herdr::Herdr") {
                    let rel = path.strip_prefix(repo_root).unwrap_or(&path);
                    violations.push(format!("{}:{}: {}", rel.display(), idx + 1, line.trim()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "direct Herdr references outside workspace/:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn hcb_04_for_workspace_uses_the_recorded_backend() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();

        let env = MapEnv::new(&proj)
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy())
            .with("HORCH_PROJECT_DIR", &proj.to_string_lossy())
            .with("HERDR_PANE_ID", "p_1");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        assert_eq!(ctx.herdr.backend, Backend::Herdr);

        let store = ExecutionStore::open_in(&ctx).unwrap();
        store
            .insert(LedgerRecordV1 {
                record_id: "r-hc".into(),
                role: "orchestrator".into(),
                status: STATUS_WORKING.into(),
                workspace_id: Some("sp-hc".into()),
                backend: Backend::Hc,
                ..LedgerRecordV1::default()
            })
            .unwrap();
        store
            .insert(LedgerRecordV1 {
                record_id: "r-herdr".into(),
                role: "orchestrator".into(),
                status: STATUS_WORKING.into(),
                workspace_id: Some("ws-herdr".into()),
                backend: Backend::Herdr,
                ..LedgerRecordV1::default()
            })
            .unwrap();

        assert_eq!(
            PaneHost::for_workspace(&ctx, "sp-hc").backend(),
            Backend::Hc
        );
        assert_eq!(
            PaneHost::for_workspace(&ctx, "ws-herdr").backend(),
            Backend::Herdr
        );

        // Detached courier case: no project_dir and cwd outside the project.
        let detached_env = MapEnv::new("/")
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy());
        let detached_ctx = RuntimeContext::from_env(&detached_env).unwrap();
        assert_eq!(
            PaneHost::for_workspace(&detached_ctx, "sp-hc").backend(),
            Backend::Hc
        );
    }

    #[test]
    fn hcb_04_for_workspace_without_a_record_uses_the_context() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let hc_env = MapEnv::new("/")
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy())
            .with("HC_TERMINAL_ID", "t2");
        let hc_ctx = RuntimeContext::from_env(&hc_env).unwrap();
        assert_eq!(
            PaneHost::for_workspace(&hc_ctx, "unrecorded").backend(),
            Backend::Hc
        );

        let herdr_env = MapEnv::new("/")
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy());
        let herdr_ctx = RuntimeContext::from_env(&herdr_env).unwrap();
        assert_eq!(
            PaneHost::for_workspace(&herdr_ctx, "unrecorded").backend(),
            Backend::Herdr
        );
    }
}
