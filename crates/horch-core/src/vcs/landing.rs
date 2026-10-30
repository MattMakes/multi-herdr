//! Worker worktree claims and loss-resistant landing.
//!
//! A [`Landing`] keeps one durable registry per repository. Landing is
//! serialized with that registry's lock, so concurrent workers always rebase
//! and fast-forward in a linear order.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use horch_marketplace::git::{GitOutput, GitRunner};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::fsx::{self, DirLock, PRIVATE_FILE};

const LOCK_STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_HEARTBEAT: Duration = Duration::from_secs(60);

/// The durable state of a worktree claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimState {
    Open,
    Landed,
    Unlanded,
}

/// A durable ownership record for one worker worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub name: String,
    pub role: String,
    pub record_id: Option<String>,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
    pub base_commit: String,
    #[serde(with = "rfc3339")]
    pub created_at: DateTime<Utc>,
    pub state: ClaimState,
    pub landed_commit: Option<String>,
    pub reason: Option<String>,
}

/// Live facts about a registered claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimStatus {
    pub ahead: usize,
    pub dirty: usize,
    pub contained: bool,
    pub exists: bool,
}

/// A linked worktree which is not present in the claim registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unregistered {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub ahead: usize,
    pub dirty: usize,
}

/// A linked worktree the scan could not read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: PathBuf,
    /// `prunable` (git cannot reach its gitdir) or `folder missing`.
    pub reason: &'static str,
}

/// What [`Landing::unregistered`] found, and what it had to leave out.
///
/// A skipped path is named, never dropped in silence: a deleted folder used
/// to fail the whole scan, and `done` then reported nothing at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnregisteredScan {
    pub entries: Vec<Unregistered>,
    pub skipped: Vec<Skipped>,
}

impl UnregisteredScan {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A linked worktree as `git worktree list --porcelain` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Worktree {
    path: PathBuf,
    branch: Option<String>,
    /// git's reason, when the folder or its gitdir is gone.
    prunable: Option<String>,
}

/// The result of a successful landing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    pub branch: String,
    pub base: String,
    pub commit: String,
    pub commits: usize,
}

/// Worktree registry and landing operations.
#[derive(Debug, Clone)]
pub struct Landing {
    runner: GitRunner,
    state_root: PathBuf,
    env: Vec<(OsString, OsString)>,
    /// Worktree paths the task ledger owns, which the scan never reports.
    recorded: Vec<PathBuf>,
    lock_stale_after: Duration,
    lock_timeout: Duration,
    lock_heartbeat: Duration,
}

#[derive(Debug)]
struct Repo {
    top: PathBuf,
    registry_dir: PathBuf,
    key: String,
}

impl Landing {
    pub fn new(git_bin: PathBuf, state_root: PathBuf) -> Self {
        Self {
            runner: GitRunner::new(git_bin),
            state_root,
            env: Vec::new(),
            recorded: Vec::new(),
            lock_stale_after: LOCK_STALE_AFTER,
            lock_timeout: LOCK_TIMEOUT,
            lock_heartbeat: LOCK_HEARTBEAT,
        }
    }

    /// Paths the task ledger records as worktrees. Its worktrees are not in
    /// the claim registry, so without this every worker's `done` lists the
    /// other workers' in-flight worktrees as unregistered.
    pub fn with_recorded_worktrees(mut self, paths: Vec<PathBuf>) -> Self {
        self.recorded = paths;
        self
    }

