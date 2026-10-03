//! The dataset config: `.multi-herdr/dataset.yaml` with CLI flags over it.
//!
//! [`load`] reads the file when it exists, applies [`RunFlags`] over it,
//! fills the defaults and validates the result. Budgets are whole
//! micro-dollars; `--budget-usd` is parsed digit by digit, never through a
//! float, so `12.345678` is exactly 12 345 678 µ$.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::evaluation::winner::WinnerPolicy;
use crate::ids::TeammateName;

/// The config file, relative to the project root.
pub(crate) const CONFIG_FILE: &str = ".multi-herdr/dataset.yaml";

/// Candidates per round when neither the file nor `--candidates` says.
pub(crate) const DEFAULT_CANDIDATES: u32 = 3;
/// The soft limit, in percent of the hard ceiling, when only the ceiling is set.
pub(crate) const DEFAULT_SOFT_PERCENT: i64 = 80;
/// The judge reserve, in percent of the hard ceiling, when the file does not set it.
pub(crate) const DEFAULT_JUDGE_RESERVE_PERCENT: i64 = 10;
pub(crate) const DEFAULT_JUDGE_TIMEOUT_S: u64 = 900;
pub(crate) const DEFAULT_CANDIDATE_DEADLINE_S: u64 = 3600;
/// 10 GiB of disk left free after every worktree and build is counted.
pub(crate) const DEFAULT_DISK_HEADROOM_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub(crate) const DEFAULT_LOG_CAP_BYTES: u64 = 262_144;
pub(crate) const DEFAULT_OUTPUT_CAP_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetConfig {
    pub candidates: u32,
    pub strategy: Strategy,
    pub budget: BudgetConfig,
    pub judge: JudgeConfig,
    #[serde(default)]
    pub baseline: Option<TeammateName>,
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    #[serde(default)]
    pub caps: Caps,
    #[serde(default)]
    pub exclude: Vec<TeammateName>,
    #[serde(default)]
    pub promote_to: Option<String>,
    #[serde(default)]
    pub worktree_root: Option<PathBuf>,
    #[serde(default)]
    pub allow_dirty: bool,
    #[serde(default)]
    pub prune_branches: bool,
    /// SEC-03: transcripts are kept only on explicit opt-in.
    #[serde(default)]
    pub retain_transcripts: bool,
}

/// How candidates are picked from the roster.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// One candidate per distinct harness and model, as far as the roster allows.
    #[default]
    Diverse,
}

impl FromStr for Strategy {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "diverse" => Ok(Strategy::Diverse),
            other => bail!("unknown strategy {other:?} (expected \"diverse\")"),
        }
    }
}

/// Budgets in whole micro-dollars. A hard ceiling of 0 means none is
/// configured; preflight (PRE-09) refuses such a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetConfig {
    pub soft_usd_micro: i64,
    pub hard_usd_micro: i64,
    pub judge_reserve_usd_micro: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeMode {
    /// The judge runs as soon as every candidate is frozen.
    #[default]
    Auto,
}

impl FromStr for JudgeMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(JudgeMode::Auto),
            // SPEC-TODO(Spec B §3): the judge modes besides `auto`.
            other => bail!("unknown judge mode {other:?} (expected \"auto\")"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JudgeConfig {
    pub mode: JudgeMode,
    pub model: String,
    pub effort: String,
    pub timeout_s: u64,
    pub policy: WinnerPolicy,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            mode: JudgeMode::Auto,
            model: "opus".to_string(),
            effort: "high".to_string(),
            timeout_s: DEFAULT_JUDGE_TIMEOUT_S,
            policy: WinnerPolicy::default(),
        }
    }
}

