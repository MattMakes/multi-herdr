//! `horch teammates` - inspect, validate, and scaffold the roster.
//!
//! The orchestrator reads the roster through its own briefing, not through this
//! command; this is for the human maintaining `teammates/`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use horch_core::harness::HarnessKind;
use horch_core::roster::{model_takes_effort, Teammate, TEMPLATE};
use horch_core::runtime::RuntimeContext;
use horch_core::usage;
use serde::Serialize;

use crate::output;

/// Human-readable table: what the orchestrator can pick, and what it cannot.
pub fn list(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let roster = super::load_roster(ctx, None)?;
    if json {
        let all: Vec<_> = roster
            .names()
            .iter()
            .map(|n| roster.get(n).unwrap())
            .collect();
        output::println(&serde_json::to_string_pretty(&all)?);
        return Ok(());
    }

    let mut out = String::new();
    if roster.sources.is_empty() {
        out.push_str("Roster: compiled-in only\n\n");
    } else {
        let dirs: Vec<String> = roster
            .sources
            .iter()
            .map(|d| d.display().to_string())
            .collect();
        out.push_str(&format!(
            "Roster: compiled-in, overlaid by {}\n\n",
            dirs.join(", ")
        ));
    }

    let offered = roster.offered();
    // Width spans every printed name, hidden ones included, so the two
    // sections stay in one column.
    let width = roster
        .names()
        .iter()
        .map(|n| n.len())
        .max()
        .unwrap_or(0)
        .max(4);
    // Agent and model names vary a lot across five harnesses - `claude`/`opus`
    // next to `opencode`/`opencode/nemotron-3.5-lightning-free` - so both
    // columns are measured rather than guessed, or the descriptions stop
    // lining up exactly when the roster gets interesting.
    let agent_width = offered
        .iter()
        .map(|t| t.agent.as_str().len())
        .max()
        .unwrap_or(0)
        .max(5);
    let model_width = offered
        .iter()
        .map(|t| t.model.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(0)
        .max(5);
    out.push_str("OFFERED TO THE ORCHESTRATOR\n");
    let (specialists, generics): (Vec<_>, Vec<_>) = offered.iter().partition(|t| !t.generic);
    let groups: [(&str, Vec<&&horch_core::roster::Teammate>); 2] = [
        ("specialists", specialists),
        ("generic fallbacks", generics),
    ];
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
            // Only for a teammate that sets it, so every other line stays as it was.
            if !t.offer_when.is_empty() {
                out.push_str(&format!(
                    "    {:<width$}  offered when the project has: {}\n",
                    "",
                    t.offer_when.join(", "),
                    width = width
                ));
            }
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
            out.push_str(&format!(
                "    {:<width$}  {}\n",
                t.name,
                t.brief_description,
                width = width
            ));
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
    /// Offered by name only (`available_skills`).
    pub available_skills: Vec<String>,
    /// Expected skills copied from the operator's machine, as `<dir>/<name>`.
    pub operator_skills: Vec<String>,
    /// Name globs that gate the offer; empty means always offered.
    pub offer_when: Vec<String>,
    /// Host tools `horch doctor` checks.
    pub requires: Vec<String>,
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
        None if t.agent == HarnessKind::Codex => "inherits config.toml".into(),
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
        available_skills: t.available_skills.clone(),
        operator_skills: t
            .operator_skills
            .iter()
            .flat_map(|o| {
                let dir = o.dir.trim_end_matches('/');
                o.names.iter().map(move |n| format!("{dir}/{n}"))
            })
            .collect(),
        offer_when: t.offer_when.clone(),
        requires: t.requires.iter().map(|r| r.as_str().to_string()).collect(),
        offered: !t.hidden,
        generic: t.generic,
        trains_on_input: t.trains_on_input,
        price_in: price.map(|p| p.input),
        price_out: price.map(|p| p.output),
        model,
    }
}

