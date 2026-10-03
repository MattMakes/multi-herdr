//! Locating the programs horch runs: the agent CLIs a worker pane launches,
//! herdr, and the helpers the telemetry and quota readers call.
//!
//! The resolvers moved here from `agent.rs`. They are functions of the
//! explicit `HORCH_*_BIN` overrides and a PATH value, so nothing here reads the
//! process environment.

use std::ffi::OsStr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::context::EnvSource;
use super::process;

/// The explicit `HORCH_*_BIN` values, and nothing else. A pane is a fresh
/// shell that does not inherit the spawner's environment, so these travel in
/// the worker's brief.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prime: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub herdr: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite3: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<PathBuf>,
}

impl BinOverrides {
    /// Every override variable, in a fixed order.
    pub(crate) const VARS: [&'static str; 9] = [
        "HORCH_CLAUDE_BIN",
        "HORCH_CODEX_BIN",
        "HORCH_OPENCODE_BIN",
        "HORCH_PI_BIN",
        "HORCH_PRIME_BIN",
        "HORCH_HERDR_BIN",
        "HORCH_SQLITE3_BIN",
        "HORCH_OLLAMA_BIN",
        "HORCH_GIT_BIN",
    ];

    /// The overrides an environment sets. An empty value is treated as unset,
    /// not as an empty command.
    pub fn from_env(env: &dyn EnvSource) -> BinOverrides {
        let mut out = BinOverrides::default();
        for key in Self::VARS {
            if let Some(value) = super::paths::nonempty_path(env, key) {
                *out.slot(key) = Some(value);
            }
        }
        out
    }

    fn slot(&mut self, key: &str) -> &mut Option<PathBuf> {
        match key {
            "HORCH_CLAUDE_BIN" => &mut self.claude,
            "HORCH_CODEX_BIN" => &mut self.codex,
            "HORCH_OPENCODE_BIN" => &mut self.opencode,
            "HORCH_PI_BIN" => &mut self.pi,
            "HORCH_PRIME_BIN" => &mut self.prime,
            "HORCH_HERDR_BIN" => &mut self.herdr,
            "HORCH_SQLITE3_BIN" => &mut self.sqlite3,
            "HORCH_OLLAMA_BIN" => &mut self.ollama,
            "HORCH_GIT_BIN" => &mut self.git,
            other => unreachable!("not a bin override: {other}"),
        }
    }

    /// `(variable, value)` for every override that is set, in `Self::VARS`
    /// order.
    pub fn env_pairs(&self) -> Vec<(&'static str, PathBuf)> {
        let mut copy = self.clone();
        Self::VARS
            .iter()
            .filter_map(|key| copy.slot(key).take().map(|v| (*key, v)))
            .collect()
    }

    /// Take every override `other` sets; keep the rest.
    pub fn merge(&mut self, other: &BinOverrides) {
        let mut other = other.clone();
        for key in Self::VARS {
            if let Some(value) = other.slot(key).take() {
                *self.slot(key) = Some(value);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self == &BinOverrides::default()
    }
}

/// The resolved program for each tool: the override, else the default name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessBins {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub opencode: PathBuf,
    pub pi: PathBuf,
    pub prime: PathBuf,
    pub herdr: PathBuf,
    pub sqlite3: PathBuf,
    pub ollama: PathBuf,
    pub git: PathBuf,
}

impl HarnessBins {
    /// Resolve every tool. `path` and `pathext` are only searched for `cpx`.
    pub fn resolve(o: &BinOverrides, path: Option<&OsStr>, pathext: Option<&str>) -> HarnessBins {
        HarnessBins {
            claude: claude_bin(o, path, pathext),
            codex: codex_bin(o),
            opencode: opencode_bin(o),
            pi: pi_bin(o),
            prime: prime_bin(o),
            herdr: herdr_bin(o),
            sqlite3: sqlite3_bin(o),
            ollama: ollama_bin(o),
            git: git_bin(o),
        }
    }
}

fn or_default(value: &Option<PathBuf>, name: &str) -> PathBuf {
    value.clone().unwrap_or_else(|| PathBuf::from(name))
}

/// Which Claude CLI to launch.
///
/// The bash implementation hard-coded a sibling checkout
/// (`../claude-code-proxy/bin/cpx`), which cannot exist on a fresh Windows
/// machine. Resolution order now:
///   1. `$HORCH_CLAUDE_BIN` - explicit override.
///   2. `cpx` on PATH - preserves the proxy setup where it is already installed.
///   3. `claude` - the plain Claude Code CLI.
pub fn claude_bin(o: &BinOverrides, path: Option<&OsStr>, pathext: Option<&str>) -> PathBuf {
    if let Some(explicit) = &o.claude {
        return explicit.clone();
    }
    if process::which(path, pathext, "cpx").is_some() {
        return PathBuf::from("cpx");
    }
    PathBuf::from("claude")
}

/// Which Codex CLI to launch. `$HORCH_CODEX_BIN` overrides.
pub fn codex_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.codex, "codex")
}