    /// Add an environment variable to every git child process.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.runner = self.runner.with_env(key, value);
        self.env.push((key.into(), value.into()));
        self
    }

    #[cfg(test)]
    fn with_lock_timing(mut self, stale: Duration, timeout: Duration, heartbeat: Duration) -> Self {
        self.lock_stale_after = stale;
        self.lock_timeout = timeout;
        self.lock_heartbeat = heartbeat;
        self
    }

    /// Create and register `wt/<role>-<name>` from the current base branch.
    pub fn create(
        &self,
        dir: &Path,
        role: &str,
        record_id: Option<&str>,
        name: &str,
    ) -> Result<Claim> {
        validate_component("role", role)?;
        validate_component("name", name)?;
        let repo = self.repo(dir)?;
        let base = self
            .current_branch(dir)?
            .ok_or_else(|| anyhow!("cannot create a worktree from a detached HEAD"))?;
        let base_commit = self.text(dir, &["rev-parse", "--verify", "HEAD"])?;
        let branch = format!("wt/{role}-{name}");
        let path = repo.top.join(".worktrees").join(format!("{role}-{name}"));

        let _guard = self.lock(&repo)?;
        let mut claims = self.load_unlocked(&repo)?;
        if claims
            .iter()
            .any(|claim| claim.role == role && claim.name == name)
        {
            bail!("worktree name '{name}' already exists for role '{role}'");
        }
        if path.exists() || self.ref_exists(&repo.top, &format!("refs/heads/{branch}"))? {
            bail!(
                "worktree name '{name}' already exists at {} or branch {branch} exists",
                path.display()
            );
        }
        let path_arg = path_text(&path)?;
        self.run(
            &repo.top,
            &["worktree", "add", "-b", &branch, path_arg, &base],
        )
        .with_context(|| {
            format!(
                "creating worktree {}. Git kept the base {base} unchanged; retry with: git -C {} worktree add -b {branch} {} {base}",
                path.display(),
                repo.top.display(),
                path.display()
            )
        })?;
        let claim = Claim {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.to_owned(),
            role: role.to_owned(),
            record_id: record_id.map(str::to_owned),
            path,
            branch,
            base,
            base_commit,
            created_at: Utc::now(),
            state: ClaimState::Open,
            landed_commit: None,
            reason: None,
        };
        claims.push(claim.clone());
        self.write_unlocked(&repo, &claims).with_context(|| {
            format!(
                "the worktree and branch were kept, but the claim registry was not written; inspect with: git -C {} worktree list",
                repo.top.display()
            )
        })?;
        Ok(claim)
    }

    /// Land one open claim owned by `role` into its base branch.
    pub fn land(
        &self,
        dir: &Path,
        role: &str,
        name: Option<&str>,
        check: Option<&str>,
    ) -> Result<Landed> {
        let repo = self.repo(dir)?;
        let guard = self.lock(&repo)?;
        let mut claims = self.load_unlocked(&repo)?;
        let matches: Vec<usize> = claims
            .iter()
            .enumerate()
            .filter(|(_, claim)| {
                claim.role == role
                    && claim.state == ClaimState::Open
                    && name.is_none_or(|wanted| claim.name == wanted)
            })
            .map(|(index, _)| index)
            .collect();
        if matches.len() != 1 {
            let names = claims
                .iter()
                .filter(|claim| claim.role == role && claim.state == ClaimState::Open)
                .map(|claim| claim.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "cannot select one open claim for role '{role}'; found {}: [{}]",
                matches.len(),
                names
            );
        }
        let index = matches[0];
        let claim = claims[index].clone();
        let result = self.land_locked(&repo, &claim, check, &guard).map_err(|error| {
            anyhow!(
                "{error}\nKept branch {} and worktree when they still exist. Continue by hand:\ngit -C {} status\ngit -C {} rebase {}\ngit -C {} merge --ff-only {}\ngit -C {} worktree remove {}\ngit -C {} branch -d {}",
                claim.branch,
                claim.path.display(),
                claim.path.display(),
                claim.base,
                repo.top.display(),
                claim.branch,
                repo.top.display(),
                claim.path.display(),
                repo.top.display(),
                claim.branch
            )
        })?;
        claims[index].state = ClaimState::Landed;
        claims[index].landed_commit = Some(result.commit.clone());
        claims[index].reason = None;
        self.write_unlocked(&repo, &claims).with_context(|| {
            format!(
                "landed {} at {}, but the registry update failed; inspect {}",
                claim.branch,
                result.commit,
                self.registry_path(&repo).display()
            )
        })?;
        Ok(result)
    }

    /// Read live status for `claim`.
    pub fn status(&self, claim: &Claim) -> Result<ClaimStatus> {
        let exists = claim.path.is_dir();
        if !exists {
            let repo = claim.path.parent().and_then(Path::parent).ok_or_else(|| {
                anyhow!(
                    "claim path {} has no repository parent",
                    claim.path.display()
                )
            })?;
            let branch_exists = self.ref_exists(repo, &format!("refs/heads/{}", claim.branch))?;
            let ahead = if branch_exists {
                self.lines(
                    repo,
                    &["rev-list", &format!("{}..{}", claim.base, claim.branch)],
                )?
                .len()
            } else {
                0
            };
            let contained = if branch_exists {
                self.is_ancestor(repo, &claim.branch, &claim.base)?
            } else {
                claim.state == ClaimState::Landed
            };
            return Ok(ClaimStatus {
                ahead,
                dirty: 0,
                contained,
                exists: false,
            });
        }
        let dirty = line_count(&self.text(&claim.path, &["status", "--porcelain"])?);
        let ahead = self
            .lines(
                &claim.path,
                &["rev-list", &format!("{}..{}", claim.base, claim.branch)],
            )?
            .len();
        let contained = self.is_ancestor(&claim.path, &claim.branch, &claim.base)?;
        Ok(ClaimStatus {
            ahead,
            dirty,
            contained,
            exists,
        })
    }

    /// Return all registered claims for the repository containing `dir`.
    pub fn claims(&self, dir: &Path) -> Result<Vec<Claim>> {
        let repo = self.repo(dir)?;
        let _guard = self.lock(&repo)?;
        self.load_unlocked(&repo)
    }

    /// Return open claims owned by one ledger record.
    pub fn open_claims_for_record(&self, dir: &Path, record_id: &str) -> Result<Vec<Claim>> {
        Ok(self
            .claims(dir)?
            .into_iter()
            .filter(|claim| {
                claim.state == ClaimState::Open && claim.record_id.as_deref() == Some(record_id)
            })
            .collect())
    }

    /// Return whether any registry has an open claim owned by one ledger record.
    pub fn record_has_open_claims(&self, record_id: &str) -> Result<bool> {
        let registry_dir = self.state_root.join("worktrees");
        let entries = match std::fs::read_dir(&registry_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", registry_dir.display()))
            }
        };
        let mut found = false;
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading {}", registry_dir.display()))?
                .path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)
                .with_context(|| format!("reading claim registry {}", path.display()))?;
            let claims: Vec<Claim> = serde_json::from_slice(&bytes)
                .with_context(|| format!("reading claim registry {}", path.display()))?;
            if claims.iter().any(|claim| {
                claim.state == ClaimState::Open && claim.record_id.as_deref() == Some(record_id)
            }) {
                found = true;
            }
        }
        Ok(found)
    }

    /// Mark a claim unlanded without removing its branch or worktree.
    pub fn mark_unlanded(&self, dir: &Path, claim_id: &str, reason: &str) -> Result<Claim> {
        let repo = self.repo(dir)?;
        let _guard = self.lock(&repo)?;
        let mut claims = self.load_unlocked(&repo)?;
        let claim = claims
            .iter_mut()
            .find(|claim| claim.id == claim_id)
            .ok_or_else(|| anyhow!("claim '{claim_id}' does not exist"))?;
        if claim.state != ClaimState::Open {
            bail!("claim '{claim_id}' is not open");
        }
        claim.state = ClaimState::Unlanded;
        claim.reason = Some(reason.to_owned());
        let result = claim.clone();
        self.write_unlocked(&repo, &claims)?;
        Ok(result)
    }

    /// List changed linked worktrees which have no registry claim.
    ///
    /// Only worktrees under the repo's `.worktrees/` folder are fleet
    /// worktrees: the operator's own checkouts anywhere on disk are not
    /// reported. A path git cannot read is skipped and named, so one deleted
    /// folder does not fail the scan.
    pub fn unregistered(&self, dir: &Path) -> Result<UnregisteredScan> {
        let repo = self.repo(dir)?;
        let base = self
            .current_branch(dir)?
            .ok_or_else(|| anyhow!("cannot scan unregistered worktrees from a detached HEAD"))?;
        let registered = self.claims(dir)?;
        let worktrees_root = repo.top.join(".worktrees");
        let mut scan = UnregisteredScan::default();
        for worktree in self.worktrees(&repo.top)? {
            if worktree.path == repo.top || !worktree.path.starts_with(&worktrees_root) {
                continue;
            }
            // Every git call inside a worktree whose folder or gitdir is gone
            // exits 128. Name it and move on.
            let prunable = worktree.prunable.is_some();
            if prunable || !worktree.path.is_dir() {
                scan.skipped.push(Skipped {
                    path: worktree.path,
                    reason: if prunable {
                        "prunable"
                    } else {
                        "folder missing"
                    },
                });
                continue;
            }
            if registered.iter().any(|claim| claim.path == worktree.path)
                || self.recorded.contains(&worktree.path)
            {
                continue;
            }
            if worktree
                .path
                .strip_prefix(&worktrees_root)
                .ok()
                .and_then(|relative| relative.components().next())
                .is_some_and(|part| part.as_os_str().to_string_lossy().starts_with('_'))
            {
                continue;
            }
            let path = worktree.path;
            let branch = worktree.branch;
            let dirty = line_count(&self.text(&path, &["status", "--porcelain"])?);
            if branch.is_none() && dirty == 0 {
                continue;
            }
            let ahead = self
                .lines(&path, &["rev-list", &format!("{base}..HEAD")])?
                .len();
            if ahead != 0 || dirty != 0 {
                scan.entries.push(Unregistered {
                    path,
                    branch,
                    ahead,
                    dirty,
                });
            }
        }
        scan.entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(scan)
    }

    fn land_locked(
        &self,
        repo: &Repo,
        claim: &Claim,
        check: Option<&str>,
        guard: &fsx::DirLockGuard,
    ) -> Result<Landed> {
        if !claim.path.is_dir() {
            bail!("worktree {} does not exist", claim.path.display());
        }
        let dirty = self.text(&claim.path, &["status", "--porcelain"])?;
        if !dirty.is_empty() {
            bail!(
                "refused to land: worktree {} has {} modified, staged, or untracked entries",
                claim.path.display(),
                line_count(&dirty)
            );
        }

        let base_checkouts = self
            .worktrees(&repo.top)?
            .into_iter()
            .filter(|worktree| worktree.branch.as_deref() == Some(claim.base.as_str()))
            .map(|worktree| worktree.path)
            .collect::<Vec<_>>();
        if base_checkouts.len() > 1 {
            bail!(
                "refused to land: base {} is checked out in {} worktrees: {}",
                claim.base,
                base_checkouts.len(),
                base_checkouts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        let old_branch = self.text(&claim.path, &["rev-parse", "--verify", &claim.branch])?;
        let base_tip = self.text(&repo.top, &["rev-parse", "--verify", &claim.base])?;
        let rebase = self.output(&claim.path, &["rebase", &claim.base])?;
        if rebase.status != 0 {
            let conflicts = self
                .text_allow_failure(&claim.path, &["diff", "--name-only", "--diff-filter=U"])
                .unwrap_or_default();
            let abort = self.output(&claim.path, &["rebase", "--abort"])?;
            if abort.status != 0 {
                bail!(
                    "git rebase failed and git rebase --abort also failed: {}",
                    stderr(&abort)
                );
            }
            let restored = self.text(&claim.path, &["rev-parse", "--verify", &claim.branch])?;
            if restored != old_branch {
                bail!("git rebase --abort did not restore branch {}", claim.branch);
            }
            let detail = if conflicts.is_empty() {
                stderr(&rebase)
            } else {
                format!(
                    "conflicting files: {}",
                    conflicts.lines().collect::<Vec<_>>().join(", ")
                )
            };
            bail!("git rebase {} failed: {detail}", claim.base);
        }
        let new_branch = self.text(&claim.path, &["rev-parse", "--verify", &claim.branch])?;
        let after_rebase = (|| {
            if let Some(command) = check {
                let status = self.run_check_with_heartbeat(&claim.path, command, guard)?;
                if !status.success() {
                    bail!("check failed with status {status}: {command}");
                }
            }

            let commits = self
                .lines(
                    &claim.path,
                    &["rev-list", &format!("{base_tip}..{new_branch}")],
                )?
                .len();
            let base_checkout = base_checkouts.into_iter().next();
            let base_was_checked_out = base_checkout.is_some();
            match base_checkout {
                Some(path) => {
                    self.run(&path, &["merge", "--ff-only", &claim.branch])?;
                }
                None => {
                    self.run(
                        &repo.top,
                        &["fetch", ".", &format!("{}:{}", claim.branch, claim.base)],
                    )?;
                }
            }
            let contains = self.is_ancestor(&repo.top, &claim.branch, &claim.base)?;
            if !contains {
                bail!(
                    "base {} does not contain {} after fast-forward",
                    claim.base,
                    claim.branch
                );
            }
            // `branch -d` checks the upstream when one exists. Set it when the
            // repository's current checkout is unrelated to the landed base.
            if !base_was_checked_out {
                self.run(
                    &repo.top,
                    &["branch", "--set-upstream-to", &claim.base, &claim.branch],
                )?;
            }
            self.run(&repo.top, &["worktree", "remove", path_text(&claim.path)?])?;
            self.run(&repo.top, &["branch", "-d", &claim.branch])?;
            Ok(Landed {
                branch: claim.branch.clone(),
                base: claim.base.clone(),
                commit: new_branch.clone(),
                commits,
            })
        })();
        after_rebase.map_err(|error| {
            if old_branch == new_branch {
                error
            } else {
                anyhow!(
                    "{error}\nbranch {} was rebased from {} to {}; the old tip is {} in the reflog",
                    claim.branch,
                    old_branch,
                    new_branch,
                    old_branch
                )
            }
        })
    }

    fn run_check_with_heartbeat(
        &self,
        dir: &Path,
        command: &str,
        guard: &fsx::DirLockGuard,
    ) -> Result<std::process::ExitStatus> {
        std::thread::scope(|scope| {
            let (stop, stopped) = std::sync::mpsc::channel();
            let heartbeat = scope.spawn(move || -> fsx::Result<()> {
                loop {
                    match stopped.recv_timeout(self.lock_heartbeat) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return Ok(())
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => guard.refresh()?,
                    }
                }
            });
            let status = self.run_check(dir, command);
            let _ = stop.send(());
            let refreshed = heartbeat
                .join()
                .expect("lock heartbeat thread does not panic");
            status.and_then(|status| {
                refreshed.map_err(anyhow::Error::from)?;
                Ok(status)
            })
        })
    }

    fn repo(&self, dir: &Path) -> Result<Repo> {
        let top = self
            .worktrees(dir)?
            .into_iter()
            .next()
            .map(|worktree| worktree.path)
            .ok_or_else(|| anyhow!("git worktree list returned no main worktree"))?;
        let common_text = self.text(&top, &["rev-parse", "--git-common-dir"])?;
        let common = PathBuf::from(common_text);
        let common = if common.is_absolute() {
            common
        } else {
            top.join(common)
        };
        let common = common
            .canonicalize()
            .with_context(|| format!("canonicalizing git common dir {}", common.display()))?;
        let digest = Sha256::digest(common.as_os_str().to_string_lossy().as_bytes());
        let key = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Repo {
            top,
            registry_dir: self.state_root.join("worktrees"),
            key,
        })
    }

    fn lock(&self, repo: &Repo) -> Result<fsx::DirLockGuard> {
        DirLock::acquire(
            &repo.registry_dir,
            &repo.key,
            self.lock_stale_after,
            self.lock_timeout,
        )
        .map_err(Into::into)
    }

    fn registry_path(&self, repo: &Repo) -> PathBuf {
        repo.registry_dir.join(format!("{}.json", repo.key))
    }

    fn load_unlocked(&self, repo: &Repo) -> Result<Vec<Claim>> {
        let path = self.registry_path(repo);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("reading claim registry {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn write_unlocked(&self, repo: &Repo, claims: &[Claim]) -> Result<()> {
        std::fs::create_dir_all(&repo.registry_dir)
            .with_context(|| format!("creating {}", repo.registry_dir.display()))?;
        let bytes = serde_json::to_vec_pretty(claims)?;
        fsx::write_atomic(&self.registry_path(repo), &bytes, PRIVATE_FILE).map_err(Into::into)
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        self.runner.run(dir, args).map_err(Into::into)
    }

    fn output(&self, dir: &Path, args: &[&str]) -> Result<GitOutput> {
        self.runner.output(dir, args).map_err(Into::into)
    }

    fn text(&self, dir: &Path, args: &[&str]) -> Result<String> {
        Ok(self.run(dir, args)?.stdout_text())
    }

    fn text_allow_failure(&self, dir: &Path, args: &[&str]) -> Result<String> {
        Ok(self.output(dir, args)?.stdout_text())
    }

    fn lines(&self, dir: &Path, args: &[&str]) -> Result<Vec<String>> {
        Ok(self
            .text(dir, args)?
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    fn current_branch(&self, dir: &Path) -> Result<Option<String>> {
        let out = self.output(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        match out.status {
            0 => Ok(Some(out.stdout_text())),
            1 => Ok(None),
            _ => bail!("git symbolic-ref failed: {}", stderr(&out)),
        }
    }

    fn ref_exists(&self, dir: &Path, reference: &str) -> Result<bool> {
        let out = self.output(dir, &["show-ref", "--verify", "--quiet", reference])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            _ => bail!("git show-ref failed: {}", stderr(&out)),
        }
    }

    fn is_ancestor(&self, dir: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
        let out = self.output(dir, &["merge-base", "--is-ancestor", ancestor, descendant])?;
        match out.status {
            0 => Ok(true),
            1 => Ok(false),
            _ => bail!("git merge-base --is-ancestor failed: {}", stderr(&out)),
        }
    }

    fn worktrees(&self, repo: &Path) -> Result<Vec<Worktree>> {
        let text = self.text(repo, &["worktree", "list", "--porcelain"])?;
        let mut result = Vec::new();
        for block in text.split("\n\n") {
            let mut path = None;
            let mut branch = None;
            let mut prunable = None;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(value));
                } else if let Some(value) = line.strip_prefix("branch ") {
                    branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_owned(),
                    );
                } else if let Some(value) = line.strip_prefix("prunable") {
                    // `prunable <reason>` on git 2.36 and later.
                    prunable = Some(value.trim().to_owned());
                }
            }
            if let Some(path) = path {
                result.push(Worktree {
                    path,
                    branch,
                    prunable,
                });
            }
        }
        Ok(result)
    }

    fn run_check(&self, dir: &Path, command: &str) -> Result<std::process::ExitStatus> {
        let mut child = Command::new("sh");
        child
            .arg("-c")
            .arg(command)
            .current_dir(dir)
            .stdin(Stdio::null());
        for (key, value) in &self.env {
            child.env(key, value);
        }
        child
            .status()
            .with_context(|| format!("running check: {command}"))
    }
}