/// The roster as a tuning table.
pub fn matrix(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let roster = super::load_roster(ctx, None)?;
    let rows: Vec<MatrixRow> = roster
        .names()
        .iter()
        .filter_map(|n| roster.get(n))
        .filter(|t| t.agent != HarnessKind::None)
        .map(matrix_row)
        .collect();
    if json {
        output::println(&serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    output::print(&matrix_table(&rows));
    Ok(())
}

/// The markdown table of `horch teammates --matrix`.
fn matrix_table(rows: &[MatrixRow]) -> String {
    let list = |items: &[String]| {
        if items.is_empty() {
            "-".to_string()
        } else {
            items.join(", ")
        }
    };
    let mut out = String::from(
        "| teammate | agent | model | effort | phase | expected skills | available skills | $/MTok in/out | notes |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let mut notes: Vec<String> = Vec::new();
        if !r.offered {
            notes.push("hidden".into());
        }
        if r.generic {
            notes.push("generic".into());
        }
        if r.trains_on_input {
            notes.push("trains on input".into());
        }
        if !r.effort_is_explicit && r.effort == "inherits config.toml" {
            notes.push("effort unset".into());
        }
        if !r.offer_when.is_empty() {
            notes.push(format!("offer when {}", r.offer_when.join(" or ")));
        }
        if !r.requires.is_empty() {
            notes.push(format!("requires {}", r.requires.join(", ")));
        }
        let mut skills = r.skills.clone();
        skills.extend(r.plugin_skills.iter().cloned());
        skills.extend(r.operator_skills.iter().map(|s| format!("operator:{s}")));
        let price = match (r.price_in, r.price_out) {
            (Some(i), Some(o)) => format!("{i} / {o}"),
            _ => "unknown".into(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.name,
            r.agent,
            r.model,
            r.effort,
            r.phase,
            list(&skills),
            list(&r.available_skills),
            price,
            notes.join(", "),
        ));
    }
    out
}

/// Validate the roster. Exit code is what CI and `horch doctor` care about.
pub fn check(ctx: &RuntimeContext) -> Result<ExitCode> {
    let roster = super::load_roster_unwarned(ctx, None)?;
    let problems = roster.check();
    // Warnings (design 13.3 rule 6) never fail the check.
    for w in horch_core::roster::validation::fallback_warnings(&roster) {
        eprintln!("warning: {w}");
    }
    for w in horch_core::roster::validation::operator_skill_warnings(&roster) {
        eprintln!("warning: {w}");
    }
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
pub fn new(ctx: &RuntimeContext, name: &str, dir: Option<&str>) -> Result<()> {
    if name.starts_with('_') {
        bail!("a leading '_' means 'not parsed as a teammate'; pick another name");
    }
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => match &ctx.bins.roster_override {
            Some(d) => d.clone(),
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
    use horch_core::roster::{Roster, Teammate, TEMPLATE};

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
        // `fallbacks`, `offer_when`, `skills_when` and `requires` are skipped
        // when empty, so give each one entry to be listed.
        let actual = serde_yaml::to_value(Teammate {
            fallbacks: vec![String::new()],
            offer_when: vec![String::new()],
            skills_when: [(String::new(), vec![])].into(),
            requires: vec![horch_core::roster::Requirement::Xcode],
            ..Teammate::default()
        })
        .unwrap();
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
        assert_eq!(
            (backend.effort.as_str(), backend.effort_is_explicit),
            ("medium", true)
        );
        assert_eq!(
            (backend.price_in, backend.price_out),
            (Some(4.0), Some(20.0))
        );
        assert_eq!(row("codex-sol").effort, "medium");
        assert_eq!(row("opencode-pickle").effort, "n/a");
        assert_eq!(row("opencode-pickle").price_in, Some(0.0));
        assert_eq!(row("pi").effort, "low");
        assert_eq!(row("prime").model, "anthropic/claude-opus-5-5");
        assert_eq!(row("prime").price_in, Some(4.0));
        for name in roster.names() {
            let t = roster.get(name).unwrap();
            if t.agent == horch_core::harness::HarnessKind::Codex && !t.hidden {
                assert!(row(name).effort_is_explicit, "{name}");
            }
        }
    }

    /// The skill fields and the offer gate reach both the JSON row and the
    /// table.
    #[test]
    fn the_matrix_shows_skill_fields_and_the_offer_gate() {
        let t = Teammate {
            name: "ue-dev".into(),
            agent: horch_core::harness::HarnessKind::Claude,
            skills: vec!["unreal-cpp".into()],
            available_skills: vec!["unreal-niagara".into()],
            operator_skills: Some(horch_core::roster::OperatorSkills {
                dir: "~/.xcode-skills/".into(),
                names: vec!["swiftui-expert".into()],
            }),
            offer_when: vec!["*.uproject".into(), "*.xcodeproj".into()],
            requires: vec![horch_core::roster::Requirement::Xcode],
            ..Teammate::default()
        };
        let row = super::matrix_row(&t);
        assert_eq!(row.available_skills, ["unreal-niagara"]);
        assert_eq!(row.operator_skills, ["~/.xcode-skills/swiftui-expert"]);
        assert_eq!(row.offer_when, ["*.uproject", "*.xcodeproj"]);
        assert_eq!(row.requires, ["xcode"]);
        let json = serde_json::to_value(&row).unwrap();
        for key in [
            "available_skills",
            "operator_skills",
            "offer_when",
            "requires",
        ] {
            assert!(json.get(key).is_some(), "{key}");
        }
        let table = super::matrix_table(&[row]);
        let line = table.lines().nth(2).unwrap();
        assert!(
            line.contains(
                "| unreal-cpp, operator:~/.xcode-skills/swiftui-expert | unreal-niagara |"
            ),
            "{line}"
        );
        assert!(
            line.contains("offer when *.uproject or *.xcodeproj, requires xcode"),
            "{line}"
        );
        let header = table.lines().next().unwrap();
        assert_eq!(header.matches('|').count(), line.matches('|').count());
    }

    /// The template must survive the same parser real teammates go through,
    /// including `deny_unknown_fields`.
    #[test]
    fn the_template_scaffolds_a_loadable_teammate() {
        let text = TEMPLATE.replace("name: my-teammate", "name: scratch");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("scratch.md"), text).unwrap();
        let mut roster = Roster::builtin().unwrap();
        roster
            .overlay(dir.path())
            .expect("template must load as a teammate");
        assert_eq!(roster.require("scratch").unwrap().name, "scratch");
    }
}
