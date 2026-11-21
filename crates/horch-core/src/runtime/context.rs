use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::backend::Backend;
use super::bins::{BinOverrides, HarnessBins};
use super::fault::Faults;
use super::paths::{nonempty_path, Paths};
use super::process;
use crate::execution::TilingMode;
use crate::ids::{PaneId, WorkspaceId};

pub trait EnvSource {
    fn var(&self, key: &str) -> Option<String>;
    fn var_os(&self, key: &str) -> Option<OsString>;
    fn current_dir(&self) -> Option<PathBuf>;
    fn current_exe(&self) -> Option<PathBuf>;
    fn temp_dir(&self) -> PathBuf;
}

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

#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    pub vars: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub exe: Option<PathBuf>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrEnv {
    pub backend: Backend,
    pub workspace: Option<WorkspaceId>,
    pub pane: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bins {
    pub roster_override: Option<PathBuf>,
    pub current_exe: Option<PathBuf>,
    pub installed_horch: Option<PathBuf>,
    pub horch_exe: PathBuf,
    pub harness: HarnessBins,
    pub overrides: BinOverrides,
}

impl Bins {
    pub fn exe(&self) -> Result<PathBuf> {
        self.current_exe
            .clone()
            .context("locating the horch binary")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub now: Option<DateTime<Utc>>,
    pub test_mail_clock_file: Option<PathBuf>,
    pub now_unparsable: Option<String>,
    pub tiling: TilingMode,
    pub balance_override: Option<String>,
    pub quota_file: Option<PathBuf>,
    pub machine_file: Option<PathBuf>,
    pub probe_timeout: Option<Duration>,
    pub local_probe_timeout: Option<Duration>,
    pub tell_grace: Option<Duration>,
    pub spawn_wait: Option<Duration>,
    pub no_update: bool,
    pub no_self_check: bool,
    pub no_watchdog: bool,
    pub update_timeout: Duration,
    pub update_cooldown_days: u32,
    pub faults: Faults,
}

pub(crate) const DEFAULT_UPDATE_TIMEOUT: Duration = Duration::from_millis(180_000);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inherited {
    pub home_var: Option<OsString>,
    pub path: Option<OsString>,
    pub claude_base_url_host: Option<String>,
    pub pathext: Option<String>,
    pub blender_path: Option<OsString>,
    pub godot_path: Option<OsString>,
    pub godot_mono_path: Option<OsString>,
    pub opencode_config_content: Option<String>,
    pub codex_home: Option<PathBuf>,
    pub claude_config_dir: Option<PathBuf>,
    pub claude_code_effort_level: Option<String>,
    pub pi_session_dir: Option<PathBuf>,
    pub opencode_db: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub hostname: Option<String>,
    pub herdr_session: Option<String>,
    pub herdr_socket_path: Option<PathBuf>,
    pub hc_terminal_socket: Option<PathBuf>,
    pub herdr_app_home: Option<PathBuf>,
    pub opencode_disable_external_skills: Option<String>,
    pub rlm_max_depth: Option<String>,
    pub claude_code_auto_compact_window: Option<String>,
    pub prime_agent_dir: Option<PathBuf>,
}

pub(crate) const DEFAULT_ENV_YIELD: [&str; 2] =
    ["OPENCODE_DISABLE_EXTERNAL_SKILLS", "RLM_MAX_DEPTH"];

impl Inherited {
    pub const DEFAULT_ENV_YIELD: [&'static str; 2] = DEFAULT_ENV_YIELD;

    pub fn operator_env(&self, key: &str) -> Option<&str> {
        match key {
            "OPENCODE_DISABLE_EXTERNAL_SKILLS" => self.opencode_disable_external_skills.as_deref(),
            "RLM_MAX_DEPTH" => self.rlm_max_depth.as_deref(),
            _ => None,
        }
    }

    pub fn from_env(env: &dyn EnvSource) -> Inherited {
        let nonempty = |key: &str| env.var(key).filter(|s| !s.is_empty());
        Inherited {
            home_var: env.var_os(horch_platform::path::HOME_VAR),
            path: env.var_os("PATH"),
            claude_base_url_host: nonempty("ANTHROPIC_BASE_URL").map(|url| base_url_host(&url)),
            pathext: env.var("PATHEXT"),
            blender_path: env.var_os("BLENDER_PATH").filter(|p| !p.is_empty()),
            godot_path: env.var_os("GODOT_PATH").filter(|p| !p.is_empty()),
            godot_mono_path: env.var_os("GODOT_MONO_PATH").filter(|p| !p.is_empty()),
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
            herdr_socket_path: nonempty_path(env, "HERDR_SOCKET_PATH"),
            hc_terminal_socket: nonempty_path(env, "HC_TERMINAL_SOCKET"),
            herdr_app_home: nonempty_path(env, "HERDR_APP_HOME"),
            opencode_disable_external_skills: env.var("OPENCODE_DISABLE_EXTERNAL_SKILLS"),
            rlm_max_depth: env.var("RLM_MAX_DEPTH"),
            claude_code_auto_compact_window: env.var("CLAUDE_CODE_AUTO_COMPACT_WINDOW"),
            prime_agent_dir: nonempty_path(env, "PRIME_AGENT_CODING_AGENT_DIR"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerEnv {
    pub role: Option<String>,
    pub teammate: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub record_id: Option<String>,
    pub session_id: Option<String>,
    pub resume: Option<String>,
    pub task: Option<String>,
    pub worker_id: Option<String>,
}

impl WorkerEnv {
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
            worker_id: nonempty("HORCH_WORKER_ID"),
        };
        (w != WorkerEnv::default()).then_some(w)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    pub paths: Paths,
    pub herdr: HerdrEnv,
    pub bins: Bins,
    pub settings: Settings,
    pub inherited: Inherited,
    pub worker: Option<WorkerEnv>,
    pub report_socket: Option<PathBuf>,
}

impl RuntimeContext {
    pub fn from_env(env: &dyn EnvSource) -> Result<RuntimeContext> {
        Self::from_env_with_backend(env, None)
    }

    pub fn from_env_with_backend(
        env: &dyn EnvSource,
        explicit_backend: Option<&str>,
    ) -> Result<RuntimeContext> {
        let nonempty = |key: &str| env.var(key).filter(|s| !s.is_empty());
        let inherited = Inherited::from_env(env);

        let herdr_pane = nonempty("HERDR_PANE_ID");
        let hc_terminal = nonempty("HC_TERMINAL_ID");
        let env_backend = nonempty("HORCH_BACKEND");
        let explicit = explicit_backend.or(env_backend.as_deref());
        let backend =
            super::backend::resolve(explicit, herdr_pane.is_some(), hc_terminal.is_some())?;
        let (pane_raw, pane_var) = match backend {
            Backend::Herdr => (herdr_pane, "HERDR_PANE_ID"),
            Backend::Hc => (hc_terminal, "HC_TERMINAL_ID"),
        };

        let herdr = HerdrEnv {
            backend,
            workspace: nonempty("HORCH_WORKSPACE_ID")
                .map(WorkspaceId::new)
                .transpose()
                .context("HORCH_WORKSPACE_ID")?,
            pane: pane_raw.map(PaneId::new).transpose().context(pane_var)?,
        };

        let overrides = BinOverrides::from_env(env);
        let current_exe = env.current_exe();
        let bins = Bins {
            roster_override: nonempty_path(env, "HORCH_TEAMMATES_DIR"),
            installed_horch: process::which(
                inherited.path.as_deref(),
                inherited.pathext.as_deref(),
                "horch",
            ),
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
            test_mail_clock_file: nonempty_path(env, "HORCH_TEST_MAIL_CLOCK_FILE"),
            tiling: tiling(env.var("HORCH_TILE").as_deref()),
            balance_override: env.var("HORCH_BALANCE"),
            quota_file: nonempty_path(env, "HORCH_QUOTA_FILE"),
            machine_file: nonempty_path(env, "HORCH_MACHINE_FILE"),
            probe_timeout: nonempty("HORCH_PROBE_TIMEOUT_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            local_probe_timeout: nonempty("HORCH_LOCAL_PROBE_TIMEOUT_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            tell_grace: nonempty("HORCH_TELL_GRACE_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            spawn_wait: nonempty("HORCH_SPAWN_WAIT_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map(Duration::from_millis),
            no_update: env.var("HORCH_NO_UPDATE").as_deref() == Some("1"),
            no_self_check: env.var("HORCH_NO_SELF_CHECK").as_deref() == Some("1"),
            no_watchdog: env.var("HORCH_NO_WATCHDOG").as_deref() == Some("1"),
            update_timeout: nonempty("HORCH_UPDATE_TIMEOUT_MS")
                .and_then(|ms| ms.parse::<u64>().ok())
                .map_or(DEFAULT_UPDATE_TIMEOUT, Duration::from_millis),
            update_cooldown_days: nonempty("HORCH_UPDATE_COOLDOWN_DAYS")
                .and_then(|days| days.trim().parse::<u32>().ok())
                .unwrap_or(crate::harness::cooldown::DEFAULT_DAYS),
            faults: Faults::parse(env.var("HORCH_FAULT").as_deref()),
        };

        Ok(RuntimeContext {
            paths: Paths::from_env(env),
            herdr,
            bins,
            settings,
            inherited,
            worker: WorkerEnv::from_env(env),
            report_socket: nonempty_path(env, crate::messaging::report::ENV),
        })
    }

    pub(crate) fn apply_overrides(&mut self, overrides: &BinOverrides) {
        self.bins.overrides.merge(overrides);
        self.refresh_bins();
    }

    pub(crate) fn refresh_bins(&mut self) {
        self.bins.harness = HarnessBins::resolve(
            &self.bins.overrides,
            self.inherited.path.as_deref(),
            self.inherited.pathext.as_deref(),
        );
    }

    pub fn prepend_own_dir_to_path(&mut self) -> Option<OsString> {
        let exe = self.bins.current_exe.as_deref()?;
        let next = process::path_with_own_dir(exe, self.inherited.path.as_deref())?;
        self.inherited.path = Some(next.clone());
        self.refresh_bins();
        Some(next)
    }
}

pub(crate) fn tiling(raw: Option<&str>) -> TilingMode {
    match raw.unwrap_or_default() {
        "0" | "false" | "no" | "off" => TilingMode::Disabled,
        _ => TilingMode::Automatic,
    }
}

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

fn base_url_host(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
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
            .with("HORCH_LOCAL_PROBE_TIMEOUT_MS", "120000")
            .with("HORCH_TELL_GRACE_MS", "0")
            .with("HORCH_SPAWN_WAIT_MS", "1500")
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
            .with("HERDR_SOCKET_PATH", "/herdr.sock")
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
        assert_eq!(ctx.bins.harness.hc, PathBuf::from("/fake/HORCH_HC_BIN"));
        assert_eq!(
            ctx.bins.harness.herdr_app,
            PathBuf::from("/fake/HORCH_HERDR_APP_BIN")
        );
        assert_eq!(ctx.bins.overrides.env_pairs().len(), 12);

        let s = &ctx.settings;
        assert_eq!(s.now, crate::clock::parse("2026-09-28T18:00:00Z"));
        assert_eq!(s.now_unparsable, None);
        assert_eq!(s.tiling, TilingMode::Disabled);
        assert_eq!(s.balance_override.as_deref(), Some("advise"));
        assert_eq!(s.quota_file, Some(PathBuf::from("/q.json")));
        assert_eq!(s.machine_file, Some(PathBuf::from("/m.json")));
        assert_eq!(s.probe_timeout, Some(Duration::from_millis(250)));
        assert_eq!(s.local_probe_timeout, Some(Duration::from_secs(120)));
        assert_eq!(s.tell_grace, Some(Duration::ZERO));
        assert_eq!(s.spawn_wait, Some(Duration::from_millis(1500)));
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
        assert_eq!(i.herdr_socket_path, Some(PathBuf::from("/herdr.sock")));

        let w = ctx.worker.as_ref().unwrap();
        assert_eq!(w.role.as_deref(), Some("sonnet-1"));
        assert_eq!(w.record_id.as_deref(), Some("r1"));
        assert_eq!(w.task.as_deref(), Some("t"));

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
        assert_eq!(ctx.settings.local_probe_timeout, None);
        assert_eq!(ctx.settings.tell_grace, None);
        assert_eq!(ctx.settings.spawn_wait, None);
        assert!(ctx.settings.faults.is_empty());
        assert_eq!(ctx.inherited, Inherited::default());
        assert_eq!(ctx.worker, None);
    }

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
    fn arc_34_update_settings_read_both_variables_and_the_default() {
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with("HORCH_NO_UPDATE", "1")
                .with("HORCH_UPDATE_TIMEOUT_MS", "5000"),
        )
        .unwrap();
        assert!(ctx.settings.no_update);
        assert_eq!(ctx.settings.update_timeout, Duration::from_millis(5000));

        let ctx = RuntimeContext::from_env(&MapEnv::new("/")).unwrap();
        assert!(!ctx.settings.no_update);
        assert_eq!(ctx.settings.update_timeout, Duration::from_millis(180_000));
        assert_eq!(ctx.settings.update_timeout, DEFAULT_UPDATE_TIMEOUT);

        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with("HORCH_NO_UPDATE", "0")
                .with("HORCH_UPDATE_TIMEOUT_MS", "soon"),
        )
        .unwrap();
        assert!(!ctx.settings.no_update);
        assert_eq!(ctx.settings.update_timeout, DEFAULT_UPDATE_TIMEOUT);
    }

    #[test]
    fn arc_36_the_cooldown_reads_its_variable_and_the_default() {
        let days = |value: Option<&str>| {
            let mut env = MapEnv::new("/");
            if let Some(v) = value {
                env = env.with("HORCH_UPDATE_COOLDOWN_DAYS", v);
            }
            RuntimeContext::from_env(&env)
                .unwrap()
                .settings
                .update_cooldown_days
        };
        assert_eq!(days(None), 7);
        assert_eq!(days(Some("14")), 14);
        assert_eq!(days(Some("0")), 0);
        assert_eq!(days(Some("")), 7);
        assert_eq!(days(Some("-1")), 7);
        assert_eq!(days(Some("a week")), 7);
    }

    #[test]
    fn self_check_setting_reads_only_1() {
        let read = |value: Option<&str>| {
            let env = match value {
                Some(v) => MapEnv::new("/").with("HORCH_NO_SELF_CHECK", v),
                None => MapEnv::new("/"),
            };
            RuntimeContext::from_env(&env)
                .unwrap()
                .settings
                .no_self_check
        };
        assert!(read(Some("1")));
        assert!(!read(Some("0")));
        assert!(!read(None));
    }

    #[test]
    fn no_watchdog_setting_reads_only_1() {
        let read = |value: Option<&str>| {
            let env = match value {
                Some(v) => MapEnv::new("/").with("HORCH_NO_WATCHDOG", v),
                None => MapEnv::new("/"),
            };
            RuntimeContext::from_env(&env).unwrap().settings.no_watchdog
        };
        assert!(read(Some("1")));
        assert!(!read(Some("0")));
        assert!(!read(None));
    }

    #[test]
    fn an_unparsable_now_is_kept_for_the_warning() {
        let ctx =
            RuntimeContext::from_env(&MapEnv::new("/").with("HORCH_NOW", "yesterday")).unwrap();
        assert_eq!(ctx.settings.now, None);
        assert_eq!(ctx.settings.now_unparsable.as_deref(), Some("yesterday"));
    }

    #[test]
    fn the_local_probe_timeout_parses_milliseconds_and_ignores_the_rest() {
        for (value, want) in [
            ("7000", Some(Duration::from_secs(7))),
            ("0", Some(Duration::ZERO)),
            ("", None),
            ("5s", None),
            ("-1", None),
        ] {
            let env = MapEnv::new("/").with("HORCH_LOCAL_PROBE_TIMEOUT_MS", value);
            let ctx = RuntimeContext::from_env(&env).unwrap();
            assert_eq!(ctx.settings.local_probe_timeout, want, "{value:?}");
        }
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
