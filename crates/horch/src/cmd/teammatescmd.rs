//! `horch teammates` - inspect, validate, and scaffold the roster.
//!
//! The orchestrator reads the roster through its own briefing, not through this
//! command; this is for the human maintaining `teammates/`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use horch_core::teammates::{Roster, TEMPLATE};

use crate::output;

/// Human-readable table: what the orchestrator can pick, and what it cannot.
pub fn list(json: bool) -> Result<()> {
    let roster = Roster::load()?;
    if json {
        let all: Vec<_> = roster.names().iter().map(|n| roster.get(n).unwrap()).collect();
        output::println(&serde_json::to_string_pretty(&all)?);
        return Ok(());
    }

    let mut out = String::new();
    if roster.sources.is_empty() {
        out.push_str("Roster: compiled-in only\n\n");
    } else {
        let dirs: Vec<String> = roster.sources.iter().map(|d| d.display().to_string()).collect();
        out.push_str(&format!("Roster: compiled-in, overlaid by {}\n\n", dirs.join(", ")));
    }

    let offered = roster.offered();
    // Width spans every printed name, hidden ones included, so the two
    // sections stay in one column.
    let width = roster.names().iter().map(|n| n.len()).max().unwrap_or(0).max(4);
    out.push_str("OFFERED TO THE ORCHESTRATOR\n");
    let (specialists, generics): (Vec<_>, Vec<_>) =
        offered.iter().partition(|t| !t.generic);
    let groups: [(&str, Vec<&&horch_core::teammates::Teammate>); 2] =
        [("specialists", specialists), ("generic fallbacks", generics)];
    for (label, group) in groups {
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("  {label}:\n"));
        for t in group {
            out.push_str(&format!(
                "    {:<width$}  {:<6} {:<14} {}\n",
                t.name,
                t.agent.as_str(),
                t.model.as_deref().unwrap_or("-"),
                t.brief_description,
                width = width
            ));
        }
    }

    let hidden: Vec<_> = roster
        .names()
        .into_iter()
        .filter_map(|n| roster.get(n))
        .filter(|t| t.hidden)
        .collect();
    if !hidden.is_empty() {
        out.push_str("\nHIDDEN (spawnable by name, never in the orchestrator's context)\n");
        for t in hidden {
            out.push_str(&format!("    {:<width$}  {}\n", t.name, t.brief_description, width = width));
        }
    }
    output::print(&out);
    Ok(())
}

/// Validate the roster. Exit code is what CI and `horch doctor` care about.
pub fn check() -> Result<ExitCode> {
    let roster = Roster::load()?;
    let problems = roster.check();
    if problems.is_empty() {
        output::println(&format!(
            "roster ok: {} teammates, {} offered to the orchestrator",
            roster.names().len(),
            roster.offered().len()
        ));
        return Ok(ExitCode::SUCCESS);
    }
    let mut out = String::new();
    for p in &problems {
        out.push_str(&format!("  {p}\n"));
    }
    eprint!("{out}");
    eprintln!("{} problem(s) in the roster", problems.len());
    Ok(ExitCode::FAILURE)
}

/// Scaffold a new teammate from `_template.md`.
///
/// The template is the schema's documentation, so it is also what a new file is
/// cut from - there is no second, drifting copy of the field list in here.
pub fn new(name: &str, dir: Option<&str>) -> Result<()> {
    if name.starts_with('_') {
        bail!("a leading '_' means 'not parsed as a teammate'; pick another name");
    }
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => match std::env::var_os("HORCH_TEAMMATES_DIR") {
            Some(d) => PathBuf::from(d),
            None => bail!(
                "no target directory: pass --dir, or set HORCH_TEAMMATES_DIR to the \
                 teammates/ folder you want to add to"
            ),
        },
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{name}.md"));
    if path.exists() {
        bail!("{} already exists", path.display());
    }

    // The only edit: `name:` must match the filename, which is the id.
    let body = TEMPLATE.replace("name: my-teammate", &format!("name: {name}"));
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    output::println(&path.display().to_string());
    eprintln!("Edit brief_description first - it is the only field the orchestrator reads.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use horch_core::teammates::{Roster, Teammate, TEMPLATE};

    /// The template is the schema's documentation. If a field is added to
    /// `Teammate` without being documented (or a documented key is removed),
    /// this fails - which is what keeps `_template.md` from becoming a stale
    /// description of a struct that has moved on.
    #[test]
    fn the_template_documents_exactly_the_teammate_fields() {
        let front = TEMPLATE
            .strip_prefix("---\n")
            .unwrap()
            .split("\n---\n")
            .next()
            .unwrap();
        let documented: serde_yaml::Mapping = serde_yaml::from_str(front).unwrap();
        let actual = serde_yaml::to_value(Teammate::default()).unwrap();
        let actual = actual.as_mapping().unwrap();

        let mut documented: Vec<String> = documented
            .keys()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
        let mut actual: Vec<String> = actual
            .keys()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
        documented.sort();
        actual.sort();
        assert_eq!(
            documented, actual,
            "_template.md and struct Teammate disagree about the field list"
        );
    }

    /// The template must survive the same parser real teammates go through,
    /// including `deny_unknown_fields`.
    #[test]
    fn the_template_scaffolds_a_loadable_teammate() {
        let text = TEMPLATE.replace("name: my-teammate", "name: scratch");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("scratch.md"), text).unwrap();
        let mut roster = Roster::builtin().unwrap();
        roster.overlay(dir.path()).expect("template must load as a teammate");
        assert_eq!(roster.require("scratch").unwrap().name, "scratch");
    }
}
