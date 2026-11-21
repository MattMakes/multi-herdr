use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use horch_core::harness::HarnessKind;
use horch_core::roster::{model_takes_effort, Roster, Teammate, TEMPLATE};
use horch_core::runtime::RuntimeContext;
use horch_core::usage;
use serde::Serialize;

use crate::output;

pub fn list(ctx: &RuntimeContext, json: bool) -> Result<()> {
    let roster = super::load_roster(ctx, None)?;
    if json {
        let all: Vec<_> = roster
            .names()
            .iter()
            .map(|n| roster.get(n).unwrap())
            .collect();
        output::println(&serde_json::to_string_pretty(&all)?);
    } else {
        output::print(&roster_text(&roster));
    }
    Ok(())
}

fn roster_text(roster: &Roster) -> String {
    let mut out = roster_header(roster);
    let width = roster
        .names()
        .iter()
        .map(|n| n.len())
        .max()
        .unwrap_or(0)
        .max(4);
    offered_section(&mut out, roster, width);
    hidden_section(&mut out, roster, width);
    out
}

fn roster_header(roster: &Roster) -> String {
    if roster.sources.is_empty() {
        return "Roster: compiled-in only\n\n".to_string();
    }
    let dirs: Vec<String> = roster
        .sources
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    format!("Roster: compiled-in, overlaid by {}\n\n", dirs.join(", "))
}

fn offered_section(out: &mut String, roster: &Roster, width: usize) {
    let offered = roster.offered();
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
    let groups: [(&str, Vec<&&Teammate>); 2] = [
        ("specialists", specialists),
        ("generic fallbacks", generics),
    ];
    for (label, group) in groups.iter().filter(|(_, g)| !g.is_empty()) {
        out.push_str(&format!("  {label}:\n"));
        for t in group {
            out.push_str(&format!(
                "    {:<width$}  {:<agent_width$}  {:<model_width$}  {}\n",
                t.name,
                t.agent.as_str(),
                t.model.as_deref().unwrap_or("-"),
                t.brief_description,
            ));
            if !t.offer_when.is_empty() {
                out.push_str(&format!(
                    "    {:<width$}  offered when the project has: {}\n",
                    "",
                    t.offer_when.join(", "),
                ));
            }
        }
    }
}

fn hidden_section(out: &mut String, roster: &Roster, width: usize) {
    let hidden: Vec<_> = roster
        .names()
        .into_iter()
        .filter_map(|n| roster.get(n))
        .filter(|t| t.hidden)
        .collect();
    if hidden.is_empty() {
        return;
    }
    out.push_str("\nHIDDEN (never in the orchestrator's context)\n");
    for t in hidden {
        out.push_str(&format!(
            "    {:<width$}  {}\n",
            t.name, t.brief_description,
        ));
    }
}

#[derive(Debug, Serialize)]
pub struct MatrixRow {
    pub name: String,
    pub agent: String,
    pub model: String,
    pub effort: String,
    pub effort_is_explicit: bool,
    pub signoff: Option<String>,
    pub phase: String,
    pub skills: Vec<String>,
    pub plugin_skills: Vec<String>,
    pub available_skills: Vec<String>,
    pub operator_skills: Vec<String>,
    pub offer_when: Vec<String>,
    pub requires: Vec<String>,
    pub offered: bool,
    pub generic: bool,
    pub trains_on_input: bool,
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
        signoff: t.signoff.clone(),
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
        let mut skills = r.skills.clone();
        skills.extend(r.plugin_skills.iter().cloned());
        skills.extend(r.operator_skills.iter().map(|s| format!("operator:{s}")));
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.name,
            r.agent,
            r.model,
            r.effort,
            r.phase,
            list(&skills),
            list(&r.available_skills),
            price_cell(r),
            row_notes(r).join(", "),
        ));
    }
    out
}

fn price_cell(r: &MatrixRow) -> String {
    match (r.price_in, r.price_out) {
        (Some(i), Some(o)) => format!("{i} / {o}"),
        _ => "unknown".into(),
    }
}

fn row_notes(r: &MatrixRow) -> Vec<String> {
    let flags = [
        (!r.offered, "hidden".to_string()),
        (r.generic, "generic".to_string()),
        (r.trains_on_input, "trains on input".to_string()),
        (
            !r.effort_is_explicit && r.effort == "inherits config.toml",
            "effort unset".to_string(),
        ),
        (
            !r.offer_when.is_empty(),
            format!("offer when {}", r.offer_when.join(" or ")),
        ),
        (
            !r.requires.is_empty(),
            format!("requires {}", r.requires.join(", ")),
        ),
        (
            r.signoff.is_some(),
            format!("signoff: {}", r.signoff.as_deref().unwrap_or_default()),
        ),
    ];
    flags
        .into_iter()
        .filter_map(|(on, label)| on.then_some(label))
        .collect()
}