/// A validation command every candidate runs, such as `cargo test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateConfig {
    pub name: String,
    pub command: String,
    pub timeout_s: u64,
    #[serde(default = "yes")]
    pub required: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Caps {
    pub candidate_deadline_s: u64,
    pub max_parallel: Option<u32>,
    pub disk_headroom_bytes: u64,
    pub log_cap_bytes: u64,
    pub output_cap_bytes: u64,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            candidate_deadline_s: DEFAULT_CANDIDATE_DEADLINE_S,
            max_parallel: None,
            disk_headroom_bytes: DEFAULT_DISK_HEADROOM_BYTES,
            log_cap_bytes: DEFAULT_LOG_CAP_BYTES,
            output_cap_bytes: DEFAULT_OUTPUT_CAP_BYTES,
        }
    }
}

/// The flags of `multi-herdr-dataset run`. `None` (or `false`) leaves the
/// file's value, or the default, in place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFlags {
    pub task: String,
    pub candidates: Option<u32>,
    pub strategy: Option<Strategy>,
    /// `--budget-usd`, as typed: a decimal with at most 6 fraction digits.
    pub budget_usd: Option<String>,
    pub judge: Option<JudgeMode>,
    pub baseline: Option<TeammateName>,
    pub promote_to: Option<String>,
    pub worktree_root: Option<PathBuf>,
    pub allow_dirty: bool,
}

/// `dataset.yaml` as written: every key optional, unknown keys rejected.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetFile {
    candidates: Option<u32>,
    strategy: Option<Strategy>,
    budget: Option<BudgetFile>,
    judge: Option<JudgeConfig>,
    baseline: Option<TeammateName>,
    gates: Option<Vec<GateConfig>>,
    caps: Option<Caps>,
    exclude: Option<Vec<TeammateName>>,
    promote_to: Option<String>,
    worktree_root: Option<PathBuf>,
    allow_dirty: Option<bool>,
    prune_branches: Option<bool>,
    retain_transcripts: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetFile {
    soft_usd_micro: Option<i64>,
    hard_usd_micro: Option<i64>,
    judge_reserve_usd_micro: Option<i64>,
}

