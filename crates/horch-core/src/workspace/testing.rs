//! In-memory [`WorkspaceClient`] for hermetic tests.

use std::cell::RefCell;

use anyhow::{anyhow, Result};

use crate::workspace::client::WorkspaceClient;
use crate::workspace::model::{Direction, NewWorkspace, Pane};

/// One recorded call: the method name and its arguments, rendered as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCall {
    pub method: &'static str,
    pub args: Vec<String>,
}

/// What `pane_get` says about a pane's agent: `(agent, agent_status)`.
type AgentState = (Option<String>, Option<String>);

#[derive(Debug, Default)]
struct State {
    /// `(workspace id, pane id)` in creation order.
    panes: Vec<(String, String)>,
    workspaces: Vec<String>,
    screens: Vec<(String, String)>,
    /// `(pane, agent, status)` sequences: each `pane_get` takes the next
    /// pair, and the last one stays.
    agents: Vec<(String, Vec<AgentState>)>,
    calls: Vec<FakeCall>,
    failures: Vec<(&'static str, String)>,
    next_pane: u32,
    next_workspace: u32,
    /// The server does not answer: every call fails.
    unreachable: bool,
}

#[derive(Debug, Default)]
pub struct FakeWorkspace {
    state: RefCell<State>,
}

impl FakeWorkspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the next call of `method` fail with `message`. The failure fires once.
    pub fn fail_next(&self, method: &'static str, message: &str) {
        self.state
            .borrow_mut()
            .failures
            .push((method, message.to_owned()));
    }

    /// Make the server stop answering (`false`) or answer again (`true`).
    /// While it does not answer, every call fails.
    pub fn set_reachable(&self, reachable: bool) {
        self.state.borrow_mut().unreachable = !reachable;
    }

    /// Set the text that `pane_read` returns for `pane`.
    pub fn set_screen(&self, pane: &str, text: &str) {
        let mut st = self.state.borrow_mut();
        st.screens.retain(|(p, _)| p != pane);
        st.screens.push((pane.to_owned(), text.to_owned()));
    }

    /// Script what `pane_get` reports about the agent in `pane`: one
    /// `(agent, status)` per call, the last one repeating. `None` is a pane
    /// where herdr detects no agent.
    pub fn set_agent_states(&self, pane: &str, states: &[(Option<&str>, Option<&str>)]) {
        let mut st = self.state.borrow_mut();
        st.agents.retain(|(p, _)| p != pane);
        let states = states
            .iter()
            .map(|(a, s)| (a.map(str::to_owned), s.map(str::to_owned)))
            .collect();
        st.agents.push((pane.to_owned(), states));
    }

    pub fn calls(&self) -> Vec<FakeCall> {
        self.state.borrow().calls.clone()
    }

    pub fn pane_ids(&self) -> Vec<String> {
        self.state
            .borrow()
            .panes
            .iter()
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Log the call, then fire an injected failure for it if one waits.
    fn enter(&self, method: &'static str, args: &[&str]) -> Result<()> {
        let mut st = self.state.borrow_mut();
        st.calls.push(FakeCall {
            method,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        });
        if st.unreachable {
            return Err(anyhow!("herdr: no server is running"));
        }
        if let Some(i) = st.failures.iter().position(|(m, _)| *m == method) {
            let (_, message) = st.failures.remove(i);
            return Err(anyhow!(message));
        }
        Ok(())
    }

    fn pane(&self, id: &str, workspace: &str) -> Pane {
        let (agent, agent_status) = self.agent_state(id);
        Pane {
            pane_id: id.to_owned(),
            workspace_id: Some(workspace.to_owned()),
            tab_id: None,
            agent_session: None,
            agent,
            agent_status,
        }
    }

    /// The next scripted `(agent, status)` of `pane`; none when unscripted.
    fn agent_state(&self, pane: &str) -> AgentState {
        let mut st = self.state.borrow_mut();
        let Some((_, states)) = st.agents.iter_mut().find(|(p, _)| p == pane) else {
            return (None, None);
        };
        if states.len() > 1 {
            states.remove(0)
        } else {
            states.first().cloned().unwrap_or((None, None))
        }
    }

    fn require(&self, pane: &str) -> Result<()> {
        if self.state.borrow().panes.iter().any(|(_, p)| p == pane) {
            Ok(())
        } else {
            Err(anyhow!("pane {pane} not found"))
        }
    }
}

impl WorkspaceClient for FakeWorkspace {
    fn pane_get(&self, pane: &str) -> Result<Pane> {
        self.enter("pane_get", &[pane])?;
        let workspace = self
            .state
            .borrow()
            .panes
            .iter()
            .find(|(_, p)| p == pane)
            .map(|(w, _)| w.clone())
            .ok_or_else(|| anyhow!("pane {pane} not found"))?;
        Ok(self.pane(pane, &workspace))
    }

    fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>> {
        self.enter("pane_list", &[workspace])?;
        // Collected first: building a pane borrows the state again.
        let listed: Vec<(String, String)> = self
            .state
            .borrow()
            .panes
            .iter()
            .filter(|(w, _)| w == workspace)
            .cloned()
            .collect();
        Ok(listed.iter().map(|(w, p)| self.pane(p, w)).collect())
    }

    fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        self.enter("pane_split", &[from, direction.as_str()])?;
        let workspace = self
            .state
            .borrow()
            .panes
            .iter()
            .find(|(_, p)| p == from)
            .map(|(w, _)| w.clone())
            .ok_or_else(|| anyhow!("pane {from} not found"))?;
        let mut st = self.state.borrow_mut();
        st.next_pane += 1;
        let id = format!("p{}", st.next_pane);
        st.panes.push((workspace, id.clone()));
        Ok(id)
    }

    fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        self.enter("pane_run", &[pane, command])?;
        self.require(pane)
    }

    fn pane_close(&self, pane: &str) -> Result<()> {
        self.enter("pane_close", &[pane])?;
        self.require(pane)?;
        self.state.borrow_mut().panes.retain(|(_, p)| p != pane);
        Ok(())
    }

    fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
        self.enter("agent_prompt", &[pane, text])?;
        self.require(pane)
    }

    fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.enter("pane_send_text", &[pane, text])?;
        self.require(pane)
    }

    fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        self.enter("pane_send_keys", &[pane, keys])?;
        self.require(pane)
    }

    fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        self.enter("pane_read", &[pane, source])?;
        self.require(pane)?;
        Ok(self
            .state
            .borrow()
            .screens
            .iter()
            .find(|(p, _)| p == pane)
            .map(|(_, t)| t.clone())
            .unwrap_or_default())
    }

    fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        self.enter(
            "workspace_create",
            &[
                label,
                cwd.unwrap_or(""),
                if focus { "focus" } else { "no-focus" },
            ],
        )?;
        let mut st = self.state.borrow_mut();
        st.next_workspace += 1;
        let workspace_id = format!("w{}", st.next_workspace);
        st.next_pane += 1;
        let root_pane_id = format!("p{}", st.next_pane);
        st.workspaces.push(workspace_id.clone());
        st.panes.push((workspace_id.clone(), root_pane_id.clone()));
        Ok(NewWorkspace {
            workspace_id,
            root_pane_id,
            tab_id: None,
        })
    }

    fn workspace_close(&self, workspace: &str) -> Result<()> {
        self.enter("workspace_close", &[workspace])?;
        let mut st = self.state.borrow_mut();
        if !st.workspaces.iter().any(|w| w == workspace) {
            return Err(anyhow!("workspace {workspace} not found"));
        }
        st.workspaces.retain(|w| w != workspace);
        st.panes.retain(|(w, _)| w != workspace);
        Ok(())
    }

    fn server_reachable(&self) -> bool {
        self.enter("server_reachable", &[]).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_adds_a_pane_with_the_next_id() {
        let fake = FakeWorkspace::new();
        let ws = fake.workspace_create("w", None, false).unwrap();
        assert_eq!(ws.root_pane_id, "p1");
        let id = fake.pane_split("p1", Direction::Right).unwrap();
        assert_eq!(id, "p2");
        assert_eq!(fake.pane_list(&ws.workspace_id).unwrap().len(), 2);
        assert_eq!(fake.calls().last().unwrap().method, "pane_list");
    }

    #[test]
    fn close_removes_a_pane() {
        let fake = FakeWorkspace::new();
        fake.workspace_create("w", None, false).unwrap();
        let id = fake.pane_split("p1", Direction::Down).unwrap();
        fake.pane_close(&id).unwrap();
        assert_eq!(fake.pane_ids(), vec!["p1".to_owned()]);
        assert!(fake.pane_get(&id).is_err());
    }

    #[test]
    fn an_unreachable_server_fails_every_call_until_it_answers() {
        let fake = FakeWorkspace::new();
        fake.workspace_create("w", None, false).unwrap();
        assert!(fake.server_reachable());
        fake.set_reachable(false);
        assert!(!fake.server_reachable());
        assert!(fake.pane_get("p1").is_err());
        fake.set_reachable(true);
        assert!(fake.server_reachable());
        fake.pane_get("p1").unwrap();
    }

    #[test]
    fn an_injected_failure_fires_once() {
        let fake = FakeWorkspace::new();
        fake.workspace_create("w", None, false).unwrap();
        fake.fail_next("pane_run", "boom");
        let err = fake.pane_run("p1", "ls").unwrap_err();
        assert_eq!(err.to_string(), "boom");
        fake.pane_run("p1", "ls").unwrap();
    }

    #[test]
    fn pane_read_returns_the_screen_that_was_set() {
        let fake = FakeWorkspace::new();
        fake.workspace_create("w", None, false).unwrap();
        assert_eq!(fake.pane_read("p1", "visible").unwrap(), "");
        fake.set_screen("p1", "hello");
        assert_eq!(fake.pane_read("p1", "visible").unwrap(), "hello");
    }
}
