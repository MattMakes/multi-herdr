use std::path::{Path, PathBuf};

use crate::runtime::RuntimeContext;
use crate::workspace::client::WorkspaceClient;
use crate::workspace::host::PaneHost;
use anyhow::{bail, Context, Result};

use crate::messaging::brief::Brief;
use crate::messaging::private;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneState {
    Open,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleEntry {
    pub role: String,
    pub pane: String,
    pub state: PaneState,
}

#[derive(Debug, Clone)]
pub struct Mailbox {
    dir: PathBuf,
    legacy_dir: Option<PathBuf>,
    workspace_id: String,
}

impl Mailbox {
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

    pub fn with_legacy_root(mut self, temp_root: impl AsRef<Path>) -> Self {
        self.legacy_dir = Some(Self::dir_in(temp_root.as_ref(), &self.workspace_id));
        self
    }

    pub fn in_context(ctx: &RuntimeContext, workspace_id: &str) -> Self {
        Self::under(&ctx.paths.state_root, workspace_id).with_legacy_root(&ctx.paths.temp_root)
    }

    fn resolve(
        herdr: &PaneHost,
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
                "not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset) and HORCH_WORKSPACE_ID is not set"
            )
        };
        let pane = herdr.pane_get(pane_id)?;
        let ws = pane
            .workspace_id
            .with_context(|| format!("herdr did not report a workspace_id for pane {pane_id}"))?;
        Ok(open(&ws))
    }

    pub fn resolve_in(herdr: &PaneHost, ctx: &RuntimeContext) -> Result<Self> {
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
        self.settle();
        self.dir.is_dir()
    }

    fn read_path(&self, file: &str) -> PathBuf {
        self.settle();
        self.dir.join(file)
    }

    fn settle(&self) {
        let Some(legacy) = &self.legacy_dir else {
            return;
        };
        if self.dir.join(LEGACY_COPIED).exists() || !legacy.exists() {
            return;
        }
        if let Err(e) = self.copy_legacy(legacy) {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!(
                    "horch: the old registry {} is not used: {e:#}",
                    legacy.display()
                );
            });
        }
    }

    fn copy_legacy(&self, legacy: &Path) -> Result<()> {
        private::check_shared(legacy)?;
        if self.dir.is_dir() {
            private::check(&self.dir)?;
            copy_plain_files(legacy, &self.dir, false)?;
            return private::write(&self.dir.join(LEGACY_COPIED), b"");
        }
        let parent = self
            .dir
            .parent()
            .context("the registry has no parent dir")?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let staging = parent.join(format!(
            ".{}.copy-{}",
            self.dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("registry"),
            uuid::Uuid::new_v4().simple()
        ));
        let copied = (|| {
            private::ensure_dir(&staging)?;
            copy_plain_files(legacy, &staging, true)?;
            private::write(&staging.join(LEGACY_COPIED), b"")?;
            std::fs::rename(&staging, &self.dir)
                .with_context(|| format!("moving the copy to {}", self.dir.display()))
        })();
        if copied.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
            if self.dir.is_dir() {
                return Ok(());
            }
        }
        copied
    }

    fn trusted(&self) -> bool {
        match private::check(&self.dir) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("horch: {e:#}");
                false
            }
        }
    }

    fn ensure_dir(&self) -> Result<()> {
        self.settle();
        private::ensure_dir(&self.dir).with_context(|| format!("mailbox {}", self.dir.display()))
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

    fn locate(
        herdr: &PaneHost,
        state_root: &Path,
        temp_root: &Path,
        pane: Option<&str>,
    ) -> Result<(Self, String)> {
        let internal = pane
            .filter(|s| !s.is_empty())
            .context("not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset)")?;
        let pane = herdr.pane_get(internal)?;
        let ws = pane
            .workspace_id
            .clone()
            .with_context(|| format!("herdr did not report a workspace_id for pane {internal}"))?;
        let mailbox = Self::under(state_root, &ws).with_legacy_root(temp_root);
        mailbox.ensure_dir()?;
        Ok((mailbox, pane.pane_id))
    }

    pub fn register_role(&self, role: &str, pane: &str) -> Result<()> {
        private::write(&self.dir.join(Self::id_file(role)), pane.as_bytes())
            .with_context(|| format!("registering role '{role}'"))
    }

    pub fn locate_in(herdr: &PaneHost, ctx: &mut RuntimeContext) -> Result<(Self, String)> {
        let pane = ctx.herdr.pane.as_ref().map(|p| p.to_string());
        let (mailbox, pane_id) = Self::locate(
            herdr,
            &ctx.paths.state_root,
            &ctx.paths.temp_root,
            pane.as_deref(),
        )?;
        ctx.herdr.workspace = Some(crate::ids::WorkspaceId::new(mailbox.workspace_id())?);
        Ok((mailbox, pane_id))
    }

    pub fn register_in(
        herdr: &PaneHost,
        ctx: &mut RuntimeContext,
        role: &str,
    ) -> Result<(Self, String)> {
        let (mailbox, pane_id) = Self::locate_in(herdr, ctx)?;
        mailbox.register_role(role, &pane_id)?;
        Ok((mailbox, pane_id))
    }

    pub fn pane_for(&self, role: &str) -> Option<String> {
        let path = self.id_read_path(role);
        if !self.trusted() {
            return None;
        }
        let raw = std::fs::read_to_string(path).ok()?;
        let trimmed = raw.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn registered_at(&self, role: &str) -> Option<std::time::SystemTime> {
        std::fs::metadata(self.id_read_path(role))
            .ok()?
            .modified()
            .ok()
    }

    pub fn roles(&self) -> Vec<(String, String)> {
        self.settle();
        if !self.trusted() {
            return Vec::new();
        }
        let mut out = Self::roles_in(&self.dir);
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

    pub(crate) fn panes_to_roles(&self) -> Vec<(String, String)> {
        self.roles().into_iter().map(|(r, p)| (p, r)).collect()
    }

    pub fn write_brief(&self, brief: &Brief) -> Result<()> {
        self.ensure_dir()?;
        let path = self.dir.join(Self::brief_file(&brief.role));
        let json = serde_json::to_string_pretty(brief)?;
        private::write(&path, json.as_bytes()).context("writing the brief")
    }

    pub fn read_brief(&self, role: &str) -> Result<Brief> {
        let path = self.brief_read_path(role);
        private::check(&self.dir)?;
        let raw = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "no brief at {} (spawn this worker with `horch spawn`)",
                path.display()
            )
        })?;
        Brief::from_json(&raw).with_context(|| format!("parsing brief {}", path.display()))
    }

    pub fn role_taken(&self, role: &str) -> bool {
        self.id_read_path(role).exists() || self.brief_read_path(role).exists()
    }

    pub fn unregister(&self, role: &str) {
        if let Err(e) = self.try_unregister(role) {
            eprintln!("horch: {e:#}");
        }
    }

    pub fn try_unregister(&self, role: &str) -> Result<()> {
        let mut first_error = None;
        let groups = [
            (
                [Self::launch_marker_file(role), Self::harvest_log_file(role)],
                "a new launch of the role may fail",
            ),
            (
                [Self::id_file(role), Self::brief_file(role)],
                "the role stays registered",
            ),
        ];
        for (files, consequence) in &groups {
            for dir in std::iter::once(&self.dir).chain(&self.legacy_dir) {
                for file in files {
                    let path = dir.join(file);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            first_error.get_or_insert(anyhow::Error::new(e).context(format!(
                                "removing {} failed; {consequence} (role '{role}')",
                                path.display()
                            )));
                        }
                    }
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub(crate) fn next_seq(&self, teammate: &str) -> Result<u32> {
        self.ensure_dir()?;
        let file = format!(".seq-{teammate}");
        let path = self.dir.join(&file);
        let current: u32 = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let next = current + 1;
        private::write(&path, next.to_string().as_bytes())?;
        Ok(next)
    }

    pub(crate) fn launch_marker(&self, role: &str) -> PathBuf {
        self.dir.join(Self::launch_marker_file(role))
    }

    pub(crate) fn launch_marker_read_path(&self, role: &str) -> PathBuf {
        self.read_path(&Self::launch_marker_file(role))
    }

    pub(crate) fn harvest_log(&self, role: &str) -> PathBuf {
        self.dir.join(Self::harvest_log_file(role))
    }

    fn launch_marker_file(role: &str) -> String {
        format!(".{role}.launch-marker")
    }

    fn harvest_log_file(role: &str) -> String {
        format!("{role}.harvest.log")
    }
}

const LEGACY_COPIED: &str = ".legacy-copied";

fn copy_plain_files(from: &Path, to: &Path, replace: bool) -> Result<()> {
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let target = to.join(entry.file_name());
        if !replace && target.exists() {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        private::write(&target, &bytes)?;
    }
    Ok(())
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
            controller: None,
            backend: Default::default(),
        }
    }

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
    fn the_old_location_is_not_read_once_the_state_folder_has_a_registry() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("old-1.id"), "w1-2").unwrap();
        assert_eq!(mb.pane_for("old-1").as_deref(), Some("w1-2"), "copied");
        std::fs::write(legacy.join("orchestrator.id"), "planted").unwrap();
        std::fs::write(mb.dir().join("new-1.id"), "w1-3").unwrap();

        assert_eq!(
            mb.roles(),
            vec![
                ("new-1".to_string(), "w1-3".to_string()),
                ("old-1".to_string(), "w1-2".to_string()),
            ]
        );
        assert_eq!(mb.pane_for("orchestrator"), None);
    }

    #[test]
    fn a_split_registry_gets_the_old_files_it_lacks_once() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("herdr-orchestration-w1");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("orchestrator.id"), "w1-1").unwrap();
        std::fs::write(legacy.join("sonnet-1.id"), "old-pane").unwrap();
        let split = state.path().join("herdr-orchestration-w1");
        std::fs::create_dir_all(&split).unwrap();
        std::fs::write(split.join("sonnet-1.id"), "w1-2").unwrap();
        let mb = Mailbox::under(state.path(), "w1").with_legacy_root(temp.path());

        assert_eq!(mb.pane_for("orchestrator").as_deref(), Some("w1-1"));
        assert_eq!(mb.pane_for("sonnet-1").as_deref(), Some("w1-2"));
        std::fs::write(legacy.join("planted.id"), "w1-9").unwrap();
        assert_eq!(mb.pane_for("planted"), None, "the copy ran only once");
    }

    #[cfg(unix)]
    #[test]
    fn an_old_registry_that_is_a_link_is_not_copied() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let elsewhere = temp.path().join("elsewhere");
        private::ensure_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("orchestrator.id"), "planted").unwrap();
        std::os::unix::fs::symlink(&elsewhere, temp.path().join("herdr-orchestration-w1")).unwrap();
        let mb = Mailbox::under(state.path(), "w1").with_legacy_root(temp.path());

        assert_eq!(mb.pane_for("orchestrator"), None);
        assert!(!mb.dir().exists(), "nothing was copied");
    }

    #[cfg(unix)]
    #[test]
    fn an_old_registry_others_can_write_is_not_copied() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("orchestrator.id"), "planted").unwrap();
        std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o777)).unwrap();

        assert_eq!(mb.pane_for("orchestrator"), None);
        assert!(mb.roles().is_empty());
        assert!(!mb.exists());
        assert!(!mb.dir().exists(), "nothing was copied");
        assert!(mb.read_brief("orchestrator").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_registry_others_can_write_is_refused_by_name() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(state.path(), "w1");
        mb.write_brief(&brief("sonnet-1")).unwrap();
        std::fs::write(mb.dir().join("orchestrator.id"), "w1-1").unwrap();
        std::fs::set_permissions(mb.dir(), std::fs::Permissions::from_mode(0o777)).unwrap();

        let err = format!("{:#}", mb.read_brief("sonnet-1").unwrap_err());
        assert!(err.contains(&mb.dir().display().to_string()), "{err}");
        assert!(err.contains("writable by other users"), "{err}");
        assert_eq!(mb.pane_for("orchestrator"), None);
        assert!(mb.write_brief(&brief("sonnet-2")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn registry_dirs_are_0700_and_files_0600() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let state = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(state.path(), "w1");
        mb.write_brief(&brief("sonnet-1")).unwrap();
        mb.next_seq("sonnet").unwrap();
        assert_eq!(mode(mb.dir()), 0o700);
        assert_eq!(mode(&mb.dir().join("sonnet-1.brief.json")), 0o600);
        assert_eq!(mode(&mb.dir().join(".seq-sonnet")), 0o600);

        let temp = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join("orchestrator.id"), "w1-1").unwrap();
        std::fs::set_permissions(
            legacy.join("orchestrator.id"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(mb.exists());
        assert_eq!(mode(mb.dir()), 0o700);
        assert_eq!(mode(&mb.dir().join("orchestrator.id")), 0o600);
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

    #[test]
    fn try_unregister_returns_a_removal_that_fails_and_tries_the_rest() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        let id = mb.dir().join("sonnet-1.id");
        std::fs::create_dir_all(id.join("not-a-file")).unwrap();
        std::fs::write(mb.dir().join("sonnet-1.brief.json"), "{}").unwrap();
        for file in [
            "sonnet-1.id",
            "sonnet-1.brief.json",
            ".sonnet-1.launch-marker",
        ] {
            std::fs::write(legacy.join(file), "").unwrap();
        }

        let error = mb.try_unregister("sonnet-1").unwrap_err();

        let text = format!("{error:#}");
        assert!(text.contains(&id.display().to_string()), "{text}");
        assert!(text.contains("sonnet-1"), "{text}");
        assert!(mb.role_taken("sonnet-1"));
        assert!(!mb.dir().join("sonnet-1.brief.json").exists());
        for file in [
            "sonnet-1.id",
            "sonnet-1.brief.json",
            ".sonnet-1.launch-marker",
        ] {
            assert!(!legacy.join(file).exists(), "{file}");
        }
    }

    #[test]
    fn try_unregister_returns_a_discovery_file_that_stays() {
        let state = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(state.path(), "w1");
        let marker = mb.dir().join(".sonnet-1.launch-marker");
        std::fs::create_dir_all(marker.join("not-a-file")).unwrap();

        let text = format!("{:#}", mb.try_unregister("sonnet-1").unwrap_err());

        assert!(text.contains(&marker.display().to_string()), "{text}");
    }

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
    fn launch_marker_of_the_old_location_arrives_with_the_copy() {
        let state = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (mb, legacy) = legacy_mailbox(state.path(), temp.path());
        std::fs::write(legacy.join(".sonnet-1.launch-marker"), "").unwrap();

        let read = mb.launch_marker_read_path("sonnet-1");
        assert_eq!(read, mb.dir().join(".sonnet-1.launch-marker"));
        assert!(read.exists());
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

    #[test]
    fn empty_id_file_is_not_a_reachable_role() {
        let tmp = tempfile::tempdir().unwrap();
        let mb = Mailbox::under(tmp.path(), "w1");
        std::fs::create_dir_all(mb.dir()).unwrap();
        std::fs::write(mb.dir().join("half.id"), "  \n").unwrap();
        assert_eq!(mb.pane_for("half"), None);
        assert!(mb.roles().is_empty());
    }

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
