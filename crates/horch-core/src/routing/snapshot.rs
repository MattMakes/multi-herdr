use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::clock;
use crate::routing::policy::{self, Policy};
use crate::routing::quota::{probe_due, quota_path, QuotaFile, QuotaView};
use crate::routing::quota_probe::{probe_all, probe_budget, ProbeBins};

#[derive(Debug, Clone)]
pub struct QuotaEnv {
    pub quota_file: Option<PathBuf>,
    pub probe_timeout: Option<StdDuration>,
    pub temp_root: PathBuf,
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

    pub fn write(&mut self, state_root: &Path, now: DateTime<Utc>, policy: &Policy) -> Result<()> {
        self.written_at = Some(clock::stamp(now));
        let view = QuotaView::new(self.clone(), now, policy.clone(), false);
        for (name, reading) in self.pools.iter_mut() {
            let a = view.assess_shown(name);
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

const REFRESH_POLL: StdDuration = StdDuration::from_millis(50);

pub fn refresh(
    state_root: &Path,
    now: DateTime<Utc>,
    policy: &Policy,
    env: &QuotaEnv,
) -> Result<QuotaView> {
    if env.quota_file.is_some() {
        return obtain(state_root, now, policy, false, env);
    }
    if crate::telemetry::lock::collector_live(state_root) {
        let tick = StdDuration::from_millis(policy.tick_ms);
        let wait = (tick + probe_budget(&env.bins, env.probe_timeout)) * 2;
        return refresh_through_collector(state_root, now, policy, wait);
    }
    let mut file = QuotaFile::load(state_root, None).unwrap_or_default();
    probe_all(&mut file, &env.bins, now, env.probe_timeout, &env.temp_root);
    file.write(state_root, now, policy)?;
    Ok(QuotaView::new(file, now, policy.clone(), false))
}

pub(crate) fn refresh_through_collector(
    state_root: &Path,
    now: DateTime<Utc>,
    policy: &Policy,
    wait: StdDuration,
) -> Result<QuotaView> {
    let before = QuotaFile::load(state_root, None)
        .unwrap_or_default()
        .latest_probe()
        .map(str::to_owned);
    let request = crate::telemetry::refresh::request(state_root)?;
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if !request.exists() {
            let file = QuotaFile::load(state_root, None)?;
            if file.latest_probe().is_some() && file.latest_probe() >= before.as_deref() {
                return Ok(QuotaView::new(file, now, policy.clone(), false));
            }
        }
        std::thread::sleep(REFRESH_POLL);
    }
    crate::telemetry::refresh::clear(&[request]);
    anyhow::bail!(
        "quota refresh failed: the collector published no new reading within {} s",
        wait.as_secs()
    )
}
