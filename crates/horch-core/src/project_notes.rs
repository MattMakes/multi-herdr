//! Durable, bounded project knowledge, separate from task and session notes.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
/// Maximum number of facts retained; oldest entries expire first.
pub const MAX_FACTS: usize = 100;
/// Maximum bytes in one fact.
pub const MAX_FACT_BYTES: usize = 2048;
/// The per-project facts file. Canonical roots share facts through symlink aliases.
pub fn path(state_root: &Path, project: &Path) -> Result<PathBuf> {
    let root = identity(project)?;
    let key = Sha256::digest(root.to_string_lossy().as_bytes());
    Ok(state_root.join("facts").join(format!("{key:x}.txt")))
}
// Git linked worktrees contain a gitdir pointer plus commondir. Resolve the
// common repository directory so notes made from any checkout share a store.
fn identity(project: &Path) -> Result<PathBuf> {
    let project = project
        .canonicalize()
        .context("resolving project for facts")?;
    for dir in project.ancestors() {
        let dotgit = dir.join(".git");
        if dotgit.is_dir() {
            return Ok(dir.to_owned());
        }
        if dotgit.is_file() {
            let text = std::fs::read_to_string(&dotgit)?;
            if let Some(gitdir) = text.trim().strip_prefix("gitdir: ") {
                let gitdir = dir.join(gitdir);
                if let Ok(common) = std::fs::read_to_string(gitdir.join("commondir")) {
                    let common = gitdir.join(common.trim()).canonicalize()?;
                    if common.file_name().is_some_and(|n| n == ".git") {
                        return Ok(common.parent().context("git common parent")?.to_owned());
                    }
                    return Ok(common);
                }
                return Ok(dir.to_owned());
            }
        }
    }
    Ok(project)
}
/// Read facts in oldest-to-newest order. A missing store is an empty store.
pub fn read(state_root: &Path, project: &Path) -> Result<Vec<String>> {
    let path = path(state_root, project)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(s
            .lines()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}
/// Append one line once, retaining the newest facts. Returns whether it was added.
pub fn record(state_root: &Path, project: &Path, fact: &str) -> Result<bool> {
    let fact = fact.trim();
    if fact.is_empty() || fact.contains(['\n', '\r']) || fact.len() > MAX_FACT_BYTES {
        bail!("a fact must be one nonempty line of at most {MAX_FACT_BYTES} bytes");
    }
    let file = path(state_root, project)?;
    let dir = file.parent().context("facts directory")?;
    std::fs::create_dir_all(dir)?;
    let key = file.file_stem().context("facts key")?.to_string_lossy();
    let _lock =
        crate::fsx::DirLock::acquire(dir, &key, Duration::from_secs(10), Duration::from_secs(60))?;
    let mut facts = read(state_root, project)?;
    if facts.iter().any(|f| f == fact) {
        return Ok(false);
    }
    facts.push(fact.to_owned());
    if facts.len() > MAX_FACTS {
        facts.drain(..facts.len() - MAX_FACTS);
    }
    crate::fsx::write_atomic(
        &file,
        format!("{}\n", facts.join("\n")).as_bytes(),
        crate::fsx::PRIVATE_FILE,
    )?;
    Ok(true)
}
/// Text for the briefing placeholder and claim output.
pub fn display(state_root: &Path, project: &Path) -> Result<String> {
    let facts = read(state_root, project)?;
    Ok(if facts.is_empty() {
        "(No project facts recorded.)".into()
    } else {
        facts
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    })
}
/// Host facts derive from the binary target, without ambient environment reads.
pub fn host_line() -> String {
    let tools = if cfg!(target_os = "macos") {
        "BSD tools; no GNU timeout; sed -i requires a backup suffix"
    } else if cfg!(target_os = "linux") {
        "GNU tools; check availability before using timeout or sed -i"
    } else {
        "host tools vary; check command support before use"
    };
    format!(
        "{} / {}; {tools}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}
