//! The readings a decision runs on: `policy.json` and `quota.json` I/O, and
//! [`obtain`], the only routing path that probes (QUO-07).

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::clock;
use crate::routing::policy::{self, Policy};
use crate::routing::quota::{probe_due, quota_path, QuotaFile, QuotaView};
use crate::routing::quota_probe::{probe_all, ProbeBins};

/// The process settings a snapshot depends on, taken from a
/// `RuntimeContext` ([`QuotaEnv::from_context`]).
#[derive(Debug, Clone)]
pub struct QuotaEnv {
    /// `HORCH_QUOTA_FILE`: pool readings from a file, never probed (tests,
    /// drills).
    pub quota_file: Option<PathBuf>,
    /// `HORCH_PROBE_TIMEOUT_MS`; `None` is the 10 s default.
    pub probe_timeout: Option<StdDuration>,
    /// Where a probe makes its scratch dir.
    pub temp_root: PathBuf,
    /// The programs a probe runs.
    pub bins: ProbeBins,
}

impl QuotaEnv {
    pub fn from_context(ctx: &crate::runtime::RuntimeContext) -> QuotaEnv {
        QuotaEnv {
            quota_file: ctx.settings.quota_file.clone(),
            probe_timeout: ctx.settings.probe_timeout,
            temp_root: ctx.paths.temp_root.clone(),
            bins: ProbeBins::from_context(ctx),
        }
    }
}

impl Policy {
    /// `<state root>/policy.json` over the defaults, then `balance_override`
    /// (the caller reads `HORCH_BALANCE`).
    pub fn load(state_root: &Path, balance_override: Option<&str>) -> Result<Policy> {
        let mut policy = match std::fs::read_to_string(policy::path(state_root)) {
            Ok(text) => Self::parse(&text)
                .with_context(|| format!("in {}", policy::path(state_root).display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Policy::default(),
            Err(e) => {
                return Err(e).context(format!("reading {}", policy::path(state_root).display()))
            }
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
}

impl QuotaFile {
    pub fn read(path: &Path) -> Result<QuotaFile> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// The readings in force: `quota_file` (`HORCH_QUOTA_FILE`), else the
    /// state root's `quota.json`, else none.
    pub fn load(state_root: &Path, quota_file: Option<&Path>) -> Result<QuotaFile> {
        if let Some(p) = quota_file {
            return QuotaFile::read(p);
        }
        let p = quota_path(state_root);
        if p.is_file() {
            QuotaFile::read(&p)
        } else {
            Ok(QuotaFile::default())
        }
    }

    /// Write with each pool's display state refreshed.
    pub fn write(&mut self, state_root: &Path, now: DateTime<Utc>, policy: &Policy) -> Result<()> {
        self.written_at = Some(clock::stamp(now));
        let view = QuotaView::new(self.clone(), now, policy.clone(), false);
        for (name, reading) in self.pools.iter_mut() {
            let a = view.assess_pool(name, None, None);
            reading.state = a.state.as_str().to_string();
            reading.reason = Some(a.reason);
        }
        let path = quota_path(state_root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        crate::telemetry::store::replace_private(&path, &json)
            .with_context(|| format!("writing {}", path.display()))
    }
}

/// The readings a CLI decides on (the spawn gate, `horch route`, `horch
/// quota`). `env.quota_file` wins and is never probed. Otherwise, when
/// `allow_probe` is set, no live collector holds the lock, and the last probe
/// is older than `probe_on_demand_age_min`, probe now and write the result
/// (QUO-07).
pub fn obtain(
    state_root: &Path,
    now: DateTime<Utc>,
    policy: &Policy,
    allow_probe: bool,
    env: &QuotaEnv,
) -> Result<QuotaView> {
    if let Some(p) = &env.quota_file {
        return Ok(QuotaView::new(
            QuotaFile::load(state_root, Some(p))?,
            now,
            policy.clone(),
            true,
        ));
    }
    let mut file = QuotaFile::load(state_root, None).unwrap_or_default();
    if allow_probe
        && !crate::telemetry::lock::collector_live(state_root)
        && probe_due(&file, now, policy.probe_on_demand_age_min)
    {
        probe_all(&mut file, &env.bins, now, env.probe_timeout, &env.temp_root);
        file.write(state_root, now, policy)?;
    }
    Ok(QuotaView::new(file, now, policy.clone(), false))
}
