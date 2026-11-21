use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::parser::{md_files, parse_base, parse_teammate};
use super::teammate::{CONTEXT_MESSAGES_BASE, CONTEXT_WINDOWS_BASE, HARNESS_DEFAULTS_BASE};
use super::{offered_in, with_project_skills, Base, ExecRule, ProjectFacts, Teammate};
use crate::compaction::window;
use crate::harness::HarnessKind;

include!(concat!(env!("OUT_DIR"), "/builtin_teammates.rs"));

#[derive(Debug, Clone, Default)]
pub struct Roster {
    pub(crate) teammates: BTreeMap<String, Teammate>,
    pub(crate) bases: BTreeMap<String, Base>,
    pub(crate) system_prompts: BTreeMap<String, String>,
    pub sources: Vec<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) skill_catalog: Option<crate::skills::SkillCatalog>,
    pub(crate) project: Option<ProjectFacts>,
    pub(crate) broken_teammates: BTreeMap<String, BrokenFile>,
    pub(crate) broken_bases: BTreeMap<String, BrokenFile>,
    teammate_origins: BTreeMap<String, PathBuf>,
    base_origins: BTreeMap<String, PathBuf>,
    pub(crate) defaults_set_in_file: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrokenFile {
    path: PathBuf,
    error: String,
    fallback: Option<String>,
}

