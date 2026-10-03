//! The loaded roster: built-ins, then each overlay directory on top.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::parser::{md_files, parse_base, parse_teammate};
use super::{Base, ExecRule, Teammate};

include!(concat!(env!("OUT_DIR"), "/builtin_teammates.rs"));

/// Every teammate and base visible to this process.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    pub(crate) teammates: BTreeMap<String, Teammate>,
    pub(crate) bases: BTreeMap<String, Base>,
    /// Directories that were overlaid, lowest precedence first. For `doctor`.
    pub sources: Vec<PathBuf>,
    /// The home directory this roster was loaded under, for `~/` paths in
    /// teammate files. `None` for the built-ins alone.
    pub(crate) home: Option<PathBuf>,
    /// The skills teammates may name. `None`: the compiled-in catalog.
    pub(crate) skill_catalog: Option<crate::skills::SkillCatalog>,
}

impl Roster {
    /// Compiled-in teammates only. Always succeeds, so a broken user directory
    /// can never leave the fleet with no roster at all.
    pub fn builtin() -> Result<Roster> {
        let mut r = Roster::default();
        for (name, text) in BUILTIN_BASES {
            r.bases.insert(
                name.to_string(),
                parse_base(name, text).with_context(|| format!("compiled-in base '{name}'"))?,
            );
        }
        for (name, text) in BUILTIN_TEAMMATES {
            r.teammates.insert(
                name.to_string(),
                parse_teammate(name, text)
                    .with_context(|| format!("compiled-in teammate '{name}'"))?,
            );
        }
        Ok(r)
    }

    /// Built-ins, overlaid by `<home>/.config/horch/teammates`, then by
    /// `roster_override` (`$HORCH_TEAMMATES_DIR`), then by `explicit`.
    ///
    /// A worker pane is a fresh shell started by the server: it inherits the
    /// user's profile, not the environment `horch` was invoked with. So a pane
    /// cannot see `$HORCH_TEAMMATES_DIR` and has to be told the path outright
    /// as `explicit`, the same way `HORCH_CLAUDE_BIN` travels in the worker's
    /// brief.
    pub fn load_layered(
        home: Option<&Path>,
        roster_override: Option<&Path>,
        explicit: Option<&str>,
    ) -> Result<Roster> {
        let mut r = Roster::builtin()?;
        r.home = home.map(Path::to_path_buf);
        let mut dirs = overlay_dirs(home, roster_override);
        if let Some(dir) = explicit.filter(|d| !d.is_empty()) {
            dirs.push(PathBuf::from(dir));
        }
        for dir in dirs {
            if dir.is_dir() {
                r.overlay(&dir)
                    .with_context(|| format!("loading teammates from {}", dir.display()))?;
                r.sources.push(dir);
            }
        }
        Ok(r)
    }

    /// Read one directory over the top of what is already loaded.
    pub fn overlay(&mut self, dir: &Path) -> Result<()> {
        let base_dir = dir.join("_base");
        if base_dir.is_dir() {
            for (stem, path) in md_files(&base_dir)? {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let base =
                    parse_base(&stem, &text).with_context(|| format!("in {}", path.display()))?;
                self.bases.insert(stem, base);
            }
        }
        for (stem, path) in md_files(dir)? {
            if stem.starts_with('_') {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let t =
                parse_teammate(&stem, &text).with_context(|| format!("in {}", path.display()))?;
            self.teammates.insert(stem, t);
        }
        Ok(())
    }

    /// Add or replace one teammate. For tests that need a roster variant.
    #[doc(hidden)]
    pub fn insert_for_test(&mut self, t: Teammate) {
        self.teammates.insert(t.name.clone(), t);
    }

    pub fn get(&self, name: &str) -> Option<&Teammate> {
        self.teammates.get(name)
    }

    /// Like [`Roster::get`], but with an error naming what is available.
    pub fn require(&self, name: &str) -> Result<&Teammate> {
        match self.teammates.get(name) {
            Some(t) => Ok(t),
            None => bail!(
                "unknown teammate '{name}'. Available: {}",
                self.names().join(", ")
            ),
        }
    }

    pub fn base(&self, name: &str) -> Option<&Base> {
        self.bases.get(name)
    }

    pub(crate) fn require_base(&self, name: &str) -> Result<&Base> {
        self.bases.get(name).with_context(|| {
            format!("teammate names base '{name}', which does not exist in _base/")
        })
    }

    /// All names, including hidden ones. Spawnable by name.
    pub fn names(&self) -> Vec<&str> {
        self.teammates.keys().map(|s| s.as_str()).collect()
    }

    /// What the orchestrator is offered: specialists first, then generics.
    /// Hidden teammates are excluded.
    pub fn offered(&self) -> Vec<&Teammate> {
        let mut v: Vec<&Teammate> = self.teammates.values().filter(|t| !t.hidden).collect();
        v.sort_by_key(|t| (t.generic, t.name.clone()));
        v
    }

    /// The `{roster}` substitution: `brief_description` lines and nothing else.
    pub fn roster_lines(&self) -> String {
        let offered = self.offered();
        let width = offered.iter().map(|t| t.name.len()).max().unwrap_or(0);
        offered
            .iter()
            .map(|t| {
                format!(
                    "  {:<width$}  {}",
                    t.name,
                    t.brief_description,
                    width = width
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The execpolicy rules a codex WORKER needs installed before launch.
    pub fn exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-execpolicy")
    }

    /// The execpolicy rules a codex ORCHESTRATOR needs instead: it never runs
    /// `horch note` or `horch done`, and the worker set allows it nothing it
    /// needs to build a fleet with.
    pub fn orchestrator_exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-orchestrator-execpolicy")
    }

    fn rules_of(&self, base: &str) -> &[ExecRule] {
        match self.bases.get(base) {
            Some(b) => &b.rules,
            None => &[],
        }
    }
}

/// Where a runtime directory may be found, lowest precedence first.
pub(crate) fn overlay_dirs(home: Option<&Path>, roster_override: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".config/horch/teammates"));
    }
    if let Some(dir) = roster_override {
        if !dir.as_os_str().is_empty() {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs
}
