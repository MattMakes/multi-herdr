//! The process environment, read once.
//!
//! The `horch` binary's bootstrap builds one [`RuntimeContext`] from
//! [`ProcessEnv`] and passes it inward. Everything else takes the values it
//! needs from that context, so a test builds the same context from a
//! [`MapEnv`] and never touches the real environment.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::bins::{BinOverrides, HarnessBins};
use super::fault::Faults;
use super::paths::{nonempty_path, Paths};
use super::process;
use crate::execution::TilingMode;
use crate::ids::{PaneId, WorkspaceId};

/// Where environment values come from.
pub trait EnvSource {
    /// The raw value, empty or not. Callers decide whether empty means unset.
    fn var(&self, key: &str) -> Option<String>;
    fn var_os(&self, key: &str) -> Option<OsString>;
    fn current_dir(&self) -> Option<PathBuf>;
    fn current_exe(&self) -> Option<PathBuf>;
    fn temp_dir(&self) -> PathBuf;
}

/// The real process environment. The only type in `horch-core` that reads it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn current_dir(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()
    }

    fn current_exe(&self) -> Option<PathBuf> {
        std::env::current_exe().ok()
    }

    fn temp_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }
}

/// A fixed environment for tests.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    pub vars: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    /// The temp dir. Defaults to `$TMPDIR` in `vars`, else `/tmp`.
    pub temp: Option<PathBuf>,
}

impl MapEnv {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        MapEnv {
            cwd: Some(cwd.into()),
            ..MapEnv::default()
        }
    }

    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_exe(mut self, exe: impl Into<PathBuf>) -> Self {
        self.exe = Some(exe.into());
        self
    }
}

impl EnvSource for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        self.vars.get(key).map(OsString::from)
    }

    fn current_dir(&self) -> Option<PathBuf> {
        self.cwd.clone()
    }

    fn current_exe(&self) -> Option<PathBuf> {
        self.exe.clone()
    }

    fn temp_dir(&self) -> PathBuf {
        self.temp
            .clone()
            .or_else(|| self.vars.get("TMPDIR").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }
}

/// The herdr pane and workspace this process runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrEnv {
    /// `HORCH_WORKSPACE_ID`: the public workspace id, set for a worker's
    /// children once it has registered.
    pub workspace: Option<WorkspaceId>,
    /// `HERDR_PANE_ID`: herdr's internal id for this pane (`p_2`).
    pub pane: Option<PaneId>,
}

/// The programs horch runs, and where it is itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bins {
    /// `HORCH_TEAMMATES_DIR`: the roster directory over the built-in one.
    pub roster_override: Option<PathBuf>,
    /// This binary, when the platform can say.
    pub current_exe: Option<PathBuf>,
    /// The `horch` beside this binary, else the one on PATH, else `horch`.
    pub horch_exe: PathBuf,
    pub harness: HarnessBins,
    pub overrides: BinOverrides,
}

impl Bins {
    /// This binary, or the error a command reports without it.
    pub fn exe(&self) -> Result<PathBuf> {
        self.current_exe
            .clone()
            .context("locating the horch binary")
    }
}

/// Switches and test hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `HORCH_NOW`, when it parses.
    pub now: Option<DateTime<Utc>>,
    /// `HORCH_NOW`, when it is set but does not parse. The clock warns once.
    pub now_unparsable: Option<String>,
    /// `HORCH_TILE`: `0`, `false`, `no` or `off` disable automatic tiling.
    pub tiling: TilingMode,
    /// `HORCH_BALANCE`, raw.
    pub balance_override: Option<String>,
    /// `HORCH_QUOTA_FILE`.
    pub quota_file: Option<PathBuf>,
    /// `HORCH_MACHINE_FILE`: a machine probe fixture.
    pub machine_file: Option<PathBuf>,
    /// `HORCH_PROBE_TIMEOUT_MS`.
    pub probe_timeout: Option<Duration>,
    /// `HORCH_TELL_GRACE_MS`: how long `horch tell` waits for herdr to see an
    /// agent in a pane whose role registered that recently. `None` is the
    /// default ([`crate::messaging::delivery::TELL_GRACE`]).
    pub tell_grace: Option<Duration>,
    /// `HORCH_FAULT`.
    pub faults: Faults,
}

