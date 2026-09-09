//! Per-workspace mailbox: the role -> pane-id registry that makes `horch tell`
//! possible, plus the brief a spawned worker reads to learn who it is.
//!
//! Lives under the system temp dir keyed by workspace id, so it dies with the
//! machine but survives individual pane restarts.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::herdr::Herdr;
use crate::teammates::{Agent, Teammate};

/// Everything a worker pane needs to know about itself, written by
/// `horch spawn` and read by `horch worker`.
///
/// The bash implementation wrote a shell fragment of `export`s quoted with
/// `printf %q`; JSON avoids that quoting problem entirely and has the same
/// meaning on Windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub role: String,
    /// The teammate's name, i.e. the `teammates/<name>.md` this worker is.
    pub teammate: String,
    pub agent: String,
    pub model: String,
    pub record_id: String,
    /// Empty until known. Claude session ids are minted at spawn; codex only
    /// reveals its id after launch.
    #[serde(default)]
    pub session_id: String,
    pub resume: bool,
    #[serde(default)]
    pub task: String,
    pub project_dir: String,
    /// Set only when the caller overrode the ledger location.
    #[serde(default)]
    pub state_dir: Option<String>,
    /// Agent-CLI overrides captured at spawn time. A pane is a fresh shell that
    /// does not inherit the spawning process's environment, so these have to
    /// travel in the brief to reach the worker at all.
    #[serde(default)]
    pub claude_bin: Option<String>,
    #[serde(default)]
    pub codex_bin: Option<String>,
    /// The teammate resolved at spawn time, carried so the worker renders the
    /// briefing the spawner actually chose. A worker's cwd is the target
    /// project, not this repo, so re-reading the roster in the pane could
    /// resolve a different file - or none at all.
    #[serde(default)]
    pub resolved: Option<Teammate>,
    /// Where the roster was read from at spawn time. A pane does not inherit
    /// `$HORCH_TEAMMATES_DIR`, so the path travels here or the worker falls
    /// back to the compiled-in copies and silently ignores local edits.
    #[serde(default)]
    pub teammates_dir: Option<String>,
}

impl Brief {
    pub fn agent(&self) -> Result<Agent> {
        self.agent.parse().map_err(anyhow::Error::msg)
    }
}

/// The mailbox directory for one herdr workspace.
#[derive(Debug, Clone)]
pub struct Mailbox {
    dir: PathBuf,
    workspace_id: String,
}

impl Mailbox {
    /// Build a mailbox handle for `workspace_id` under the system temp dir.
    pub fn new(workspace_id: &str) -> Self {
        Self::under(std::env::temp_dir(), workspace_id)
    }

    /// Same, but rooted at an explicit temp dir. Exists for tests.
    pub fn under(temp_root: impl AsRef<Path>, workspace_id: &str) -> Self {
        Self {
            dir: temp_root
                .as_ref()
                .join(format!("herdr-orchestration-{workspace_id}")),
            workspace_id: workspace_id.to_string(),
        }
    }

