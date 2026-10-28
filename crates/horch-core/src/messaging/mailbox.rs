//! Per-workspace mailbox: the role -> pane-id registry that makes `horch tell`
//! possible, plus the brief a spawned worker reads to learn who it is.
//!
//! Lives under horch's state folder keyed by workspace id, so the OS cannot
//! clear it from under an idle fleet. A fleet started before that move still
//! has its registry in the system temp dir; reads fall back to it, writes
//! never go there, and only [`Mailbox::unregister`] deletes from it.

use std::path::{Path, PathBuf};

use crate::runtime::RuntimeContext;
use crate::workspace::client::WorkspaceClient;
use crate::workspace::herdr::Herdr;
use anyhow::{bail, Context, Result};

/// The brief moved to [`crate::messaging::brief`] in A2.
use crate::messaging::brief::Brief;

/// A brief's file mode: what `std::fs::write` gave it under the usual umask.
const BRIEF_MODE: u32 = 0o644;

/// Whether herdr still has a registered role's pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneState {
    Open,
    /// herdr answered and does not list the pane.
    Closed,
    /// herdr did not answer.
    Unknown,
}

/// One registered role, as [`Mailbox::role_states`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleEntry {
    pub role: String,
    pub pane: String,
    pub state: PaneState,
}

/// The mailbox directory for one herdr workspace.
#[derive(Debug, Clone)]
pub struct Mailbox {
    dir: PathBuf,
    /// The pre-upgrade mailbox in the temp dir, read when `dir` lacks a file.
    /// Nothing is written to it; `unregister` removes a role's files from it.
    legacy_dir: Option<PathBuf>,
    workspace_id: String,
}

impl Mailbox {
    /// A mailbox handle for `workspace_id` under the state folder
    /// (`RuntimeContext::paths.state_root`).
    pub fn under(state_root: impl AsRef<Path>, workspace_id: &str) -> Self {
        Self {
            dir: Self::dir_in(state_root.as_ref(), workspace_id),
            legacy_dir: None,
            workspace_id: workspace_id.to_string(),
        }
    }

    fn dir_in(root: &Path, workspace_id: &str) -> PathBuf {
        root.join(format!("herdr-orchestration-{workspace_id}"))
    }

    /// Also read the registry that an older horch left under `temp_root`.
    pub fn with_legacy_root(mut self, temp_root: impl AsRef<Path>) -> Self {
        self.legacy_dir = Some(Self::dir_in(temp_root.as_ref(), &self.workspace_id));
        self
    }

    /// [`Mailbox::under`] the context's state folder, with its temp dir as
    /// the fallback for reads and for removing a role.
    pub fn in_context(ctx: &RuntimeContext, workspace_id: &str) -> Self {
        Self::under(&ctx.paths.state_root, workspace_id).with_legacy_root(&ctx.paths.temp_root)
    }

    /// Resolve the workspace this process belongs to, then open its mailbox.
    ///
    /// `workspace` is `HORCH_WORKSPACE_ID`, which a registered pane hands to
    /// its children, so they hit the cheap path. Otherwise fall back to this
    /// pane's own `HERDR_PANE_ID` (`pane`) - herdr exports an internal id there
    /// (`p_2`), which `herdr pane get` upgrades to the public workspace id. That
    /// fallback is what lets these commands also work from an unregistered pane.
    fn resolve(
        herdr: &Herdr,
        state_root: &Path,
        temp_root: &Path,
        workspace: Option<&str>,
        pane: Option<&str>,
    ) -> Result<Self> {
        let open = |ws: &str| Self::under(state_root, ws).with_legacy_root(temp_root);
        if let Some(ws) = workspace.filter(|ws| !ws.is_empty()) {
            return Ok(open(ws));
        }
        let Some(pane_id) = pane.filter(|p| !p.is_empty()) else {
            bail!(
                "not inside a herdr pane (HERDR_PANE_ID is unset) and HORCH_WORKSPACE_ID is not set"
            )
        };
        let pane = herdr.pane_get(pane_id)?;
        let ws = pane
            .workspace_id
            .with_context(|| format!("herdr did not report a workspace_id for pane {pane_id}"))?;
        Ok(open(&ws))
    }