/// Variables horch does not own but reads, or passes on to the tools it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inherited {
    /// `HOME` exactly as set, unlike [`Paths::home`]. The Claude settings and
    /// plugin registry are read under it.
    pub home_var: Option<OsString>,
    /// `PATH`.
    pub path: Option<OsString>,
    /// `PATHEXT` (Windows).
    pub pathext: Option<String>,
    /// `OPENCODE_CONFIG_CONTENT`, raw.
    pub opencode_config_content: Option<String>,
    /// `CODEX_HOME`.
    pub codex_home: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR`: where Claude keeps `.claude.json` when set.
    pub claude_config_dir: Option<PathBuf>,
    /// `CLAUDE_CODE_EFFORT_LEVEL`, raw.
    pub claude_code_effort_level: Option<String>,
    /// `PI_CODING_AGENT_SESSION_DIR`.
    pub pi_session_dir: Option<PathBuf>,
    /// `HORCH_OPENCODE_DB`.
    pub opencode_db: Option<PathBuf>,
    /// `XDG_DATA_HOME`.
    pub xdg_data_home: Option<PathBuf>,
    /// `LOCALAPPDATA` (Windows).
    pub local_app_data: Option<PathBuf>,
    /// `HOSTNAME`, else `COMPUTERNAME`.
    pub hostname: Option<String>,
    /// `HERDR_SESSION`.
    pub herdr_session: Option<String>,
}

impl Inherited {
    pub fn from_env(env: &dyn EnvSource) -> Inherited {
        let nonempty = |key: &str| env.var(key).filter(|s| !s.is_empty());
        Inherited {
            home_var: env.var_os("HOME"),
            path: env.var_os("PATH"),
            pathext: env.var("PATHEXT"),
            opencode_config_content: env.var("OPENCODE_CONFIG_CONTENT"),
            codex_home: nonempty_path(env, "CODEX_HOME"),
            claude_config_dir: nonempty_path(env, "CLAUDE_CONFIG_DIR"),
            claude_code_effort_level: env.var("CLAUDE_CODE_EFFORT_LEVEL"),
            pi_session_dir: nonempty_path(env, "PI_CODING_AGENT_SESSION_DIR"),
            opencode_db: nonempty_path(env, "HORCH_OPENCODE_DB"),
            xdg_data_home: nonempty_path(env, "XDG_DATA_HOME"),
            local_app_data: nonempty_path(env, "LOCALAPPDATA"),
            hostname: nonempty("HOSTNAME").or_else(|| nonempty("COMPUTERNAME")),
            herdr_session: nonempty("HERDR_SESSION"),
        }
    }
}

/// What a worker pane's agent and its `horch` children learn from the
/// transport environment (`messaging::brief::Brief::transport_env`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerEnv {
    /// `HORCH_ROLE`.
    pub role: Option<String>,
    /// `HORCH_TEAMMATE`.
    pub teammate: Option<String>,
    /// `HORCH_AGENT`.
    pub agent: Option<String>,
    /// `HORCH_MODEL`.
    pub model: Option<String>,
    /// `HORCH_RECORD_ID`.
    pub record_id: Option<String>,
    /// `HORCH_SESSION_ID`.
    pub session_id: Option<String>,
    /// `HORCH_RESUME`.
    pub resume: Option<String>,
    /// `HORCH_TASK`.
    pub task: Option<String>,
}

impl WorkerEnv {
    /// The worker variables an environment sets, or `None` when it sets none.
    /// Empty values count as unset.
    pub fn from_env(env: &dyn EnvSource) -> Option<WorkerEnv> {
        let nonempty = |key: &str| env.var(key).filter(|s| !s.is_empty());
        let w = WorkerEnv {
            role: nonempty("HORCH_ROLE"),
            teammate: nonempty("HORCH_TEAMMATE"),
            agent: nonempty("HORCH_AGENT"),
            model: nonempty("HORCH_MODEL"),
            record_id: nonempty("HORCH_RECORD_ID"),
            session_id: nonempty("HORCH_SESSION_ID"),
            resume: nonempty("HORCH_RESUME"),
            task: nonempty("HORCH_TASK"),
        };
        (w != WorkerEnv::default()).then_some(w)
    }
}

/// Everything horch reads from its environment, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    pub paths: Paths,
    pub herdr: HerdrEnv,
    pub bins: Bins,
    pub settings: Settings,
    pub inherited: Inherited,
    pub worker: Option<WorkerEnv>,
}

impl RuntimeContext {
    pub fn from_env(env: &dyn EnvSource) -> Result<RuntimeContext> {
        let nonempty = |key: &str| env.var(key).filter(|s| !s.is_empty());
        let inherited = Inherited::from_env(env);

        let herdr = HerdrEnv {
            workspace: nonempty("HORCH_WORKSPACE_ID")
                .map(WorkspaceId::new)
                .transpose()
                .context("HORCH_WORKSPACE_ID")?,
            pane: nonempty("HERDR_PANE_ID")
                .map(PaneId::new)
                .transpose()
                .context("HERDR_PANE_ID")?,
        };

        let overrides = BinOverrides::from_env(env);
        let current_exe = env.current_exe();
        let bins = Bins {
            roster_override: nonempty_path(env, "HORCH_TEAMMATES_DIR"),
            horch_exe: horch_exe(current_exe.as_deref(), &inherited),
            harness: HarnessBins::resolve(
                &overrides,
                inherited.path.as_deref(),
                inherited.pathext.as_deref(),
            ),
            current_exe,
            overrides,
        };

        let raw_now = env.var("HORCH_NOW").filter(|s| !s.trim().is_empty());
        let now = raw_now.as_deref().and_then(crate::clock::parse);
        let settings = Settings {
            now_unparsable: raw_now.filter(|_| now.is_none()),
            now,
            tiling: tiling(env.var("HORCH_TILE").as_deref()),
            balance_override: env.var("HORCH_BALANCE"),
            quota_file: nonempty_path(env, "HORCH_QUOTA_FILE"),
            machine_file: nonempty_path(env, "HORCH_MACHINE_FILE"),
            probe_timeout: nonempty("HORCH_PROBE_TIMEOUT_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            tell_grace: nonempty("HORCH_TELL_GRACE_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            faults: Faults::parse(env.var("HORCH_FAULT").as_deref()),
        };

        Ok(RuntimeContext {
            paths: Paths::from_env(env),
            herdr,
            bins,
            settings,
            inherited,
            worker: WorkerEnv::from_env(env),
        })
    }

    /// Take the bin overrides a brief carries, and resolve every program
    /// again with them.
    pub(crate) fn apply_overrides(&mut self, overrides: &BinOverrides) {
        self.bins.overrides.merge(overrides);
        self.refresh_bins();
    }

    /// Resolve every program again, after the overrides or PATH changed.
    pub(crate) fn refresh_bins(&mut self) {
        self.bins.harness = HarnessBins::resolve(
            &self.bins.overrides,
            self.inherited.path.as_deref(),
            self.inherited.pathext.as_deref(),
        );
    }

    /// Put this binary's own directory first on the PATH this context hands
    /// to children. Returns the new PATH when it changed.
    pub fn prepend_own_dir_to_path(&mut self) -> Option<OsString> {
        let exe = self.bins.current_exe.as_deref()?;
        let next = process::path_with_own_dir(exe, self.inherited.path.as_deref())?;
        self.inherited.path = Some(next.clone());
        self.refresh_bins();
        Some(next)
    }
}

/// `HORCH_TILE`: automatic tiling unless it says `0`, `false`, `no` or `off`.
pub(crate) fn tiling(raw: Option<&str>) -> TilingMode {
    match raw.unwrap_or_default() {
        "0" | "false" | "no" | "off" => TilingMode::Disabled,
        _ => TilingMode::Automatic,
    }
}

/// The `horch` a child should run: the sibling of this binary, else the one on
/// PATH, else the bare name.
fn horch_exe(current_exe: Option<&std::path::Path>, inherited: &Inherited) -> PathBuf {
    let name = format!("horch{}", std::env::consts::EXE_SUFFIX);
    if let Some(sibling) = current_exe
        .and_then(|exe| exe.parent())
        .map(|dir| dir.join(&name))
        .filter(|p| p.is_file())
    {
        return sibling;
    }
    process::which(
        inherited.path.as_deref(),
        inherited.pathext.as_deref(),
        "horch",
    )
    .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_env() -> MapEnv {
        let mut env = MapEnv::new("/cwd")
            .with_exe("/opt/horch/bin/horch")
            .with("HOME", "/home/a")
            .with("USERPROFILE", "/home/a")
            .with("PATH", "/nowhere-a2-test")
            .with("HORCH_PROJECT_DIR", "/proj")
            .with("HORCH_STATE_DIR", "/state")
            .with("XDG_DATA_HOME", "/xdg-data")
            .with("TMPDIR", "/tmp-a2")
            .with("HORCH_WORKSPACE_ID", "w7")
            .with("HERDR_PANE_ID", "p_2")
            .with("HORCH_TEAMMATES_DIR", "/roster")
            .with("HORCH_NOW", "2026-09-28T18:00:00Z")
            .with("HORCH_TILE", "off")
            .with("HORCH_BALANCE", "advise")
            .with("HORCH_QUOTA_FILE", "/q.json")
            .with("HORCH_MACHINE_FILE", "/m.json")
            .with("HORCH_PROBE_TIMEOUT_MS", "250")
            .with("HORCH_TELL_GRACE_MS", "0")
            .with("HORCH_FAULT", "after-append,abort-after-append")
            .with("OPENCODE_CONFIG_CONTENT", "{}")
            .with("CODEX_HOME", "/codex")
            .with("CLAUDE_CONFIG_DIR", "/ccd")
            .with("CLAUDE_CODE_EFFORT_LEVEL", "high")
            .with("PI_CODING_AGENT_SESSION_DIR", "/pi")
            .with("HORCH_OPENCODE_DB", "/oc.db")
            .with("LOCALAPPDATA", "/lad")
            .with("HOSTNAME", "box")
            .with("HERDR_SESSION", "s")
            .with("HORCH_ROLE", "sonnet-1")
            .with("HORCH_TEAMMATE", "sonnet")
            .with("HORCH_AGENT", "claude")
            .with("HORCH_MODEL", "sonnet")
            .with("HORCH_RECORD_ID", "r1")
            .with("HORCH_SESSION_ID", "s1")
            .with("HORCH_RESUME", "0")
            .with("HORCH_TASK", "t");
        for key in BinOverrides::VARS {
            env = env.with(key, &format!("/fake/{key}"));
        }
        env
    }

    #[test]
    fn arc_05_context_from_map_env() {
        let ctx = RuntimeContext::from_env(&full_env()).unwrap();

        assert_eq!(ctx.paths.cwd, Some(PathBuf::from("/cwd")));
        assert_eq!(ctx.paths.project_dir, Some(PathBuf::from("/proj")));
        assert_eq!(ctx.paths.state_root, PathBuf::from("/state"));
        assert_eq!(ctx.paths.state_override, Some(PathBuf::from("/state")));
        assert_eq!(ctx.paths.data_root, PathBuf::from("/xdg-data/horch"));
        assert_eq!(ctx.paths.temp_root, PathBuf::from("/tmp-a2"));
        assert_eq!(ctx.paths.home, PathBuf::from("/home/a"));

        assert_eq!(ctx.herdr.workspace.as_ref().unwrap().as_str(), "w7");
        assert_eq!(ctx.herdr.pane.as_ref().unwrap().as_str(), "p_2");

        assert_eq!(ctx.bins.roster_override, Some(PathBuf::from("/roster")));
        assert_eq!(
            ctx.bins.current_exe,
            Some(PathBuf::from("/opt/horch/bin/horch"))
        );
        assert_eq!(
            ctx.bins.harness.claude,
            PathBuf::from("/fake/HORCH_CLAUDE_BIN")
        );
        assert_eq!(ctx.bins.harness.git, PathBuf::from("/fake/HORCH_GIT_BIN"));
        assert_eq!(
            ctx.bins.harness.sqlite3,
            PathBuf::from("/fake/HORCH_SQLITE3_BIN")
        );
        assert_eq!(ctx.bins.overrides.env_pairs().len(), 10);

        let s = &ctx.settings;
        assert_eq!(s.now, crate::clock::parse("2026-09-28T18:00:00Z"));
        assert_eq!(s.now_unparsable, None);
        assert_eq!(s.tiling, TilingMode::Disabled);
        assert_eq!(s.balance_override.as_deref(), Some("advise"));
        assert_eq!(s.quota_file, Some(PathBuf::from("/q.json")));
        assert_eq!(s.machine_file, Some(PathBuf::from("/m.json")));
        assert_eq!(s.probe_timeout, Some(Duration::from_millis(250)));
        assert_eq!(s.tell_grace, Some(Duration::ZERO));
        assert!(s.faults.has("after-append") && s.faults.has("abort-after-append"));

        let i = &ctx.inherited;
        assert_eq!(i.home_var, Some(OsString::from("/home/a")));
        assert_eq!(i.opencode_config_content.as_deref(), Some("{}"));
        assert_eq!(i.codex_home, Some(PathBuf::from("/codex")));
        assert_eq!(i.claude_config_dir, Some(PathBuf::from("/ccd")));
        assert_eq!(i.claude_code_effort_level.as_deref(), Some("high"));
        assert_eq!(i.pi_session_dir, Some(PathBuf::from("/pi")));
        assert_eq!(i.opencode_db, Some(PathBuf::from("/oc.db")));
        assert_eq!(i.local_app_data, Some(PathBuf::from("/lad")));
        assert_eq!(i.hostname.as_deref(), Some("box"));
        assert_eq!(i.herdr_session.as_deref(), Some("s"));

        let w = ctx.worker.as_ref().unwrap();
        assert_eq!(w.role.as_deref(), Some("sonnet-1"));
        assert_eq!(w.record_id.as_deref(), Some("r1"));
        assert_eq!(w.task.as_deref(), Some("t"));

        // An empty environment gives the defaults.
        let ctx = RuntimeContext::from_env(&MapEnv::default()).unwrap();
        assert_eq!(ctx.paths.cwd, None);
        assert_eq!(ctx.paths.project_dir, None);
        assert_eq!(ctx.paths.home, PathBuf::from("."));
        assert_eq!(ctx.paths.state_root, PathBuf::from("./.local/state/horch"));
        assert_eq!(ctx.paths.state_override, None);
        assert_eq!(ctx.paths.data_root, PathBuf::from("./.local/share/horch"));
        assert_eq!(ctx.paths.temp_root, PathBuf::from("/tmp"));
        assert_eq!(ctx.herdr, HerdrEnv::default());
        assert_eq!(ctx.bins.roster_override, None);
        assert_eq!(ctx.bins.current_exe, None);
        assert!(ctx.bins.overrides.is_empty());
        assert_eq!(ctx.bins.harness.claude, PathBuf::from("claude"));
        assert_eq!(ctx.bins.harness.codex, PathBuf::from("codex"));
        assert_eq!(ctx.settings.now, None);
        assert_eq!(ctx.settings.tiling, TilingMode::Automatic);
        assert_eq!(ctx.settings.probe_timeout, None);
        assert_eq!(ctx.settings.tell_grace, None);
        assert!(ctx.settings.faults.is_empty());
        assert_eq!(ctx.inherited, Inherited::default());
        assert_eq!(ctx.worker, None);
    }

    /// Empty values count as unset where the old resolvers filtered them.
    #[test]
    fn empty_values_count_as_unset() {
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/cwd")
                .with("HORCH_WORKSPACE_ID", "")
                .with("HERDR_PANE_ID", "")
                .with("HORCH_STATE_DIR", "")
                .with("HORCH_PROJECT_DIR", "")
                .with("HORCH_CLAUDE_BIN", "")
                .with("HORCH_ROLE", ""),
        )
        .unwrap();
        assert_eq!(ctx.herdr, HerdrEnv::default());
        assert_eq!(ctx.paths.state_override, None);
        assert_eq!(ctx.paths.project_dir, Some(PathBuf::from("/cwd")));
        assert!(ctx.bins.overrides.is_empty());
        assert_eq!(ctx.worker, None);
    }

    #[test]
    fn an_unparsable_now_is_kept_for_the_warning() {
        let ctx =
            RuntimeContext::from_env(&MapEnv::new("/").with("HORCH_NOW", "yesterday")).unwrap();
        assert_eq!(ctx.settings.now, None);
        assert_eq!(ctx.settings.now_unparsable.as_deref(), Some("yesterday"));
    }

    #[test]
    fn tiling_is_on_unless_the_operator_switches_it_off() {
        for (value, mode) in [
            (Some(""), TilingMode::Automatic),
            (Some("1"), TilingMode::Automatic),
            (Some("yes"), TilingMode::Automatic),
            (Some("0"), TilingMode::Disabled),
            (Some("false"), TilingMode::Disabled),
            (Some("no"), TilingMode::Disabled),
            (Some("off"), TilingMode::Disabled),
            (None, TilingMode::Automatic),
        ] {
            assert_eq!(tiling(value), mode, "HORCH_TILE={value:?}");
        }
    }

    #[test]
    fn prepending_own_dir_changes_only_the_context() {
        let mut ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("PATH", "/usr/bin"),
        )
        .unwrap();
        let next = ctx.prepend_own_dir_to_path().unwrap();
        assert!(process::on_path_in(
            &next,
            std::path::Path::new("/opt/horch/bin")
        ));
        assert_eq!(ctx.inherited.path, Some(next));
        assert_eq!(ctx.prepend_own_dir_to_path(), None, "already first");
    }
}