/// Which OpenCode CLI to launch. `$HORCH_OPENCODE_BIN` overrides.
pub(crate) fn opencode_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.opencode, "opencode")
}

/// Which pi CLI to launch. `$HORCH_PI_BIN` overrides.
pub(crate) fn pi_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.pi, "pi")
}

/// Which Prime Agent CLI to launch. `$HORCH_PRIME_BIN` overrides.
pub(crate) fn prime_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.prime, "prime-agent")
}

/// Which herdr CLI to drive. `$HORCH_HERDR_BIN` overrides, which is how the
/// end-to-end tests put a recording fake in its place.
pub fn herdr_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.herdr, "herdr")
}

/// Which `sqlite3` CLI reads OpenCode's database. `$HORCH_SQLITE3_BIN`
/// overrides. horch links no SQLite library; the CLI is the only reader.
pub fn sqlite3_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.sqlite3, "sqlite3")
}

/// Which `ollama` CLI the local-pool health check asks for its model list.
/// `$HORCH_OLLAMA_BIN` overrides.
pub(crate) fn ollama_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.ollama, "ollama")
}

/// Which `git` to run. `$HORCH_GIT_BIN` overrides.
pub fn git_bin(o: &BinOverrides) -> PathBuf {
    or_default(&o.git, "git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::context::MapEnv;

    #[test]
    fn explicit_override_wins() {
        let env = MapEnv::new("/")
            .with("HORCH_CLAUDE_BIN", "/custom/agent")
            .with("HORCH_CODEX_BIN", "/custom/codex");
        let o = BinOverrides::from_env(&env);
        assert_eq!(claude_bin(&o, None, None), PathBuf::from("/custom/agent"));
        assert_eq!(codex_bin(&o), PathBuf::from("/custom/codex"));
    }

    /// An empty override is treated as unset, not as an empty command.
    #[test]
    fn empty_override_is_ignored() {
        let o = BinOverrides::from_env(&MapEnv::new("/").with("HORCH_CODEX_BIN", ""));
        assert_eq!(codex_bin(&o), PathBuf::from("codex"));
        assert!(o.is_empty());
    }

    #[test]
    fn defaults_are_the_plain_names() {
        let b = HarnessBins::resolve(&BinOverrides::default(), None, None);
        assert_eq!(b.claude, PathBuf::from("claude"));
        assert_eq!(b.prime, PathBuf::from("prime-agent"));
        assert_eq!(b.git, PathBuf::from("git"));
    }

    #[cfg(unix)]
    #[test]
    fn cpx_on_path_beats_plain_claude() {
        let tmp = tempfile::tempdir().unwrap();
        let cpx = tmp.path().join("cpx");
        std::fs::write(&cpx, "#!/bin/sh\n").unwrap();
        process::make_executable(&cpx).unwrap();
        let path = std::env::join_paths([tmp.path()]).unwrap();
        let o = BinOverrides::default();
        assert_eq!(claude_bin(&o, Some(&path), None), PathBuf::from("cpx"));
    }

    #[test]
    fn env_pairs_and_merge_cover_every_override() {
        let mut env = MapEnv::new("/");
        for key in BinOverrides::VARS {
            env = env.with(key, &format!("/fake/{key}"));
        }
        let all = BinOverrides::from_env(&env);
        let pairs = all.env_pairs();
        assert_eq!(pairs.len(), 9);
        for (key, value) in &pairs {
            assert_eq!(value, &PathBuf::from(format!("/fake/{key}")));
        }

        let mut base = BinOverrides {
            claude: Some("/old/claude".into()),
            pi: Some("/old/pi".into()),
            ..BinOverrides::default()
        };
        base.merge(&BinOverrides {
            claude: Some("/new/claude".into()),
            ..BinOverrides::default()
        });
        assert_eq!(base.claude, Some("/new/claude".into()));
        assert_eq!(base.pi, Some("/old/pi".into()));
    }
}