    /// [`Mailbox::resolve`] with the context's dirs, workspace and pane.
    pub fn resolve_in(herdr: &Herdr, ctx: &RuntimeContext) -> Result<Self> {
        Self::resolve(
            herdr,
            &ctx.paths.state_root,
            &ctx.paths.temp_root,
            ctx.herdr.workspace.as_ref().map(|w| w.as_str()),
            ctx.herdr.pane.as_ref().map(|p| p.as_str()),
        )
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn exists(&self) -> bool {
        self.dir.is_dir() || self.legacy_dir.as_ref().is_some_and(|d| d.is_dir())
    }

    /// Where to read `file`: the state folder, else the old temp location
    /// when only that has it. Writes use `self.dir` directly.
    fn read_path(&self, file: &str) -> PathBuf {
        let path = self.dir.join(file);
        match &self.legacy_dir {
            Some(legacy) if !path.exists() && legacy.join(file).exists() => legacy.join(file),
            _ => path,
        }
    }

    fn id_file(role: &str) -> String {
        format!("{role}.id")
    }

    fn brief_file(role: &str) -> String {
        format!("{role}.brief.json")
    }

    fn id_read_path(&self, role: &str) -> PathBuf {
        self.read_path(&Self::id_file(role))
    }

    fn brief_read_path(&self, role: &str) -> PathBuf {
        self.read_path(&Self::brief_file(role))
    }

    /// Record this pane's public id under `role` so other panes can address it.
    ///
    /// `pane` is this pane's `HERDR_PANE_ID`. Returns the mailbox and the
    /// resolved public pane id. The caller hands [`Mailbox::workspace_id`] to
    /// its children as `HORCH_WORKSPACE_ID`; this process's environment is
    /// left alone.
    fn register(
        herdr: &Herdr,
        state_root: &Path,
        temp_root: &Path,
        pane: Option<&str>,
        role: &str,
    ) -> Result<(Self, String)> {
        let internal = pane
            .filter(|s| !s.is_empty())
            .context("must run inside a herdr pane (HERDR_PANE_ID is unset)")?;
        let pane = herdr.pane_get(internal)?;
        let ws = pane
            .workspace_id
            .clone()
            .with_context(|| format!("herdr did not report a workspace_id for pane {internal}"))?;
        let mailbox = Self::under(state_root, &ws).with_legacy_root(temp_root);
        std::fs::create_dir_all(&mailbox.dir)
            .with_context(|| format!("creating mailbox {}", mailbox.dir.display()))?;
        std::fs::write(mailbox.dir.join(Self::id_file(role)), &pane.pane_id)
            .with_context(|| format!("registering role '{role}'"))?;
        Ok((mailbox, pane.pane_id))
    }

    /// [`Mailbox::register`] this context's pane, then record the workspace
    /// in the context, so everything this process does next, and every child
    /// it starts, belongs to that workspace.
    pub fn register_in(
        herdr: &Herdr,
        ctx: &mut RuntimeContext,
        role: &str,
    ) -> Result<(Self, String)> {
        let pane = ctx.herdr.pane.as_ref().map(|p| p.to_string());
        let (mailbox, pane_id) = Self::register(
            herdr,
            &ctx.paths.state_root,
            &ctx.paths.temp_root,
            pane.as_deref(),
            role,
        )?;
        ctx.herdr.workspace = Some(crate::ids::WorkspaceId::new(mailbox.workspace_id())?);
        Ok((mailbox, pane_id))
    }

    /// The pane id registered for `role`, if any.
    pub fn pane_for(&self, role: &str) -> Option<String> {
        let raw = std::fs::read_to_string(self.id_read_path(role)).ok()?;
        let trimmed = raw.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// When `role` registered: the modification time of its id file.
    pub fn registered_at(&self, role: &str) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.id_read_path(role))
            .ok()?
            .modified()
            .ok()
    }

    /// Every registered `(role, pane_id)`, sorted by role.
    pub fn roles(&self) -> Vec<(String, String)> {
        let mut out = Self::roles_in(&self.dir);
        if let Some(legacy) = &self.legacy_dir {
            for (role, pane) in Self::roles_in(legacy) {
                if !self.dir.join(Self::id_file(&role)).exists() {
                    out.push((role, pane));
                }
            }
        }
        out.sort();
        out
    }