    /// Resolve the workspace this process belongs to, then open its mailbox.
    ///
    /// `HORCH_WORKSPACE_ID` is exported by [`Mailbox::register`], so a registered
    /// pane hits the cheap path. Otherwise fall back to this pane's own
    /// `HERDR_PANE_ID` - herdr exports an internal id there (`p_2`), which
    /// `herdr pane get` upgrades to the public workspace id. That fallback is what
    /// lets these commands also work from an unregistered pane.
    pub fn resolve(herdr: &Herdr) -> Result<Self> {
        if let Ok(ws) = std::env::var("HORCH_WORKSPACE_ID") {
            if !ws.is_empty() {
                return Ok(Self::new(&ws));
            }
        }
        let pane_id = std::env::var("HERDR_PANE_ID").ok().filter(|s| !s.is_empty());
        let Some(pane_id) = pane_id else {
            bail!(
                "not inside a herdr pane (HERDR_PANE_ID is unset) and HORCH_WORKSPACE_ID is not set"
            )
        };
        let pane = herdr.pane_get(&pane_id)?;
        let ws = pane.workspace_id.with_context(|| {
            format!("herdr did not report a workspace_id for pane {pane_id}")
        })?;
        Ok(Self::new(&ws))
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn exists(&self) -> bool {
        self.dir.is_dir()
    }

    fn id_path(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{role}.id"))
    }

    fn brief_path(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{role}.brief.json"))
    }

    /// Record this pane's public id under `role` so other panes can address it.
    ///
    /// Returns the resolved public pane id. Also sets `HORCH_WORKSPACE_ID` in this
    /// process so child processes inherit it.
    pub fn register(herdr: &Herdr, role: &str) -> Result<(Self, String)> {
        let internal = std::env::var("HERDR_PANE_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .context("must run inside a herdr pane (HERDR_PANE_ID is unset)")?;
        let pane = herdr.pane_get(&internal)?;
        let ws = pane
            .workspace_id
            .clone()
            .with_context(|| format!("herdr did not report a workspace_id for pane {internal}"))?;
        let mailbox = Self::new(&ws);
        std::fs::create_dir_all(&mailbox.dir)
            .with_context(|| format!("creating mailbox {}", mailbox.dir.display()))?;
        std::fs::write(mailbox.id_path(role), &pane.pane_id)
            .with_context(|| format!("registering role '{role}'"))?;
        std::env::set_var("HORCH_WORKSPACE_ID", &ws);
        Ok((mailbox, pane.pane_id))
    }

    /// The pane id registered for `role`, if any.
    pub fn pane_for(&self, role: &str) -> Option<String> {
        let raw = std::fs::read_to_string(self.id_path(role)).ok()?;
        let trimmed = raw.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// Every registered `(role, pane_id)`, sorted by role.
    pub fn roles(&self) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<(String, String)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension()? != "id" {
                    return None;
                }
                let role = path.file_stem()?.to_str()?.to_string();
                let pane = std::fs::read_to_string(&path).ok()?.trim().to_string();
                (!pane.is_empty()).then_some((role, pane))
            })
            .collect();
        out.sort();
        out
    }

    /// Map of pane id -> role, for labelling layout output.
    pub fn panes_to_roles(&self) -> Vec<(String, String)> {
        self.roles().into_iter().map(|(r, p)| (p, r)).collect()
    }