impl BrokenFile {
    fn new(
        origins: &BTreeMap<String, PathBuf>,
        name: &str,
        loaded: bool,
        path: PathBuf,
        error: String,
    ) -> BrokenFile {
        BrokenFile {
            path,
            error,
            fallback: fallback_for(origins, name, loaded),
        }
    }

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
        for (name, text) in BUILTIN_SYSTEM_PROMPTS {
            r.system_prompts.insert(name.to_string(), text.to_string());
        }
        r.attach_harness_defaults();
        r.attach_system_prompts();
        Ok(r)
    }

    fn insert_parsed(&mut self, name: String, t: Teammate) {
        if t.harness_defaults.is_empty() {
            self.defaults_set_in_file.remove(&name);
        } else {
            self.defaults_set_in_file.insert(name.clone());
        }
        self.teammates.insert(name, t);
    }

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

    fn attach_system_prompts(&mut self) {
        for t in self.teammates.values_mut() {
            t.system_prompt_text = t
                .system_prompt
                .as_ref()
                .and_then(|name| self.system_prompts.get(name).cloned());
        }
    }

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

    pub fn overlay(&mut self, dir: &Path) -> Result<()> {
        let base_dir = dir.join("_base");
        if base_dir.is_dir() {
            self.overlay_bases(md_files(&base_dir)?);
        }
        let prompt_dir = dir.join("system_prompt");
        if prompt_dir.is_dir() {
            for (stem, path) in md_files(&prompt_dir)? {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                self.system_prompts.insert(stem, text);
            }
        }
        self.overlay_teammates(md_files(dir)?);
        self.attach_harness_defaults();
        self.attach_system_prompts();
        Ok(())
    }

    fn overlay_bases(&mut self, files: Vec<(String, PathBuf)>) {
        for (stem, path) in files {
            match read_parsed(&path, |text| parse_base(&stem, text)) {
                Ok(base) => {
                    self.bases.insert(stem.clone(), base);
                    self.broken_bases.remove(&stem);
                    self.base_origins.insert(stem, path);
                }
                Err(error) => {
                    let loaded = self.bases.contains_key(&stem);
                    let file = BrokenFile::new(&self.base_origins, &stem, loaded, path, error);
                    self.broken_bases.insert(stem, file);
                }
            }
        }
    }

    fn overlay_teammates(&mut self, files: Vec<(String, PathBuf)>) {
        for (stem, path) in files {
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
                    let file = BrokenFile::new(&self.teammate_origins, &stem, loaded, path, error);
                    self.broken_teammates.insert(stem, file);
                }
            }
        }
    }

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

    #[doc(hidden)]
    pub fn insert_for_test(&mut self, t: Teammate) {
        self.teammates.insert(t.name.clone(), t);
    }

    pub fn get(&self, name: &str) -> Option<&Teammate> {
        self.teammates.get(name)
    }

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

    pub fn names(&self) -> Vec<&str> {
        self.teammates.keys().map(|s| s.as_str()).collect()
    }

    pub fn with_project_facts(mut self, facts: ProjectFacts) -> Roster {
        self.project = Some(facts);
        self
    }

    pub fn project_facts(&self) -> Option<&ProjectFacts> {
        self.project.as_ref()
    }

    pub fn for_launch(&self, t: Teammate) -> Teammate {
        match &self.project {
            Some(facts) => with_project_skills(t, facts),
            None => t,
        }
    }

    pub fn offered(&self) -> Vec<&Teammate> {
        let mut v: Vec<&Teammate> = self
            .teammates
            .values()
            .filter(|t| offered_in(t, self.project.as_ref()))
            .collect();
        v.sort_by_key(|t| (t.generic, t.name.clone()));
        v
    }

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

    pub fn exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-execpolicy")
    }

    pub fn orchestrator_exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-orchestrator-execpolicy")
    }

    pub fn context_windows(&self) -> Option<&Base> {
        self.bases.get(CONTEXT_WINDOWS_BASE)
    }

    pub fn context_messages(&self) -> Option<&Base> {
        self.bases.get(CONTEXT_MESSAGES_BASE)
    }

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

    pub fn compacts_in_place(&self, harness: HarnessKind) -> bool {
        let listed = self
            .context_windows()
            .and_then(|b| b.in_place.as_ref())
            .is_some_and(|l| l.iter().any(|h| h == harness.as_str()));
        listed && harness.capabilities().compaction.has_command()
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_base(dir: &Path, stem: &str, text: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join("_base")).unwrap();
        let path = dir.join("_base").join(format!("{stem}.md"));
        std::fs::write(&path, text).unwrap();
        path
    }

    fn base_text(name: &str, body: &str) -> String {
        format!("---\nname: {name}\n---\n{body}\n")
    }

    const NO_FENCE: &str = "file does not start with a '---' frontmatter fence";

    #[test]
    fn a_good_base_replaces_the_builtin_and_names_its_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layer");
        let text = base_text("fleet-worker", "from the layer");
        let path = write_base(&dir, "fleet-worker", &text);

        let mut r = Roster::builtin().unwrap();
        assert_eq!(r.base_origin("fleet-worker"), None);
        r.overlay(&dir).unwrap();
        assert_eq!(r.base("fleet-worker").unwrap().body, "from the layer");
        assert_eq!(r.base_origin("fleet-worker"), Some(path.as_path()));
        assert!(r.load_warnings().is_empty(), "{:?}", r.load_warnings());
    }

    #[test]
    fn a_later_good_base_heals_an_earlier_broken_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (first, second) = (tmp.path().join("first"), tmp.path().join("second"));
        write_base(&first, "fleet-worker", "no frontmatter here\n");
        write_base(
            &second,
            "fleet-worker",
            &base_text("fleet-worker", "from the second layer"),
        );

        let mut r = Roster::builtin().unwrap();
        r.overlay(&first).unwrap();
        assert_eq!(r.load_warnings().len(), 1, "{:?}", r.load_warnings());
        r.overlay(&second).unwrap();
        assert_eq!(
            r.base("fleet-worker").unwrap().body,
            "from the second layer"
        );
        assert!(r.load_warnings().is_empty(), "{:?}", r.load_warnings());
    }

    #[test]
    fn a_broken_base_says_what_is_used_instead() {
        let tmp = tempfile::tempdir().unwrap();
        let (first, second, third) = (
            tmp.path().join("first"),
            tmp.path().join("second"),
            tmp.path().join("third"),
        );
        let good = write_base(
            &first,
            "fleet-worker",
            &base_text("fleet-worker", "from the first layer"),
        );
        let wrong_name = write_base(&second, "fleet-worker", &base_text("other", "x"));
        let builtin = write_base(&second, "channel", "no frontmatter here\n");
        let unknown = write_base(&third, "no-such-base", "no frontmatter here\n");

        let mut r = Roster::builtin().unwrap();
        let channel = r.base("channel").unwrap().body.clone();
        r.overlay(&first).unwrap();
        r.overlay(&second).unwrap();
        r.overlay(&third).unwrap();

        assert_eq!(r.base("fleet-worker").unwrap().body, "from the first layer");
        assert_eq!(r.base("channel").unwrap().body, channel);
        assert!(r.base("no-such-base").is_none());
        assert_eq!(
            r.load_warnings(),
            vec![
                format!(
                    "base 'channel' did not load from {}: {NO_FENCE}; using the built-in 'channel' instead",
                    builtin.display()
                ),
                format!(
                    "base 'fleet-worker' did not load from {}: name is 'other' but the filename says 'fleet-worker'; using the earlier {} instead",
                    wrong_name.display(),
                    good.display()
                ),
                format!(
                    "base 'no-such-base' did not load from {}: {NO_FENCE}; base 'no-such-base' is not available",
                    unknown.display()
                ),
            ]
        );
    }

    #[test]
    fn a_file_that_starts_with_an_underscore_is_no_teammate() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("layer");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_scratch.md"), "not a teammate\n").unwrap();

        let mut r = Roster::builtin().unwrap();
        let before = r.names().len();
        r.overlay(&dir).unwrap();
        assert_eq!(r.names().len(), before);
        assert!(r.load_warnings().is_empty(), "{:?}", r.load_warnings());
    }

    #[test]
    fn overlaying_a_directory_that_cannot_be_read_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let error = Roster::builtin().unwrap().overlay(&missing).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.starts_with(&format!("reading {}: ", missing.display())),
            "{message}"
        );
    }
}