    fn roles_in(dir: &Path) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
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
            .collect()
    }

    /// Every registered role with whether herdr still has its pane, from one
    /// `pane list` of this workspace. A worker whose pane closed without
    /// `horch done` leaves its id file behind; this is how `horch inbox`
    /// tells it apart. When herdr does not answer, every pane is
    /// [`PaneState::Unknown`].
    pub fn role_states(&self, ws: &dyn WorkspaceClient) -> Vec<RoleEntry> {
        let open: Option<Vec<String>> = ws
            .pane_list(&self.workspace_id)
            .ok()
            .map(|panes| panes.into_iter().map(|p| p.pane_id).collect());
        self.roles()
            .into_iter()
            .map(|(role, pane)| {
                let state = match &open {
                    None => PaneState::Unknown,
                    Some(open) if open.contains(&pane) => PaneState::Open,
                    Some(_) => PaneState::Closed,
                };
                RoleEntry { role, pane, state }
            })
            .collect()
    }

    /// Map of pane id -> role, for labelling layout output.
    pub(crate) fn panes_to_roles(&self) -> Vec<(String, String)> {
        self.roles().into_iter().map(|(r, p)| (p, r)).collect()
    }

    /// Write `brief` atomically: the worker never reads half of one.
    pub fn write_brief(&self, brief: &Brief) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating mailbox {}", self.dir.display()))?;
        let path = self.dir.join(Self::brief_file(&brief.role));
        let json = serde_json::to_string_pretty(brief)?;
        crate::fsx::write_atomic(&path, json.as_bytes(), BRIEF_MODE)
            .with_context(|| format!("writing brief {}", path.display()))
    }

    pub fn read_brief(&self, role: &str) -> Result<Brief> {
        let path = self.brief_read_path(role);
        let raw = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "no brief at {} (spawn this worker with `horch spawn`)",
                path.display()
            )
        })?;
        Brief::from_json(&raw).with_context(|| format!("parsing brief {}", path.display()))
    }

    /// True when anything already claims this role in this workspace.
    pub fn role_taken(&self, role: &str) -> bool {
        self.id_read_path(role).exists() || self.brief_read_path(role).exists()
    }

    /// Drop a role's registration and brief, so the role name frees up.
    /// A removal that fails is logged; [`Self::try_unregister`] returns it.
    pub fn unregister(&self, role: &str) {
        if let Err(e) = self.try_unregister(role) {
            eprintln!("horch: {e:#}");
        }
    }

    /// Drop a role's registration and brief, and the discovery files its
    /// launch left. Every file is tried; the error names the first removal of
    /// a registration file that failed. Discovery files are best effort.
    pub fn try_unregister(&self, role: &str) -> Result<()> {
        let mut first_error = None;
        for dir in std::iter::once(&self.dir).chain(&self.legacy_dir) {
            for file in [Self::id_file(role), Self::brief_file(role)] {
                let path = dir.join(file);
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        first_error.get_or_insert(anyhow::Error::new(e).context(format!(
                            "removing {} failed; role '{role}' stays registered",
                            path.display()
                        )));
                    }
                }
            }
            for file in [
                format!(".{role}.launch-marker"),
                format!("{role}.harvest.log"),
            ] {
                let _ = std::fs::remove_file(dir.join(file));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Next per-teammate ordinal for auto-naming a role `<teammate>-<n>`.
    pub(crate) fn next_seq(&self, teammate: &str) -> Result<u32> {
        std::fs::create_dir_all(&self.dir)?;
        let file = format!(".seq-{teammate}");
        let path = self.dir.join(&file);
        // The first count after the upgrade continues the old fleet's count.
        let current: u32 = std::fs::read_to_string(self.read_path(&file))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let next = current + 1;
        std::fs::write(&path, next.to_string())?;
        Ok(next)
    }

    /// Marker file used to bound the codex rollout-file search to sessions
    /// created after this worker launched.
    pub(crate) fn launch_marker(&self, role: &str) -> PathBuf {
        self.dir.join(format!(".{role}.launch-marker"))
    }

    /// The marker to read: a worker launched before the upgrade left its
    /// marker in the old temp location.
    pub(crate) fn launch_marker_read_path(&self, role: &str) -> PathBuf {
        self.read_path(&format!(".{role}.launch-marker"))
    }

    pub(crate) fn harvest_log(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{role}.harvest.log"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::SessionMode;
    use crate::harness::HarnessKind;
    use crate::runtime::BinOverrides;

    fn brief(role: &str) -> Brief {
        Brief {
            schema: crate::messaging::brief::SCHEMA,
            role: role.into(),
            teammate: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            record_id: "r1".into(),
            session: SessionMode::Fresh(Some("s1".parse().unwrap())),
            task: "do the thing".into(),
            project_dir: "/tmp/proj".into(),
            state_dir: None,
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: None,
            workdir: None,
            bin_overrides: BinOverrides::default(),
            report_to: crate::execution::ReportTarget::Orchestrator,
            worker_id: None,
            ledger_db: None,
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

    /// The session mode is typed in memory but keeps the brief's two fields
    /// on disk, so a worker from either side of A1 reads the other's brief.
    #[test]
    fn arc_04_brief_session_wire_unchanged() {
        let s1: crate::ids::SessionId = "s1".parse().unwrap();
        for (session, id, resume) in [
            (SessionMode::Fresh(None), "", false),
            (SessionMode::Fresh(Some(s1.clone())), "s1", false),
            (SessionMode::Resume(s1), "s1", true),
        ] {
            let mut b = brief("r");
            b.session = session.clone();
            let v = serde_json::to_value(&b).unwrap();
            assert_eq!(v["session_id"], id);
            assert_eq!(v["resume"], resume);
            assert!(v.get("session").is_none());
            let back: Brief = serde_json::from_value(v).unwrap();
            assert_eq!(back.session, session);
        }

        // A missing session_id is a fresh session whose id is not known yet.
        let v = serde_json::json!({"role":"r","teammate":"t","agent":"codex","model":"m",
            "record_id":"r1","resume":false,"project_dir":"/p"});
        let b: Brief = serde_json::from_value(v).unwrap();
        assert_eq!(b.session, SessionMode::Fresh(None));

        let v = serde_json::json!({"role":"r","teammate":"t","agent":"claude","model":"m",
            "record_id":"r1","session_id":"","resume":true,"project_dir":"/p"});
        assert!(serde_json::from_value::<Brief>(v).is_err());
    }

    #[test]
    fn mailbox_dir_is_keyed_by_workspace() {
        let mb = Mailbox::under("/state", "w42");
        assert_eq!(mb.dir(), Path::new("/state/herdr-orchestration-w42"));
    }

    /// A fleet started before the upgrade keeps its registry in the temp dir.
    fn legacy_mailbox(state: &Path, temp: &Path) -> (Mailbox, PathBuf) {
        let legacy = temp.join("herdr-orchestration-w1");
        std::fs::create_dir_all(&legacy).unwrap();
        (Mailbox::under(state, "w1").with_legacy_root(temp), legacy)
    }

    #[test]
    fn registry_found_only_in_the_old_temp_location() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("orchestrator.id"), "w1-1").unwrap();
        std::fs::write(legacy.join("sonnet-1.id"), "w1-2").unwrap();
        let mut b = brief("sonnet-1");
        b.task = "old task".into();
        std::fs::write(
            legacy.join("sonnet-1.brief.json"),
            serde_json::to_string(&b).unwrap(),
        )
        .unwrap();

        assert!(mb.exists());
        assert_eq!(mb.pane_for("sonnet-1").as_deref(), Some("w1-2"));
        assert!(mb.registered_at("sonnet-1").is_some());
        assert_eq!(mb.roles().len(), 2);
        assert_eq!(mb.read_brief("sonnet-1").unwrap().task, "old task");
        assert!(mb.role_taken("sonnet-1"));

        mb.unregister("sonnet-1");
        assert_eq!(mb.pane_for("sonnet-1"), None);
        assert!(!mb.role_taken("sonnet-1"));
    }

    #[test]
    fn writes_go_only_to_the_state_dir_and_win_over_the_old_location() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("sonnet-1.id"), "old-pane").unwrap();
        mb.write_brief(&brief("sonnet-2")).unwrap();
        std::fs::write(mb.dir().join("sonnet-1.id"), "new-pane").unwrap();

        assert_eq!(mb.pane_for("sonnet-1").as_deref(), Some("new-pane"));
        assert_eq!(
            mb.roles(),
            vec![("sonnet-1".to_string(), "new-pane".to_string())]
        );
        assert!(mb.dir().join("sonnet-2.brief.json").exists());
        assert!(!legacy.join("sonnet-2.brief.json").exists());
        assert_eq!(mb.next_seq("sonnet").unwrap(), 1);
        assert!(!legacy.join(".seq-sonnet").exists());
    }

    #[test]
    fn roles_merge_across_the_state_and_old_locations() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("old-1.id"), "w1-2").unwrap();
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(mb.dir().join("new-1.id"), "w1-3").unwrap();

        assert_eq!(
            mb.roles(),
            vec![
                ("new-1".to_string(), "w1-3".to_string()),
                ("old-1".to_string(), "w1-2".to_string()),
            ]
        );
        assert_eq!(mb.pane_for("old-1").as_deref(), Some("w1-2"));
        assert_eq!(mb.pane_for("new-1").as_deref(), Some("w1-3"));
    }

    #[test]
    fn unregister_removes_a_role_held_in_both_locations() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("sonnet-1.id"), "old-pane").unwrap();
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(mb.dir().join("sonnet-1.id"), "new-pane").unwrap();
        mb.write_brief(&brief("sonnet-1")).unwrap();

        mb.unregister("sonnet-1");

        assert!(!legacy.join("sonnet-1.id").exists());
        assert!(!mb.dir().join("sonnet-1.id").exists());
        assert_eq!(mb.pane_for("sonnet-1"), None);
        assert!(!mb.role_taken("sonnet-1"));
    }

    /// An upgraded fleet keeps counting from its old counter, so the next
    /// auto-name does not collide with a role that is still registered.
    #[test]
    fn seq_counter_continues_from_the_old_location() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join(".seq-sonnet"), "4").unwrap();

        assert_eq!(mb.next_seq("sonnet").unwrap(), 5);
        assert_eq!(mb.next_seq("sonnet").unwrap(), 6);
        assert_eq!(
            std::fs::read_to_string(legacy.join(".seq-sonnet")).unwrap(),
            "4"
        );
    }

    /// A removal that fails for a reason other than "missing" does not panic
    /// and leaves the role registered, so the warning is the only signal.
    #[test]
    fn unregister_survives_a_removal_that_fails() {
        let state = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(state.path(), "w1");
        let id = mb.dir().join("sonnet-1.id");
        std::fs::create_dir_all(id.join("not-a-file")).unwrap();

        mb.unregister("sonnet-1");

        assert!(id.exists());
        assert!(mb.role_taken("sonnet-1"));
    }

    /// A removal that fails for a reason other than "missing" leaves the role
    /// registered, and the caller gets the error with the path.
    #[test]
    fn try_unregister_returns_a_removal_that_fails() {
        let state = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(state.path(), "w1");
        let id = mb.dir().join("sonnet-1.id");
        std::fs::create_dir_all(id.join("not-a-file")).unwrap();

        let error = mb.try_unregister("sonnet-1").unwrap_err();

        let text = format!("{error:#}");
        assert!(text.contains(&id.display().to_string()), "{text}");
        assert!(text.contains("sonnet-1"), "{text}");
        assert!(mb.role_taken("sonnet-1"));
    }

    /// A discovery that failed leaves a harvest log, and one that never found
    /// its session leaves the marker; unregistering the role removes both,
    /// from the state folder and the old location.
    #[test]
    fn unregister_removes_the_launch_marker_and_harvest_log() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::create_dir_all(mb.dir()).unwrap();
        for dir in [mb.dir(), legacy.as_path()] {
            std::fs::write(dir.join(".sonnet-1.launch-marker"), "").unwrap();
            std::fs::write(dir.join("sonnet-1.harvest.log"), "could not capture").unwrap();
        }
        std::fs::write(mb.dir().join(".sonnet-2.launch-marker"), "").unwrap();

        mb.try_unregister("sonnet-1").unwrap();

        for dir in [mb.dir(), legacy.as_path()] {
            assert!(!dir.join(".sonnet-1.launch-marker").exists());
            assert!(!dir.join("sonnet-1.harvest.log").exists());
        }
        assert!(mb.dir().join(".sonnet-2.launch-marker").exists());
    }

    #[test]
    fn launch_marker_read_path_finds_the_old_location() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join(".sonnet-1.launch-marker"), "").unwrap();

        assert_eq!(
            mb.launch_marker_read_path("sonnet-1"),
            legacy.join(".sonnet-1.launch-marker")
        );
        assert_eq!(
            mb.launch_marker("sonnet-1"),
            mb.dir().join(".sonnet-1.launch-marker")
        );
    }

    #[test]
    fn context_mailbox_lives_in_the_state_root() {
        let env = crate::runtime::MapEnv::new("/cwd")
            .with("HOME", "/home/a")
            .with("HORCH_STATE_DIR", "/s")
            .with("TMPDIR", "/t");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let mb = Mailbox::in_context(&ctx, "w1");
        assert_eq!(mb.dir(), Path::new("/s/herdr-orchestration-w1"));
        assert_eq!(
            mb.legacy_dir.as_deref(),
            Some(Path::new("/t/herdr-orchestration-w1"))
        );
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
        assert_eq!(back.agent().unwrap(), HarnessKind::Claude);
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