fn validate_component(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\']) {
        bail!("invalid worktree {label} '{value}'");
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("path {} is not valid UTF-8", path.display()))
}

fn line_count(text: &str) -> usize {
    text.lines().filter(|line| !line.is_empty()).count()
}

fn stderr(output: &GitOutput) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

mod rfc3339 {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_rfc3339())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&value)
            .map(|date| date.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct Fixture {
        _temp: TempDir,
        repo: PathBuf,
        state: PathBuf,
        config: PathBuf,
        landing: Landing,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::Builder::new()
                .prefix("horch-arc-31-")
                .tempdir_in(std::env::temp_dir())
                .unwrap();
            let repo = temp.path().join("repo");
            let state = temp.path().join("state");
            let config = temp.path().join("gitconfig");
            fs::write(&config, "").unwrap();
            fs::create_dir(&repo).unwrap();
            run_git(&repo, &config, &["init", "-b", "main"]);
            fs::write(repo.join("common.txt"), "initial\n").unwrap();
            run_git(&repo, &config, &["add", "common.txt"]);
            run_git(&repo, &config, &["commit", "-m", "initial"]);
            let landing = configured_landing(&state, &config);
            Self {
                _temp: temp,
                repo,
                state,
                config,
                landing,
            }
        }

        fn create(&self, role: &str, record: Option<&str>, name: &str) -> Claim {
            self.landing.create(&self.repo, role, record, name).unwrap()
        }

        fn commit_file(&self, dir: &Path, name: &str, contents: &str, message: &str) {
            fs::write(dir.join(name), contents).unwrap();
            run_git(dir, &self.config, &["add", name]);
            run_git(dir, &self.config, &["commit", "-m", message]);
        }

        fn git(&self, dir: &Path, args: &[&str]) -> String {
            run_git(dir, &self.config, args)
        }
    }

    fn configured_landing(state: &Path, config: &Path) -> Landing {
        Landing::new(PathBuf::from("git"), state.to_path_buf())
            .with_env("GIT_CONFIG_GLOBAL", config.to_str().unwrap())
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("GIT_AUTHOR_NAME", "Horch Test")
            .with_env("GIT_AUTHOR_EMAIL", "horch@example.invalid")
            .with_env("GIT_COMMITTER_NAME", "Horch Test")
            .with_env("GIT_COMMITTER_EMAIL", "horch@example.invalid")
    }

    fn run_git(dir: &Path, config: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Horch Test")
            .env("GIT_AUTHOR_EMAIL", "horch@example.invalid")
            .env("GIT_COMMITTER_NAME", "Horch Test")
            .env("GIT_COMMITTER_EMAIL", "horch@example.invalid")
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn arc_31_registry_round_trips() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", Some("record-1"), "one");
        let fresh = configured_landing(&fixture.state, &fixture.config);
        assert_eq!(fresh.claims(&fixture.repo).unwrap(), vec![claim]);
    }

    #[test]
    fn arc_31_create_makes_branch_and_worktree_from_base() {
        let fixture = Fixture::new();
        let base = fixture.git(&fixture.repo, &["rev-parse", "HEAD"]);
        let claim = fixture.create("worker", None, "one");
        assert_eq!(claim.base, "main");
        assert_eq!(claim.base_commit, base);
        assert!(claim.path.is_dir());
        assert_eq!(
            fixture.git(&claim.path, &["branch", "--show-current"]),
            claim.branch
        );
        assert_eq!(fixture.git(&claim.path, &["rev-parse", "HEAD"]), base);
    }

    #[test]
    fn arc_31_create_fails_on_detached_head() {
        let fixture = Fixture::new();
        fixture.git(&fixture.repo, &["checkout", "--detach"]);
        let error = fixture
            .landing
            .create(&fixture.repo, "worker", None, "one")
            .unwrap_err();
        assert!(error.to_string().contains("detached HEAD"));
    }

    #[test]
    fn arc_31_create_refuses_an_existing_name() {
        let fixture = Fixture::new();
        fixture.create("worker", None, "one");
        let error = fixture
            .landing
            .create(&fixture.repo, "worker", None, "one")
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn arc_31_land_fast_forwards_base_removes_worktree_and_branch() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let expected = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        let landed = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap();
        assert_eq!(landed.commit, expected);
        assert_eq!(landed.commits, 1);
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), expected);
        assert!(!claim.path.exists());
        assert!(!fixture
            .landing
            .ref_exists(&fixture.repo, &format!("refs/heads/{}", claim.branch))
            .unwrap());
        assert_eq!(
            fixture.landing.claims(&fixture.repo).unwrap()[0].state,
            ClaimState::Landed
        );
    }

    #[test]
    fn arc_31_land_from_inside_the_worktree_completes() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let expected = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        let landed = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap();
        assert_eq!(landed.commit, expected);
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), expected);
        assert!(!claim.path.exists());
        assert!(!fixture
            .landing
            .ref_exists(&fixture.repo, &format!("refs/heads/{}", claim.branch))
            .unwrap());
        assert_eq!(
            fixture.landing.claims(&fixture.repo).unwrap()[0].state,
            ClaimState::Landed
        );
    }

    #[test]
    fn arc_31_land_rebases_when_base_moved() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        fixture.commit_file(&fixture.repo, "base.txt", "base\n", "base");
        let landed = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap();
        assert_eq!(landed.commits, 1);
        assert!(fixture.repo.join("worker.txt").is_file());
        assert!(fixture.repo.join("base.txt").is_file());
    }

    #[test]
    fn arc_31_land_refuses_modified_or_untracked_files() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let base = fixture.git(&fixture.repo, &["rev-parse", "main"]);
        fs::write(claim.path.join("worker.txt"), "modified\n").unwrap();
        fs::write(claim.path.join("untracked.txt"), "new\n").unwrap();
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("2 modified, staged, or untracked"));
        assert!(error.to_string().contains("git -C"));
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), base);
        assert!(claim.path.is_dir());
    }

    #[test]
    fn arc_31_land_rebase_conflict_aborts_and_keeps_everything() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "common.txt", "worker\n", "worker");
        let branch = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        fixture.commit_file(&fixture.repo, "common.txt", "base\n", "base");
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap_err();
        assert!(error.to_string().contains("common.txt"));
        assert_eq!(fixture.git(&claim.path, &["rev-parse", "HEAD"]), branch);
        assert!(claim.path.is_dir());
    }

    #[test]
    fn arc_31_land_failing_check_keeps_everything() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let base = fixture.git(&fixture.repo, &["rev-parse", "main"]);
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), Some("exit 9"))
            .unwrap_err();
        assert!(error.to_string().contains("check failed"));
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), base);
        assert!(claim.path.is_dir());
    }

    #[test]
    fn arc_31_failed_check_after_rebase_names_the_old_tip() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let old = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        fixture.commit_file(&fixture.repo, "base.txt", "base\n", "base");
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), Some("exit 9"))
            .unwrap_err();
        let new = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        assert_ne!(new, old, "the moved base causes a real rebase");
        assert!(
            error.to_string().contains(&format!(
                "branch {} was rebased from {old} to {new}; the old tip is {old} in the reflog",
                claim.branch
            )),
            "{error:#}"
        );
    }

    #[test]
    fn arc_31_land_refuses_base_checked_out_twice() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let second = fixture._temp.path().join("second-main");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "--force",
                second.to_str().unwrap(),
                "main",
            ],
        );
        let before = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("base main is checked out in 2 worktrees"),
            "{text}"
        );
        assert!(text.contains(&fixture.repo.display().to_string()), "{text}");
        assert!(text.contains(&second.display().to_string()), "{text}");
        assert_eq!(fixture.git(&claim.path, &["rev-parse", "HEAD"]), before);
    }

    #[test]
    fn arc_31_long_check_keeps_the_lock() {
        // The check holds until the test releases it. The test ages the
        // lock past its stale age by hand and waits for the heartbeat to
        // refresh it: no wall-clock margin, which a loaded test run (a slow
        // fsync in the refresh) broke at a 0.4 s stale age.
        let fixture = Fixture::new();
        let stale = Duration::from_secs(3600);
        let landing = configured_landing(&fixture.state, &fixture.config).with_lock_timing(
            stale,
            Duration::from_millis(200),
            Duration::from_millis(20),
        );
        let claim = landing
            .create(&fixture.repo, "worker", None, "one")
            .unwrap();
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let marker = fixture._temp.path().join("check-started");
        let release = fixture._temp.path().join("check-release");
        let command = format!(
            "touch {}; while [ ! -f {} ]; do sleep 0.02; done",
            marker.display(),
            release.display()
        );
        let worker_path = claim.path.clone();
        let worker_landing = landing.clone();
        let land = std::thread::spawn(move || {
            worker_landing.land(&worker_path, "worker", Some("one"), Some(&command))
        });
        let wait_for = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !done() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if !done() {
                // A failed assert must not leave the check looping forever.
                fs::write(&release, "").unwrap();
                panic!("{what}");
            }
        };
        wait_for("the check started", &|| marker.exists());
        let lock = fs::read_dir(fixture.state.join("worktrees"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "lock"))
            .expect("the land holds the registry lock");
        let mtime = || fs::metadata(&lock).and_then(|m| m.modified()).ok();
        let aged = std::time::SystemTime::now() - stale * 2;
        fs::File::open(&lock).unwrap().set_modified(aged).unwrap();
        // Without the heartbeat the lock stays stale.
        wait_for("the heartbeat refreshed the lock", &|| {
            mtime().is_some_and(|t| t > aged + stale)
        });
        let second = landing.create(&fixture.repo, "worker-2", None, "two");
        // Release before any assert: a failed assert must not leave the
        // check looping forever.
        fs::write(&release, "").unwrap();
        let error = second.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error:#}");
        land.join().unwrap().unwrap();
    }

    #[test]
    fn arc_31_land_refuses_when_base_checkout_has_conflicting_edits() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "common.txt", "worker\n", "worker");
        let base = fixture.git(&fixture.repo, &["rev-parse", "main"]);
        fs::write(fixture.repo.join("common.txt"), "local\n").unwrap();
        let error = fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap_err();
        assert!(error.to_string().contains("local changes"));
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), base);
        assert_eq!(
            fs::read_to_string(fixture.repo.join("common.txt")).unwrap(),
            "local\n"
        );
        assert!(claim.path.is_dir());
    }

    #[test]
    fn arc_31_land_updates_a_base_that_is_not_checked_out() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        let expected = fixture.git(&claim.path, &["rev-parse", "HEAD"]);
        fixture.git(&fixture.repo, &["switch", "-c", "parking"]);
        fixture
            .landing
            .land(&claim.path, "worker", Some("one"), None)
            .unwrap();
        assert_eq!(fixture.git(&fixture.repo, &["rev-parse", "main"]), expected);
        assert_eq!(
            fixture.git(&fixture.repo, &["branch", "--show-current"]),
            "parking"
        );
    }

    #[test]
    fn arc_31_two_lands_in_parallel_both_land() {
        let fixture = Fixture::new();
        let one = fixture.create("worker-1", None, "one");
        let two = fixture.create("worker-2", None, "two");
        fixture.commit_file(&one.path, "one.txt", "one\n", "one");
        fixture.commit_file(&two.path, "two.txt", "two\n", "two");
        let landing = Arc::new(fixture.landing.clone());
        let repo = fixture.repo.clone();
        let first = {
            let landing = Arc::clone(&landing);
            let repo = repo.clone();
            std::thread::spawn(move || landing.land(&repo, "worker-1", Some("one"), None))
        };
        let second = {
            let landing = Arc::clone(&landing);
            std::thread::spawn(move || landing.land(&repo, "worker-2", Some("two"), None))
        };
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert!(fixture.repo.join("one.txt").is_file());
        assert!(fixture.repo.join("two.txt").is_file());
        assert_eq!(
            fixture
                .landing
                .claims(&fixture.repo)
                .unwrap()
                .iter()
                .filter(|claim| claim.state == ClaimState::Landed)
                .count(),
            2
        );
    }

    #[test]
    fn arc_31_open_claims_are_per_record() {
        let fixture = Fixture::new();
        fixture.create("worker-1", Some("record-1"), "one");
        fixture.create("worker-2", Some("record-2"), "two");
        assert_eq!(
            fixture
                .landing
                .open_claims_for_record(&fixture.repo, "record-1")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn arc_31_open_claim_lookup_needs_no_git() {
        let fixture = Fixture::new();
        fixture.create("worker-1", Some("record-1"), "one");
        let landing = Landing::new(
            fixture._temp.path().join("git-does-not-exist"),
            fixture.state.clone(),
        );
        assert!(landing.record_has_open_claims("record-1").unwrap());
        assert!(!landing.record_has_open_claims("record-2").unwrap());

        let no_registry = Landing::new(
            fixture._temp.path().join("git-does-not-exist"),
            fixture._temp.path().join("state-does-not-exist"),
        );
        assert!(!no_registry.record_has_open_claims("record-1").unwrap());

        let invalid_state = fixture._temp.path().join("invalid-state");
        std::fs::create_dir_all(invalid_state.join("worktrees")).unwrap();
        std::fs::write(invalid_state.join("worktrees/broken.json"), b"not json").unwrap();
        let invalid = Landing::new(
            fixture._temp.path().join("git-does-not-exist"),
            invalid_state,
        );
        assert!(invalid.record_has_open_claims("record-1").is_err());
    }

    #[test]
    fn arc_31_mark_unlanded_keeps_branch_and_worktree() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", Some("record-1"), "one");
        let updated = fixture
            .landing
            .mark_unlanded(&fixture.repo, &claim.id, "operator decision")
            .unwrap();
        assert_eq!(updated.state, ClaimState::Unlanded);
        assert_eq!(updated.reason.as_deref(), Some("operator decision"));
        assert!(claim.path.is_dir());
        assert!(fixture
            .landing
            .ref_exists(&fixture.repo, &format!("refs/heads/{}", claim.branch))
            .unwrap());
    }

    #[test]
    fn arc_31_unregistered_lists_raw_worktree_with_commits() {
        let fixture = Fixture::new();
        let path = fixture.repo.join(".worktrees/raw");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "raw",
                path.to_str().unwrap(),
                "main",
            ],
        );
        fixture.commit_file(&path, "raw.txt", "raw\n", "raw");
        let found = fixture.landing.unregistered(&fixture.repo).unwrap();
        assert_eq!(found.entries.len(), 1);
        assert!(found.skipped.is_empty());
        assert_eq!(found.entries[0].path, path.canonicalize().unwrap());
        assert_eq!(found.entries[0].branch.as_deref(), Some("raw"));
        assert_eq!(found.entries[0].ahead, 1);
    }

    /// P27: one deleted folder used to fail the whole scan, so `done` reported
    /// nothing at all. It is skipped and named, and the rest is still read.
    #[test]
    fn arc_31_unregistered_skips_a_deleted_folder_and_names_it() {
        let fixture = Fixture::new();
        let gone = fixture.repo.join(".worktrees/gone");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "gone",
                gone.to_str().unwrap(),
                "main",
            ],
        );
        let gone_path = gone.canonicalize().unwrap();
        let kept = fixture.repo.join(".worktrees/kept");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "kept",
                kept.to_str().unwrap(),
                "main",
            ],
        );
        fixture.commit_file(&kept, "kept.txt", "kept\n", "kept");
        std::fs::remove_dir_all(&gone).unwrap();

        let found = fixture.landing.unregistered(&fixture.repo).unwrap();
        assert_eq!(
            found.entries.len(),
            1,
            "the readable worktree is still read"
        );
        assert_eq!(found.entries[0].path, kept.canonicalize().unwrap());
        assert_eq!(found.skipped.len(), 1, "the deleted one is named");
        assert_eq!(found.skipped[0].path, gone_path);
        assert!(
            matches!(found.skipped[0].reason, "prunable" | "folder missing"),
            "reason: {}",
            found.skipped[0].reason
        );
    }

    /// P30: the operator's own checkouts anywhere on disk are not fleet
    /// worktrees, however dirty they are.
    #[test]
    fn arc_31_unregistered_ignores_worktrees_outside_the_worktrees_folder() {
        let fixture = Fixture::new();
        let operator = fixture._temp.path().join("operator-checkout");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "operator",
                operator.to_str().unwrap(),
                "main",
            ],
        );
        fixture.commit_file(&operator, "operator.txt", "operator\n", "operator");
        std::fs::write(operator.join("operator.txt"), "dirty\n").unwrap();
        let found = fixture.landing.unregistered(&fixture.repo).unwrap();
        assert!(found.is_empty(), "entries: {:?}", found.entries);
        assert!(found.skipped.is_empty(), "skipped: {:?}", found.skipped);
    }

    /// A task-ledger worktree is not in the claim registry, so without the
    /// ledger's paths every worker's `done` lists its peers' work as
    /// UNREGISTERED.
    #[test]
    fn arc_31_unregistered_skips_recorded_worktrees() {
        let fixture = Fixture::new();
        let path = fixture.repo.join(".worktrees/T1-ledger");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "ledger",
                path.to_str().unwrap(),
                "main",
            ],
        );
        fixture.commit_file(&path, "ledger.txt", "ledger\n", "ledger");
        let landing = configured_landing(&fixture.state, &fixture.config)
            .with_recorded_worktrees(vec![path.canonicalize().unwrap()]);
        assert!(landing.unregistered(&fixture.repo).unwrap().is_empty());
        // Without the ledger's paths it is still reported.
        assert_eq!(
            fixture
                .landing
                .unregistered(&fixture.repo)
                .unwrap()
                .entries
                .len(),
            1
        );
    }

    #[test]
    fn arc_31_unregistered_skips_underscore_dirs_and_clean_detached() {
        let fixture = Fixture::new();
        let skipped = fixture.repo.join(".worktrees/_scratch");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "scratch",
                skipped.to_str().unwrap(),
                "main",
            ],
        );
        fixture.commit_file(&skipped, "scratch.txt", "scratch\n", "scratch");
        let detached = fixture._temp.path().join("detached");
        fixture.git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "--detach",
                detached.to_str().unwrap(),
                "main",
            ],
        );
        assert!(fixture
            .landing
            .unregistered(&fixture.repo)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn status_reads_a_live_and_a_removed_worktree() {
        let fixture = Fixture::new();
        let mut claim = fixture.create("worker", None, "one");
        fixture.commit_file(&claim.path, "worker.txt", "work\n", "worker");
        fs::write(claim.path.join("untracked.txt"), "new\n").unwrap();
        let live = fixture.landing.status(&claim).unwrap();
        assert_eq!(
            live,
            ClaimStatus {
                ahead: 1,
                dirty: 1,
                contained: false,
                exists: true
            }
        );

        let path = claim.path.to_str().unwrap().to_owned();
        fixture.git(&fixture.repo, &["worktree", "remove", "--force", &path]);
        let kept = fixture.landing.status(&claim).unwrap();
        assert_eq!(
            kept,
            ClaimStatus {
                ahead: 1,
                dirty: 0,
                contained: false,
                exists: false
            },
            "the branch outlives the folder"
        );

        fixture.git(&fixture.repo, &["branch", "-D", &claim.branch]);
        let gone = fixture.landing.status(&claim).unwrap();
        assert_eq!((gone.ahead, gone.contained, gone.exists), (0, false, false));
        claim.state = ClaimState::Landed;
        assert!(
            fixture.landing.status(&claim).unwrap().contained,
            "a landed claim without its branch counts as contained"
        );

        claim.path = PathBuf::from("w3-no-such-folder");
        let error = fixture.landing.status(&claim).unwrap_err();
        assert_eq!(
            error.to_string(),
            "claim path w3-no-such-folder has no repository parent"
        );
    }

    #[test]
    fn land_needs_exactly_one_open_claim_of_the_role() {
        let fixture = Fixture::new();
        let error = fixture
            .landing
            .land(&fixture.repo, "worker", None, None)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot select one open claim for role 'worker'; found 0: []"
        );
        fixture.create("worker", None, "one");
        fixture.create("worker", None, "two");
        fixture.create("other", None, "three");
        let error = fixture
            .landing
            .land(&fixture.repo, "worker", None, None)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot select one open claim for role 'worker'; found 2: [one, two]"
        );
        let error = fixture
            .landing
            .land(&fixture.repo, "worker", Some("nine"), None)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot select one open claim for role 'worker'; found 0: [one, two]"
        );
    }

    #[test]
    fn create_refuses_an_existing_branch() {
        let fixture = Fixture::new();
        fixture.git(&fixture.repo, &["branch", "wt/worker-one"]);
        let error = fixture
            .landing
            .create(&fixture.repo, "worker", None, "one")
            .unwrap_err();
        let path = fixture
            .repo
            .canonicalize()
            .unwrap()
            .join(".worktrees/worker-one");
        assert_eq!(
            error.to_string(),
            format!(
                "worktree name 'one' already exists at {} or branch wt/worker-one exists",
                path.display()
            )
        );
        assert!(!path.exists());
    }

    #[test]
    fn land_refuses_a_missing_worktree_and_keeps_the_claim_open() {
        let fixture = Fixture::new();
        let claim = fixture.create("worker", None, "one");
        let path = claim.path.to_str().unwrap().to_owned();
        fixture.git(&fixture.repo, &["worktree", "remove", "--force", &path]);
        let error = fixture
            .landing
            .land(&fixture.repo, "worker", Some("one"), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("worktree {path} does not exist\nKept branch")),
            "{error}"
        );
        assert_eq!(
            fixture.landing.claims(&fixture.repo).unwrap()[0].state,
            ClaimState::Open
        );
    }

    #[test]
    fn record_has_open_claims_reads_every_registry_and_skips_other_files() {
        let fixture = Fixture::new();
        fixture.create("worker", Some("record-1"), "one");
        let registries = fixture.state.join("worktrees");
        fs::write(registries.join("notes.txt"), b"not json").unwrap();
        assert!(fixture.landing.record_has_open_claims("record-1").unwrap());
        fs::write(registries.join("broken.json"), b"not json").unwrap();
        assert!(
            fixture.landing.record_has_open_claims("record-1").is_err(),
            "a broken registry fails the read even after a match"
        );
    }
}