/// Load `<project>/.multi-herdr/dataset.yaml` (when present), apply `flags`
/// over it, fill the defaults and validate.
pub fn load(project: &Path, flags: &RunFlags) -> Result<DatasetConfig> {
    let path = project.join(CONFIG_FILE);
    let file = match std::fs::read_to_string(&path) {
        Ok(text) => parse_file(&text).with_context(|| format!("parsing {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DatasetFile::default(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let config = merge(file, flags)?;
    validate(&config)?;
    Ok(config)
}

fn parse_file(text: &str) -> Result<DatasetFile> {
    // An empty file is a valid, empty config.
    if text.trim().is_empty() {
        return Ok(DatasetFile::default());
    }
    Ok(serde_yaml::from_str(text)?)
}

fn merge(file: DatasetFile, flags: &RunFlags) -> Result<DatasetConfig> {
    let budget_file = file.budget.unwrap_or_default();
    let hard = match &flags.budget_usd {
        Some(text) => parse_usd_micro(text).with_context(|| format!("--budget-usd {text}"))?,
        None => budget_file.hard_usd_micro.unwrap_or(0),
    };
    let budget = BudgetConfig {
        soft_usd_micro: budget_file
            .soft_usd_micro
            .unwrap_or(percent_of(hard, DEFAULT_SOFT_PERCENT)),
        hard_usd_micro: hard,
        judge_reserve_usd_micro: budget_file
            .judge_reserve_usd_micro
            .unwrap_or(percent_of(hard, DEFAULT_JUDGE_RESERVE_PERCENT)),
    };
    let mut judge = file.judge.unwrap_or_default();
    if let Some(mode) = flags.judge {
        judge.mode = mode;
    }
    Ok(DatasetConfig {
        candidates: flags
            .candidates
            .or(file.candidates)
            .unwrap_or(DEFAULT_CANDIDATES),
        strategy: flags.strategy.or(file.strategy).unwrap_or_default(),
        budget,
        judge,
        baseline: flags.baseline.clone().or(file.baseline),
        gates: file.gates.unwrap_or_default(),
        caps: file.caps.unwrap_or_default(),
        exclude: file.exclude.unwrap_or_default(),
        promote_to: flags.promote_to.clone().or(file.promote_to),
        worktree_root: flags.worktree_root.clone().or(file.worktree_root),
        allow_dirty: flags.allow_dirty || file.allow_dirty.unwrap_or(false),
        prune_branches: file.prune_branches.unwrap_or(false),
        retain_transcripts: file.retain_transcripts.unwrap_or(false),
    })
}

/// `percent` % of `value`, rounded down, without overflowing.
fn percent_of(value: i64, percent: i64) -> i64 {
    value / 100 * percent + value % 100 * percent / 100
}

/// Check the invariants every later phase relies on.
pub fn validate(config: &DatasetConfig) -> Result<()> {
    if config.candidates < 1 {
        bail!("candidates must be at least 1");
    }
    let b = &config.budget;
    if b.hard_usd_micro < 0 || b.soft_usd_micro < 0 || b.judge_reserve_usd_micro < 0 {
        bail!("budgets must not be negative");
    }
    // A hard ceiling of 0 is "not configured": PRE-09 refuses the run with a
    // report, which is more useful than a load error. Any other ceiling must
    // be consistent.
    if b.hard_usd_micro == 0 {
        if b.soft_usd_micro != 0 || b.judge_reserve_usd_micro != 0 {
            bail!("budget sets a soft limit or judge reserve but no hard ceiling");
        }
    } else {
        if b.soft_usd_micro == 0 {
            bail!("budget soft limit must be greater than 0");
        }
        if b.hard_usd_micro < b.soft_usd_micro {
            bail!(
                "budget hard ceiling ({}) is below the soft limit ({})",
                Usd(b.hard_usd_micro),
                Usd(b.soft_usd_micro)
            );
        }
        if b.judge_reserve_usd_micro >= b.hard_usd_micro {
            bail!(
                "judge reserve ({}) must be below the hard ceiling ({})",
                Usd(b.judge_reserve_usd_micro),
                Usd(b.hard_usd_micro)
            );
        }
    }
    for gate in &config.gates {
        if gate.command.trim().is_empty() {
            bail!("gate {:?} has an empty command", gate.name);
        }
        if gate.timeout_s == 0 {
            bail!("gate {:?} needs a timeout_s greater than 0", gate.name);
        }
    }
    if config.caps.max_parallel == Some(0) {
        bail!("caps.max_parallel must be at least 1");
    }
    Ok(())
}

/// Parse a non-negative decimal dollar amount into whole µ$, exactly:
/// `"12.345678"` is 12 345 678. More than 6 fraction digits is an error
/// unless the extra digits are zeros.
pub fn parse_usd_micro(text: &str) -> Result<i64> {
    let text = text.trim();
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && frac.is_empty() {
        bail!("not a dollar amount: {text:?}");
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        bail!("not a dollar amount: {text:?}");
    }
    let (kept, extra) = frac.split_at(frac.len().min(6));
    if extra.bytes().any(|b| b != b'0') {
        bail!("{text:?} has more than 6 decimal places (1 µ$ is the smallest unit)");
    }
    let mut micro: i64 = 0;
    let digits = whole
        .bytes()
        .chain(kept.bytes())
        .chain(std::iter::repeat_n(b'0', 6 - kept.len()));
    for d in digits {
        micro = micro
            .checked_mul(10)
            .and_then(|m| m.checked_add(i64::from(d - b'0')))
            .with_context(|| format!("{text:?} is too large"))?;
    }
    Ok(micro)
}

/// Micro-dollars shown as dollars, for error messages.
struct Usd(i64);

impl fmt::Display for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}.{:06}", self.0 / 1_000_000, self.0 % 1_000_000)
    }
}
