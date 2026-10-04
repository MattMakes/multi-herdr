//! The seam between horch's domain code and the herdr CLI.
//!
//! [`WorkspaceClient`] lists the calls that spawn, delivery and teardown make.
//! [`Herdr`] is the real implementation, and
//! [`FakeWorkspace`](crate::workspace::testing::FakeWorkspace) is the in-memory
//! one.

use anyhow::Result;

use crate::workspace::herdr::Herdr;
use crate::workspace::model::{Direction, NewWorkspace, Pane};

pub trait WorkspaceClient {
    fn pane_get(&self, pane: &str) -> Result<Pane>;
    fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>>;
    fn pane_split(&self, from: &str, direction: Direction) -> Result<String>;
    fn pane_run(&self, pane: &str, command: &str) -> Result<()>;
    fn pane_close(&self, pane: &str) -> Result<()>;
    fn agent_prompt(&self, pane: &str, text: &str) -> Result<()>;
    fn pane_send_text(&self, pane: &str, text: &str) -> Result<()>;
    fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()>;
    fn pane_read(&self, pane: &str, source: &str) -> Result<String>;
    fn workspace_create(&self, label: &str, cwd: Option<&str>, focus: bool)
        -> Result<NewWorkspace>;
    fn workspace_close(&self, workspace: &str) -> Result<()>;
    /// True when the herdr server answers at all. A failed pane call on a
    /// reachable server means the pane is gone.
    fn server_reachable(&self) -> bool;
}

impl WorkspaceClient for Herdr {
    fn pane_get(&self, pane: &str) -> Result<Pane> {
        Herdr::pane_get(self, pane)
    }
    fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>> {
        Herdr::pane_list(self, workspace)
    }
    fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        Herdr::pane_split(self, from, direction)
    }
    fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        Herdr::pane_run(self, pane, command)
    }
    fn pane_close(&self, pane: &str) -> Result<()> {
        Herdr::pane_close(self, pane)
    }
    fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
        Herdr::agent_prompt(self, pane, text)
    }
    fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        Herdr::pane_send_text(self, pane, text)
    }
    fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        Herdr::pane_send_keys(self, pane, keys)
    }
    fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        Herdr::pane_read(self, pane, source)
    }
    fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        Herdr::workspace_create(self, label, cwd, focus)
    }
    fn workspace_close(&self, workspace: &str) -> Result<()> {
        Herdr::workspace_close(self, workspace)
    }
    fn server_reachable(&self) -> bool {
        Herdr::server_reachable(self)
    }
}