pub fn check(ctx: &RuntimeContext) -> Result<ExitCode> {
    let roster = super::load_roster_unwarned(ctx, None)?;
    let problems = roster.check();
    for w in horch_core::roster::validation::fallback_warnings(&roster) {
        eprintln!("warning: {w}");
    }
    for w in horch_core::roster::validation::operator_skill_warnings(&roster) {
        eprintln!("warning: {w}");
    }
    for w in horch_core::roster::validation::context_policy_warnings(&roster) {
        eprintln!("warning: {w}");
    }
    for w in horch_core::roster::validation::ceiling_warnings(&roster) {
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

    let body = TEMPLATE.replace("name: my-teammate", &format!("name: {name}"));
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    output::println(&path.display().to_string());
    eprintln!("Edit brief_description first - it is the only field the orchestrator reads.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use horch_core::roster::{Roster, Teammate, TEMPLATE};

    #[test]
    fn the_template_documents_exactly_the_teammate_fields() {
        let front = TEMPLATE
            .strip_prefix("---\n")
            .unwrap()
            .split("\n---\n")
            .next()
            .unwrap();
        let documented: serde_yaml::Mapping = serde_yaml::from_str(front).unwrap();
        let actual = serde_yaml::to_value(Teammate {
            fallbacks: vec![String::new()],
            signoff: Some(String::new()),
            system_prompt: Some(String::new()),
            compact_window: Some(1),
            compact_at: Some(1),
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

    #[test]
    fn the_matrix_notes_and_price_cells_render_exactly() {
        let row = |price: Option<(f64, f64)>| super::MatrixRow {
            name: "t".into(),
            agent: "codex".into(),
            model: "m".into(),
            effort: "inherits config.toml".into(),
            effort_is_explicit: false,
            signoff: None,
            phase: "-".into(),
            skills: vec![],
            plugin_skills: vec![],
            available_skills: vec![],
            operator_skills: vec![],
            offer_when: vec!["a".into(), "b".into()],
            requires: vec!["xcode".into(), "git".into()],
            offered: false,
            generic: true,
            trains_on_input: true,
            price_in: price.map(|p| p.0),
            price_out: price.map(|p| p.1),
        };
        let table = super::matrix_table(&[row(Some((1.5, 7.0))), row(None)]);
        let lines: Vec<&str> = table.lines().skip(2).collect();
        let notes = "hidden, generic, trains on input, effort unset, offer when a or b, requires xcode, git";
        assert_eq!(
            lines[0],
            format!("| t | codex | m | inherits config.toml | - | - | - | 1.5 / 7 | {notes} |")
        );
        assert_eq!(
            lines[1],
            format!("| t | codex | m | inherits config.toml | - | - | - | unknown | {notes} |")
        );
        let mut plain = row(None);
        plain.offered = true;
        plain.generic = false;
        plain.trains_on_input = false;
        plain.effort_is_explicit = true;
        plain.offer_when.clear();
        plain.requires.clear();
        let table = super::matrix_table(&[plain]);
        assert!(table.lines().nth(2).unwrap().ends_with("| unknown |  |"));
    }

    #[test]
    fn the_matrix_shows_a_signoff_in_the_notes_and_in_json() {
        let signoff = "2026-11-21 operator: proofs need the depth";
        let signed = Teammate {
            name: "t".into(),
            effort: Some("high".into()),
            signoff: Some(signoff.into()),
            hidden: true,
            ..Teammate::default()
        };
        let row = super::matrix_row(&signed);
        assert_eq!(
            serde_json::to_value(&row).unwrap()["signoff"],
            serde_json::json!(signoff)
        );
        let table = super::matrix_table(&[row]);
        let line = table.lines().nth(2).unwrap();
        assert!(
            line.ends_with(&format!("| hidden, signoff: {signoff} |")),
            "{line}"
        );

        let unsigned = super::matrix_row(&Teammate::default());
        assert!(serde_json::to_value(&unsigned).unwrap()["signoff"].is_null());
        assert!(!super::matrix_table(&[unsigned]).contains("signoff"));
    }

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
