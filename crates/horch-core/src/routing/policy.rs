//! Telemetry and balancing settings: `<state root>/policy.json` (design
//! section 10).
//!
//! Every key has a compiled default, so the file is optional and may name
//! only what it changes. An unknown key is an error that names the key: a
//! typo must not silently keep a default.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// How the spawn gate acts on pool states (OD1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BalanceMode {
    /// Substitute or refuse as section 13.4 says.
    Auto,
    /// Print a NOTE, never substitute or refuse.
    Advise,
    /// Say nothing.
    Off,
}

impl std::str::FromStr for BalanceMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(BalanceMode::Auto),
            "advise" => Ok(BalanceMode::Advise),
            "off" => Ok(BalanceMode::Off),
            other => Err(format!(
                "unknown balance mode '{other}' (auto, advise, off)"
            )),
        }
    }
}

impl BalanceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BalanceMode::Auto => "auto",
            BalanceMode::Advise => "advise",
            BalanceMode::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub tick_ms: u64,
    pub retention_days: i64,
    pub probe_interval_min: i64,
    pub probe_on_demand_age_min: i64,
    pub stale_after_min: i64,
    pub tight_used: f64,
    pub exhausted_used: f64,
    pub pace_factor: f64,
    pub pace_min_used: f64,
    pub headroom_ratio: f64,
    pub opencode_cooldown_min: i64,
    pub balance_mode: BalanceMode,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            tick_ms: 2000,
            retention_days: 35,
            probe_interval_min: 15,
            probe_on_demand_age_min: 10,
            stale_after_min: 30,
            tight_used: 0.85,
            exhausted_used: 0.98,
            pace_factor: 1.25,
            pace_min_used: 0.50,
            headroom_ratio: 2.0,
            opencode_cooldown_min: 60,
            balance_mode: BalanceMode::Auto,
        }
    }
}

pub fn path(state_root: &Path) -> PathBuf {
    state_root.join("policy.json")
}

impl Policy {
    /// `<state root>/policy.json` over the defaults, then `balance_override`
    /// (the caller reads `HORCH_BALANCE`).
    pub fn load(state_root: &Path, balance_override: Option<&str>) -> Result<Policy> {
        let mut policy = match std::fs::read_to_string(path(state_root)) {
            Ok(text) => {
                Self::parse(&text).with_context(|| format!("in {}", path(state_root).display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Policy::default(),
            Err(e) => return Err(e).context(format!("reading {}", path(state_root).display())),
        };
        if let Some(mode) = balance_override {
            if !mode.trim().is_empty() {
                policy.balance_mode = mode
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!("HORCH_BALANCE: {e}"))?;
            }
        }
        Ok(policy)
    }

    pub fn parse(text: &str) -> Result<Policy> {
        let policy: Policy = serde_json::from_str(text).map_err(|e| {
            // serde names the unknown field; keep its words.
            anyhow::anyhow!("policy.json: {e}")
        })?;
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<()> {
        for (name, v) in [
            ("tight_used", self.tight_used),
            ("exhausted_used", self.exhausted_used),
            ("pace_min_used", self.pace_min_used),
        ] {
            if !(0.0..=1.0).contains(&v) {
                bail!("policy.json: {name} must be between 0 and 1, got {v}");
            }
        }
        if self.tight_used > self.exhausted_used {
            bail!("policy.json: tight_used must not exceed exhausted_used");
        }
        if self.tick_ms < 100 {
            bail!("policy.json: tick_ms must be at least 100");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quo_05_every_key_has_a_default_and_overrides_apply() {
        let p = Policy::parse("{}").unwrap();
        assert_eq!(p, Policy::default());
        let p = Policy::parse(r#"{"tight_used": 0.7, "balance_mode": "advise"}"#).unwrap();
        assert_eq!(p.tight_used, 0.7);
        assert_eq!(p.balance_mode, BalanceMode::Advise);
        assert_eq!(p.exhausted_used, 0.98);
    }

    #[test]
    fn quo_05_an_unknown_key_is_named() {
        let err = Policy::parse(r#"{"tight_usd": 0.7}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("tight_usd"), "{err}");
        assert!(Policy::parse(r#"{"tight_used": 1.5}"#).is_err());
    }
}
