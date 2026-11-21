use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

use crate::{Manifest, MANIFEST_FILE};

pub const CHANGES_FILE: &str = "CHANGES.md";

pub const VERSIONS_FILE: &str = "versions.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Term {
    Flag,
    EnvVar,
    SettingsKey,
}

const TERMS: [(Term, &str); 3] = [
    (Term::Flag, "Flags"),
    (Term::EnvVar, "Env vars"),
    (Term::SettingsKey, "Settings keys"),
];

fn classify(span: &str) -> Option<(Term, &str)> {
    if span.starts_with('-') {
        let word = span.split_whitespace().next()?;
        return (word.len() > 1).then_some((Term::Flag, word));
    }
    if span.contains('_')
        && span
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Some((Term::EnvVar, span));
    }
    if span.contains(['.', ':']) && !span.contains(char::is_whitespace) {
        return Some((Term::SettingsKey, span));
    }
    None
}

fn code_spans(text: &str) -> impl Iterator<Item = &str> {
    text.lines().flat_map(|line| {
        let parts: Vec<&str> = line.split('`').collect();
        let closed = parts.len().saturating_sub(1);
        (1..closed)
            .step_by(2)
            .map(move |i| parts[i].trim())
            .filter(|span| !span.is_empty())
            .collect::<Vec<_>>()
    })
}

pub(crate) fn write(tool_dir: &Path, tool: &str, version: &str) -> Result<()> {
    let text = match record_version(tool_dir, version)? {
        None => format!(
            "# {tool} {version}\n\nThis is the first sync of {tool}. \
             No previous version exists to compare.\n"
        ),
        Some(previous) => compare(tool_dir, tool, &previous, version)?,
    };
    let path = tool_dir.join(CHANGES_FILE);
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

fn record_version(tool_dir: &Path, version: &str) -> Result<Option<String>> {
    let path = tool_dir.join(VERSIONS_FILE);
    let mut versions: Vec<String> = match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let at = match versions.iter().position(|v| v == version) {
        Some(at) => at,
        None => {
            versions.push(version.to_string());
            std::fs::write(&path, serde_json::to_string_pretty(&versions)? + "\n")
                .with_context(|| format!("writing {}", path.display()))?;
            versions.len() - 1
        }
    };
    Ok(at.checked_sub(1).map(|i| versions[i].clone()))
}

fn compare(tool_dir: &Path, tool: &str, previous: &str, current: &str) -> Result<String> {
    let old = manifest(tool_dir, previous)?;
    let new = manifest(tool_dir, current)?;

    let mut pages = Vec::new();
    for (file, hash) in &new.pages {
        match old.pages.get(file) {
            None => pages.push(format!("- added: `{file}`")),
            Some(before) if before != hash => pages.push(format!("- updated: `{file}`")),
            Some(_) => {}
        }
    }
    for file in old.pages.keys().filter(|f| !new.pages.contains_key(*f)) {
        pages.push(format!("- removed: `{file}`"));
    }

    let mut text = format!("# {tool} {current}\n\nChanges against {previous}.\n");
    push_section(&mut text, "Pages", &pages);
    let old_terms = terms(&tool_dir.join(previous), &old)?;
    let new_terms = terms(&tool_dir.join(current), &new)?;
    for (term, title) in TERMS {
        let added = new_terms[&term].difference(&old_terms[&term]);
        let removed = old_terms[&term].difference(&new_terms[&term]);
        let lines: Vec<String> = added
            .map(|t| format!("- added: `{t}`"))
            .chain(removed.map(|t| format!("- removed: `{t}`")))
            .collect();
        push_section(&mut text, title, &lines);
    }
    Ok(text)
}

fn push_section(text: &mut String, title: &str, lines: &[String]) {
    let _ = write!(text, "\n## {title}\n\n");
    if lines.is_empty() {
        text.push_str("No change.\n");
    }
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
}

fn manifest(tool_dir: &Path, version: &str) -> Result<Manifest> {
    let path = tool_dir.join(version).join(MANIFEST_FILE);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

fn terms(dir: &Path, manifest: &Manifest) -> Result<BTreeMap<Term, BTreeSet<String>>> {
    let mut found: BTreeMap<Term, BTreeSet<String>> =
        TERMS.iter().map(|(t, _)| (*t, BTreeSet::new())).collect();
    for file in manifest.pages.keys() {
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        for (term, name) in code_spans(&text).filter_map(classify) {
            found.entry(term).or_default().insert(name.to_string());
        }
    }
    Ok(found)
}
