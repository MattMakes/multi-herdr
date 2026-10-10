//! The loaded roster: built-ins, then each overlay directory on top.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::parser::{md_files, parse_base, parse_teammate};
use super::teammate::{CONTEXT_MESSAGES_BASE, CONTEXT_WINDOWS_BASE, HARNESS_DEFAULTS_BASE};
use super::{offered_in, with_project_skills, Base, ExecRule, ProjectFacts, Teammate};
use crate::compaction::window;
use crate::harness::HarnessKind;

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
    /// What the project directory holds, for `offer_when`. `None`: nothing
    /// was gathered, and every non-hidden teammate is offered.
    pub(crate) project: Option<ProjectFacts>,
    /// Overlay files that did not read or parse, keyed on teammate name.
    /// The rest of the roster loads without them; see [`Roster::load_warnings`].
    pub(crate) broken_teammates: BTreeMap<String, BrokenFile>,
    /// The same for `_base/` files, keyed on base name.
    pub(crate) broken_bases: BTreeMap<String, BrokenFile>,
    /// The overlay file each overlaid teammate or base came from. A name
    /// missing here is the built-in.
    teammate_origins: BTreeMap<String, PathBuf>,
    base_origins: BTreeMap<String, PathBuf>,
    /// Teammates whose FILE set `harness_defaults`, recorded before the
    /// roster attaches the real entries over it. Check rule 2 reads it.
    pub(crate) defaults_set_in_file: BTreeSet<String>,
}

/// One overlay file that did not load.
#[derive(Debug, Clone)]
pub(crate) struct BrokenFile {
    path: PathBuf,
    error: String,
    /// What is used instead: "the built-in" or an earlier file. `None`: the
    /// name is not in the roster at all.
    fallback: Option<String>,
}

impl BrokenFile {
    fn warning(&self, kind: &str, name: &str) -> String {
        let used = match &self.fallback {
            Some(f) => format!("using {f} instead"),
            None => format!("{kind} '{name}' is not available"),
        };
        format!(
            "{kind} '{name}' did not load from {}: {}; {used}",
            self.path.display(),
            self.error
        )
    }
}

fn fallback_for(origins: &BTreeMap<String, PathBuf>, name: &str, loaded: bool) -> Option<String> {
    if !loaded {
        return None;
    }
    Some(match origins.get(name) {
        Some(path) => format!("the earlier {}", path.display()),
        None => format!("the built-in '{name}'"),
    })
}