    pub fn write_brief(&self, brief: &Brief) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.brief_path(&brief.role);
        let json = serde_json::to_string_pretty(brief)?;
        std::fs::write(&path, json).with_context(|| format!("writing brief {}", path.display()))
    }

    pub fn read_brief(&self, role: &str) -> Result<Brief> {
        let path = self.brief_path(role);
        let raw = std::fs::read_to_string(&path).with_context(|| {
            format!("no brief at {} (spawn this worker with `horch spawn`)", path.display())
        })?;
        serde_json::from_str(&raw).with_context(|| format!("parsing brief {}", path.display()))
    }

    /// True when anything already claims this role in this workspace.
    pub fn role_taken(&self, role: &str) -> bool {
        self.id_path(role).exists() || self.brief_path(role).exists()
    }

    /// Drop a role's registration and brief, so the role name frees up.
    pub fn unregister(&self, role: &str) {
        let _ = std::fs::remove_file(self.id_path(role));
        let _ = std::fs::remove_file(self.brief_path(role));
    }

    /// Next per-teammate ordinal for auto-naming a role `<teammate>-<n>`.
    pub fn next_seq(&self, teammate: &str) -> Result<u32> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!(".seq-{teammate}"));
        let current: u32 = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let next = current + 1;
        std::fs::write(&path, next.to_string())?;
        Ok(next)
    }

    /// Marker file used to bound the codex rollout-file search to sessions
    /// created after this worker launched.
    pub fn launch_marker(&self, role: &str) -> PathBuf {
        self.dir.join(format!(".{role}.launch-marker"))
    }

    pub fn harvest_log(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{role}.harvest.log"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(role: &str) -> Brief {
        Brief {
            role: role.into(),
            teammate: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            record_id: "r1".into(),
            session_id: "s1".into(),
            resume: false,
            task: "do the thing".into(),
            project_dir: "/tmp/proj".into(),
            state_dir: None,
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: None,
        }
    }

    /// A pane is a fresh shell, so an agent-CLI override only reaches the worker
    /// if it survives the brief round-trip.
    #[test]
    fn brief_carries_agent_cli_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        let mut b = brief("sonnet-1");
        b.claude_bin = Some("claude".into());
        mb.write_brief(&b).unwrap();

        let back = mb.read_brief("sonnet-1").unwrap();
        assert_eq!(back.claude_bin.as_deref(), Some("claude"));
        assert_eq!(back.codex_bin, None);
    }

    /// Briefs written before these fields existed must still load.
    #[test]
    fn brief_without_overrides_still_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(
            mb.dir().join("old.brief.json"),
            r#"{"role":"old","teammate":"sonnet","agent":"claude","model":"sonnet",
                "record_id":"r1","session_id":"s1","resume":false,
                "task":"t","project_dir":"/tmp/proj"}"#,
        )
        .unwrap();

        let back = mb.read_brief("old").unwrap();
        assert_eq!(back.claude_bin, None);
        assert_eq!(back.role, "old");
    }

    #[test]
    fn mailbox_dir_is_keyed_by_workspace() {
        let mb = Mailbox::under("/tmp", "w42");
        assert_eq!(mb.dir(), Path::new("/tmp/herdr-orchestration-w42"));
    }

    #[test]
    fn roles_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(mb.dir().join("orchestrator.id"), "w1-1").unwrap();
        std::fs::write(mb.dir().join("sonnet-1.id"), "w1-2\n").unwrap();

        assert_eq!(mb.pane_for("sonnet-1").as_deref(), Some("w1-2"));
        assert_eq!(mb.pane_for("nobody"), None);
        assert_eq!(
            mb.roles(),
            vec![
                ("orchestrator".to_string(), "w1-1".to_string()),
                ("sonnet-1".to_string(), "w1-2".to_string()),
            ]
        );
    }

    /// An empty id file means "registration started but did not finish"; it must
    /// not read back as a reachable role.
    #[test]
    fn empty_id_file_is_not_a_reachable_role() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(mb.dir().join("half.id"), "  \n").unwrap();
        assert_eq!(mb.pane_for("half"), None);
        assert!(mb.roles().is_empty());
    }

    /// The bash version quoted briefs with `printf %q`, which mangled tasks
    /// containing quotes and newlines. JSON must survive them intact.
    #[test]
    fn brief_survives_shell_hostile_tasks() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        let mut b = brief("sonnet-1");
        b.task = "fix \"quotes\", $VARS, `backticks`\nand newlines\\".into();
        b.project_dir = r"C:\Users\a b\proj".into();
        mb.write_brief(&b).unwrap();

        let back = mb.read_brief("sonnet-1").unwrap();
        assert_eq!(back.task, b.task);
        assert_eq!(back.project_dir, b.project_dir);
        assert_eq!(back.teammate, "sonnet");
        assert_eq!(back.agent().unwrap(), Agent::Claude);
    }

    #[test]
    fn role_taken_covers_both_id_and_brief() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        assert!(!mb.role_taken("sonnet-1"));
        mb.write_brief(&brief("sonnet-1")).unwrap();
        assert!(mb.role_taken("sonnet-1"));
        mb.unregister("sonnet-1");
        assert!(!mb.role_taken("sonnet-1"));
    }

    #[test]
    fn seq_counter_is_per_teammate_and_monotonic() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        assert_eq!(mb.next_seq("sonnet").unwrap(), 1);
        assert_eq!(mb.next_seq("sonnet").unwrap(), 2);
        assert_eq!(mb.next_seq("opus").unwrap(), 1);
        assert_eq!(mb.next_seq("sonnet").unwrap(), 3);
    }
}
