//! Selecting which pane host (`herdr` or `hc`) this process runs inside.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Herdr,
    Hc,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Herdr => "herdr",
            Backend::Hc => "hc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    Unknown { value: String },
    Ambiguous,
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unknown { value } => {
                write!(f, "unknown backend '{value}' (expected herdr or hc)")
            }
            BackendError::Ambiguous => write!(
                f,
                "both HERDR_PANE_ID and HC_TERMINAL_ID are set; set HORCH_BACKEND=herdr or HORCH_BACKEND=hc"
            ),
        }
    }
}

impl std::error::Error for BackendError {}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Backend {
    type Err = BackendError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "herdr" => Ok(Backend::Herdr),
            "hc" => Ok(Backend::Hc),
            other => Err(BackendError::Unknown {
                value: other.to_string(),
            }),
        }
    }
}

pub fn resolve(
    explicit: Option<&str>,
    herdr_pane: bool,
    hc_terminal: bool,
) -> Result<Backend, BackendError> {
    if let Some(value) = explicit {
        return value.parse();
    }
    match (herdr_pane, hc_terminal) {
        (true, false) => Ok(Backend::Herdr),
        (false, true) => Ok(Backend::Hc),
        (true, true) => Err(BackendError::Ambiguous),
        (false, false) => Ok(Backend::Herdr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{MapEnv, RuntimeContext};

    #[test]
    fn hcb_01_explicit_wins() {
        assert_eq!(resolve(Some("hc"), true, false), Ok(Backend::Hc));
        assert_eq!(resolve(Some("herdr"), false, true), Ok(Backend::Herdr));
        assert_eq!(resolve(Some("hc"), true, true), Ok(Backend::Hc));
        assert_eq!(resolve(Some("herdr"), true, true), Ok(Backend::Herdr));

        let err = resolve(Some("tmux"), false, false).unwrap_err().to_string();
        assert!(err.contains("herdr") && err.contains("hc"), "{err}");

        let cli_over_env = RuntimeContext::from_env_with_backend(
            &MapEnv::new("/")
                .with("HORCH_BACKEND", "herdr")
                .with("HERDR_PANE_ID", "p_1")
                .with("HC_TERMINAL_ID", "t1"),
            Some("hc"),
        )
        .unwrap();
        assert_eq!(cli_over_env.herdr.backend, Backend::Hc);
        assert_eq!(cli_over_env.herdr.pane.as_ref().unwrap().as_str(), "t1");
    }

    #[test]
    fn hcb_01_one_identity_picks_its_backend() {
        assert_eq!(resolve(None, true, false), Ok(Backend::Herdr));
        assert_eq!(resolve(None, false, true), Ok(Backend::Hc));
    }

    #[test]
    fn hcb_01_both_identities_are_refused() {
        let err = resolve(None, true, true).unwrap_err();
        assert_eq!(err, BackendError::Ambiguous);
        assert_eq!(
            err.to_string(),
            "both HERDR_PANE_ID and HC_TERMINAL_ID are set; set HORCH_BACKEND=herdr or HORCH_BACKEND=hc"
        );

        let env = MapEnv::new("/")
            .with("HERDR_PANE_ID", "p_1")
            .with("HC_TERMINAL_ID", "t1");
        let ctx_err = RuntimeContext::from_env(&env).unwrap_err().to_string();
        assert!(
            ctx_err.contains(
                "both HERDR_PANE_ID and HC_TERMINAL_ID are set; set HORCH_BACKEND=herdr or HORCH_BACKEND=hc"
            ),
            "{ctx_err}"
        );
    }

    #[test]
    fn hcb_01_neither_identity_is_herdr() {
        assert_eq!(resolve(None, false, false), Ok(Backend::Herdr));
        let ctx = RuntimeContext::from_env(&MapEnv::new("/")).unwrap();
        assert_eq!(ctx.herdr.backend, Backend::Herdr);
        assert_eq!(ctx.herdr.pane, None);
    }

    #[test]
    fn hcb_01_pane_comes_from_the_backends_variable() {
        let hc_ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with("HC_TERMINAL_ID", "t7")
                .with("HC_TERMINAL_SOCKET", "/tmp/hc.sock")
                .with("HERDR_APP_HOME", "/tmp/app"),
        )
        .unwrap();
        assert_eq!(hc_ctx.herdr.backend, Backend::Hc);
        assert_eq!(hc_ctx.herdr.pane.as_ref().unwrap().as_str(), "t7");
        assert_eq!(
            hc_ctx.inherited.hc_terminal_socket.as_deref(),
            Some(std::path::Path::new("/tmp/hc.sock"))
        );
        assert_eq!(
            hc_ctx.inherited.herdr_app_home.as_deref(),
            Some(std::path::Path::new("/tmp/app"))
        );

        let forced_hc = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with("HORCH_BACKEND", "hc")
                .with("HERDR_PANE_ID", "p_2")
                .with("HC_TERMINAL_ID", "t9"),
        )
        .unwrap();
        assert_eq!(forced_hc.herdr.backend, Backend::Hc);
        assert_eq!(forced_hc.herdr.pane.as_ref().unwrap().as_str(), "t9");

        let forced_herdr = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with("HORCH_BACKEND", "herdr")
                .with("HERDR_PANE_ID", "p_2")
                .with("HC_TERMINAL_ID", "t9"),
        )
        .unwrap();
        assert_eq!(forced_herdr.herdr.backend, Backend::Herdr);
        assert_eq!(forced_herdr.herdr.pane.as_ref().unwrap().as_str(), "p_2");
    }
}