fn read_parsed<T>(path: &Path, parse: impl FnOnce(&str) -> Result<T>) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("reading: {e}"))?;
    parse(&text).map_err(|e| format!("{e:#}"))
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
            let t = parse_teammate(name, text)
                .with_context(|| format!("compiled-in teammate '{name}'"))?;
            r.insert_parsed(name.to_string(), t);
        }
        r.attach_harness_defaults();
        Ok(r)
    }

    /// Add a parsed teammate, and record whether its file set
    /// `harness_defaults` before [`Roster::attach_harness_defaults`]
    /// overwrites the field.
    fn insert_parsed(&mut self, name: String, t: Teammate) {
        if t.harness_defaults.is_empty() {
            self.defaults_set_in_file.remove(&name);
        } else {
            self.defaults_set_in_file.insert(name.clone());
        }
        self.teammates.insert(name, t);
    }

    /// Give every teammate the entries of `_base/harness-defaults.md` whose
    /// `base` matches its own, for every harness, in file order. The builder
    /// selects by harness at launch (`HarnessDefault::for_harness`). Run
    /// after every load, so an overlaid base or teammate is seen.
    fn attach_harness_defaults(&mut self) {
        let defaults = self
            .bases
            .get(HARNESS_DEFAULTS_BASE)
            .map(|b| b.defaults.clone())
            .unwrap_or_default();
        for t in self.teammates.values_mut() {
            t.harness_defaults = defaults
                .iter()
                .filter(|d| d.matches_base(t.base.as_deref()))
                .cloned()
                .collect();
        }
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
    ///
    /// A file that does not read or parse never fails the load: an overlay
    /// can be ahead of this binary (a new field value), and one such file
    /// must not take every other teammate down. The file is recorded instead,
    /// the earlier definition of its name stays, and [`Roster::load_warnings`]
    /// and [`Roster::check`] report it. Only an unreadable directory fails.
    pub fn overlay(&mut self, dir: &Path) -> Result<()> {
        let base_dir = dir.join("_base");
        if base_dir.is_dir() {
            for (stem, path) in md_files(&base_dir)? {
                match read_parsed(&path, |text| parse_base(&stem, text)) {
                    Ok(base) => {
                        self.bases.insert(stem.clone(), base);
                        self.broken_bases.remove(&stem);
                        self.base_origins.insert(stem, path);
                    }
                    Err(error) => {
                        let loaded = self.bases.contains_key(&stem);
                        let fallback = fallback_for(&self.base_origins, &stem, loaded);
                        self.broken_bases.insert(
                            stem,
                            BrokenFile {
                                path,
                                error,
                                fallback,
                            },
                        );
                    }
                }
            }
        }
        for (stem, path) in md_files(dir)? {
            if stem.starts_with('_') {
                continue;
            }
            match read_parsed(&path, |text| parse_teammate(&stem, text)) {
                Ok(t) => {
                    self.insert_parsed(stem.clone(), t);
                    self.broken_teammates.remove(&stem);
                    self.teammate_origins.insert(stem, path);
                }
                Err(error) => {
                    let loaded = self.teammates.contains_key(&stem);
                    let fallback = fallback_for(&self.teammate_origins, &stem, loaded);
                    self.broken_teammates.insert(
                        stem,
                        BrokenFile {
                            path,
                            error,
                            fallback,
                        },
                    );
                }
            }
        }
        self.attach_harness_defaults();
        Ok(())
    }

    /// One line per overlay file that did not load: the file, the error and
    /// what is used instead. Commands print these; `--check` fails on them.
    pub fn load_warnings(&self) -> Vec<String> {
        let bases = self
            .broken_bases
            .iter()
            .map(|(name, b)| b.warning("base", name));
        let teammates = self
            .broken_teammates
            .iter()
            .map(|(name, b)| b.warning("teammate", name));
        bases.chain(teammates).collect()
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
            None if self.broken_teammates.contains_key(name) => {
                bail!("{}", self.broken_teammates[name].warning("teammate", name))
            }
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

    /// Offer teammates by what `facts` says the project holds. The CLI
    /// gathers the facts; see `offer.rs`.
    pub fn with_project_facts(mut self, facts: ProjectFacts) -> Roster {
        self.project = Some(facts);
        self
    }

    /// The facts [`Roster::with_project_facts`] attached, if any.
    pub fn project_facts(&self) -> Option<&ProjectFacts> {
        self.project.as_ref()
    }

    /// `t` as it launches in this roster's project: its `skills_when`
    /// skills join `skills:` (`offer::with_project_skills`). Unchanged when
    /// no facts were attached. The spawn record and the launch bundle must
    /// both see this teammate, or the launch's skill check (SKL-04) fails.
    pub fn for_launch(&self, t: Teammate) -> Teammate {
        match &self.project {
            Some(facts) => with_project_skills(t, facts),
            None => t,
        }
    }

    /// What the orchestrator is offered: specialists first, then generics.
    /// Hidden teammates are excluded, and so is a teammate whose `offer_when`
    /// matches nothing in the project facts, when there are facts.
    pub fn offered(&self) -> Vec<&Teammate> {
        let mut v: Vec<&Teammate> = self
            .teammates
            .values()
            .filter(|t| offered_in(t, self.project.as_ref()))
            .collect();
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

    /// Base `context-windows`: the fleet windows and the in-place harnesses.
    pub fn context_windows(&self) -> Option<&Base> {
        self.bases.get(CONTEXT_WINDOWS_BASE)
    }

    /// Base `context-messages`: the lines horch types for the context watch.
    pub fn context_messages(&self) -> Option<&Base> {
        self.bases.get(CONTEXT_MESSAGES_BASE)
    }

    /// The fleet window of `t` running `model`, with its 1-line source:
    /// `t.compact_window`, else `windows["<harness>/<teammate>"]`, else
    /// `windows["<harness>/<model>"]`. None: the harness default.
    ///
    /// A substituted launch (`routing::decision::merge`) carries the
    /// fallback's `compact_window` under the original name; the detail then
    /// names the fallback, whose file sets it.
    pub fn fleet_window(&self, t: &Teammate, model: &str) -> Option<(u64, String)> {
        let empty = BTreeMap::new();
        let windows = self
            .context_windows()
            .and_then(|b| b.windows.as_ref())
            .unwrap_or(&empty);
        let found =
            window::fleet_window(windows, t.agent.as_str(), &t.name, model, t.compact_window);
        match (found, self.compact_window_owner(t)) {
            (Some((n, _)), Some(owner)) => Some((
                n,
                format!("teammate {owner} compact_window (fallback of {})", t.name),
            )),
            (found, _) => found,
        }
    }

    /// The fallback whose `compact_window` `t` carries, when `t` is a merged
    /// launch and its own file sets another value.
    fn compact_window_owner<'a>(&'a self, t: &'a Teammate) -> Option<&'a str> {
        let n = t.compact_window?;
        if self.get(&t.name)?.compact_window == Some(n) {
            return None;
        }
        t.fallbacks.iter().map(String::as_str).find(|f| {
            self.get(f)
                .is_some_and(|f| f.agent == t.agent && f.compact_window == Some(n))
        })
    }

    /// Whether horch compacts `harness` in place (CTX-17): only when
    /// `in_place` of `context-windows` names it and horch has a compact
    /// command for it.
    pub fn compacts_in_place(&self, harness: HarnessKind) -> bool {
        let listed = self
            .context_windows()
            .and_then(|b| b.in_place.as_ref())
            .is_some_and(|l| l.iter().any(|h| h == harness.as_str()));
        listed && harness.capabilities().compaction.has_command()
    }

    /// The overlay file base `name` came from. None: the built-in.
    pub(crate) fn base_origin(&self, name: &str) -> Option<&Path> {
        self.base_origins.get(name).map(PathBuf::as_path)
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
