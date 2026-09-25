//! `horch teammates` - inspect, validate, and scaffold the roster.
//!
//! The orchestrator reads the roster through its own briefing, not through this
//! command; this is for the human maintaining `teammates/`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use horch_core::teammates::{model_takes_effort, Agent, Roster, Teammate, TEMPLATE};
use horch_core::usage;
use serde::Serialize;

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
    // Agent and model names vary a lot across five harnesses - `claude`/`opus`
    // next to `opencode`/`opencode/nemotron-3.5-lightning-free` - so both
    // columns are measured rather than guessed, or the descriptions stop
    // lining up exactly when the roster gets interesting.
    let agent_width = offered.iter().map(|t| t.agent.as_str().len()).max().unwrap_or(0).max(5);
    let model_width = offered
        .iter()
        .map(|t| t.model.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(0)
        .max(5);
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
                "    {:<width$}  {:<agent_width$}  {:<model_width$}  {}\n",
                t.name,
                t.agent.as_str(),
                t.model.as_deref().unwrap_or("-"),
                t.brief_description,
                width = width,
                agent_width = agent_width,
                model_width = model_width
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
        // Not "spawnable by name": the orchestrators are hidden AND reserved,
        // launched into a pane by `horch fleet` rather than spawned into one.
        out.push_str("\nHIDDEN (never in the orchestrator's context)\n");
        for t in hidden {
            out.push_str(&format!("    {:<width$}  {}\n", t.name, t.brief_description, width = width));
        }
    }
    output::print(&out);
    Ok(())
}

/// One teammate's tuning, as `horch teammates --matrix` shows it.
#[derive(Debug, Serialize)]
pub struct MatrixRow {
    pub name: String,
    pub agent: String,
    pub model: String,
    /// The level passed, or what the pane falls back to when none is.
    pub effort: String,
    pub effort_is_explicit: bool,
    pub phase: String,
    pub skills: Vec<String>,
    pub plugin_skills: Vec<String>,
    pub offered: bool,
    pub generic: bool,
    pub trains_on_input: bool,
    /// USD per MTok, input / output; `None` when the price table does not
    /// know the model.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
}

pub fn matrix_row(t: &Teammate) -> MatrixRow {
    let model = t.model.clone().unwrap_or_else(|| "-".into());
    let effort = match &t.effort {
        Some(e) => e.clone(),
        None if !model_takes_effort(t.agent, &model) => "n/a".into(),
        None if t.agent == Agent::Codex => "inherits config.toml".into(),
        None => "agent default".into(),
    };
    let price = usage::price_for(&usage::builtin_prices(), &model);
    MatrixRow {
        name: t.name.clone(),
        agent: t.agent.to_string(),
        effort_is_explicit: t.effort.is_some(),
        effort,
        phase: t.phase.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
        skills: t.skills.clone(),
        plugin_skills: t
            .plugin_skills
            .iter()
            .flat_map(|(p, skills)| skills.iter().map(move |s| format!("{p}:{s}")))
            .collect(),
        offered: !t.hidden,
        generic: t.generic,
        trains_on_input: t.trains_on_input,
        price_in: price.map(|p| p.input),
        price_out: price.map(|p| p.output),
        model,
    }
}

/// The roster as a tuning table.
pub fn matrix(json: bool) -> Result<()> {
    let roster = Roster::load()?;
    let rows: Vec<MatrixRow> = roster
        .names()
        .iter()
        .filter_map(|n| roster.get(n))
        .filter(|t| t.agent != Agent::None)
        .map(matrix_row)
        .collect();
    if json {
        output::println(&serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let mut out = String::from(
        "| teammate | agent | model | effort | phase | expected skills | $/MTok in/out | notes |\n\
         |---|---|---|---|---|---|---|---|\n",
    );
    for r in &rows {
        let mut notes = Vec::new();
        if !r.offered {
            notes.push("hidden");
        }
        if r.generic {
            notes.push("generic");
        }
        if r.trains_on_input {
            notes.push("trains on input");
        }
        if !r.effort_is_explicit && r.effort == "inherits config.toml" {
            notes.push("effort unset");
        }
        let mut skills = r.skills.clone();
        skills.extend(r.plugin_skills.iter().cloned());
        let price = match (r.price_in, r.price_out) {
            (Some(i), Some(o)) => format!("{i} / {o}"),
            _ => "unknown".into(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.name,
            r.agent,
            r.model,
            r.effort,
            r.phase,
            if skills.is_empty() { "-".into() } else { skills.join(", ") },
            price,
            notes.join(", "),
        ));
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

    /// The shipped roster states every effort it can, and prices every model
    /// the price table covers.
    #[test]
    fn the_matrix_shows_explicit_effort_and_known_prices() {
        let roster = Roster::builtin().unwrap();
        let row = |name: &str| super::matrix_row(roster.require(name).unwrap());
        let backend = row("backend-developer");
        assert_eq!((backend.effort.as_str(), backend.effort_is_explicit), ("medium", true));
        assert_eq!((backend.price_in, backend.price_out), (Some(4.0), Some(20.0)));
        assert_eq!(row("codex-sol").effort, "medium");
        assert_eq!(row("opencode-pickle").effort, "n/a");
        assert_eq!(row("opencode-pickle").price_in, Some(0.0));
        assert_eq!(row("pi").effort, "low");
        assert_eq!(row("prime").model, "anthropic/claude-opus-5-5");
        assert_eq!(row("prime").price_in, Some(4.0));
        for name in roster.names() {
            let t = roster.get(name).unwrap();
            if t.agent == horch_core::teammates::Agent::Codex && !t.hidden {
                assert!(row(name).effort_is_explicit, "{name}");
            }
        }
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
