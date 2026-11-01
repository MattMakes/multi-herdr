//! The operator's reply table: parsed by code, never by a model (#308).
//!
//! A reply is the markdown table that [`template`] prints, with `Decision`
//! filled in. [`parse`] matches each row to a triage row by ID, checks that
//! the `Worktree` cell still names it, and validates every value. Any bad
//! line refuses the whole reply, so no decision runs from a typo.

use std::collections::HashSet;
use std::fmt;

use super::Row;
use crate::taskledger::model::TriageDecision;

/// 1 valid reply row.
#[derive(Debug, Clone, PartialEq)]
pub struct Decided {
    /// The row's ID, `W<n>`.
    pub id: String,
    pub name: String,
    pub decision: TriageDecision,
    pub direction: Option<String>,
    /// The 1-based line of the reply.
    pub line: usize,
}

/// Every bad line of a reply.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyError {
    pub problems: Vec<String>,
}

impl fmt::Display for ReplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "the reply was refused; no decision ran:")?;
        for p in &self.problems {
            writeln!(f, "  {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ReplyError {}

const COLUMNS: &str = "| ID | Worktree | Suggested | Decision | Direction |";

/// Whether a decision needs a `Direction`.
fn needs_direction(d: TriageDecision) -> bool {
    matches!(d, TriageDecision::Finish | TriageDecision::Pr)
}

/// Whether a decision needs the row to have a branch.
fn needs_branch(d: TriageDecision) -> bool {
    matches!(
        d,
        TriageDecision::Land | TriageDecision::Pr | TriageDecision::Finish
    )
}

/// The decision words, as the errors list them.
fn words(pr_offered: bool) -> String {
    TriageDecision::ALL
        .iter()
        .filter(|d| pr_offered || **d != TriageDecision::Pr)
        .map(|d| d.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The cells of a table line, without the outer `|`.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// Whether the cells are the header or the separator line.
fn is_header(cells: &[String]) -> bool {
    cells[0].eq_ignore_ascii_case("id")
        || cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

/// The table rows of a reply: (1-based line, cells).
fn table_lines(text: &str) -> Vec<(usize, Vec<String>)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.trim_start().starts_with('|'))
        .map(|(i, l)| (i + 1, cells(l)))
        .filter(|(_, c)| !is_header(c))
        .collect()
}

/// Check 1 reply line against the triage rows.
fn check_line(n: usize, c: &[String], rows: &[Row], pr_offered: bool) -> Result<Decided, String> {
    if c.len() < 4 {
        return Err(format!("line {n}: expected {COLUMNS}"));
    }
    let id = c[0].to_ascii_uppercase();
    let row = rows.iter().find(|r| r.id == id).ok_or_else(|| {
        format!(
            "line {n}: unknown ID '{}'; this triage has W1 to W{}",
            c[0],
            rows.len()
        )
    })?;
    if c[1] != row.name {
        return Err(format!(
            "line {n}: {id} is '{}', not '{}'; run horch worktree triage again",
            row.name, c[1]
        ));
    }
    let decision = decision_of(n, &id, &c[3], pr_offered)?;
    let direction = c[4..].join("|").trim().to_string();
    let direction = (!direction.is_empty()).then_some(direction);
    if needs_direction(decision) && direction.is_none() {
        return Err(format!("line {n}: {id} {decision} needs a Direction"));
    }
    if needs_branch(decision) && row.branch.is_none() {
        return Err(format!(
            "line {n}: {id} has no branch; choose ARCHIVE, DELETE or KEEP"
        ));
    }
    Ok(Decided {
        id,
        name: row.name.clone(),
        decision,
        direction,
        line: n,
    })
}

fn decision_of(n: usize, id: &str, cell: &str, pr_offered: bool) -> Result<TriageDecision, String> {
    if cell.is_empty() {
        return Err(format!(
            "line {n}: {id} has no Decision; write one of {}",
            words(pr_offered)
        ));
    }
    let d = TriageDecision::parse(&cell.to_ascii_uppercase()).map_err(|_| {
        format!(
            "line {n}: '{cell}' is not a decision; write one of {}",
            words(pr_offered)
        )
    })?;
    if d == TriageDecision::Pr && !pr_offered {
        return Err(format!(
            "line {n}: PR is not offered here: gh or a git remote is missing"
        ));
    }
    Ok(d)
}

/// Parse the operator's reply. `pr_offered` is whether `gh` and a remote
/// exist. Every problem is collected; any problem refuses the whole reply.
pub fn parse(text: &str, rows: &[Row], pr_offered: bool) -> Result<Vec<Decided>, ReplyError> {
    let mut problems = Vec::new();
    let mut decided: Vec<Decided> = Vec::new();
    let mut seen = HashSet::new();
    for (n, c) in table_lines(text) {
        match check_line(n, &c, rows, pr_offered) {
            Ok(d) if !seen.insert(d.id.clone()) => {
                problems.push(format!("line {n}: {} appears twice", d.id))
            }
            Ok(d) => decided.push(d),
            Err(p) => problems.push(p),
        }
    }
    if problems.is_empty() && decided.is_empty() {
        problems.push(format!("the reply has no rows; paste the table {COLUMNS}"));
    }
    if problems.is_empty() {
        Ok(decided)
    } else {
        Err(ReplyError { problems })
    }
}

/// The copyable reply table: 1 row per triage row, `Decision` empty.
pub fn template(rows: &[Row]) -> String {
    let width = rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max("Worktree".len());
    let mut out = format!(
        "| ID | {:width$} | Suggested | Decision | Direction |\n|----|{}|-----------|----------|-----------|\n",
        "Worktree",
        "-".repeat(width + 2)
    );
    for r in rows {
        out.push_str(&format!(
            "| {:<2} | {:width$} |           |          |           |\n",
            r.id, r.name
        ));
    }
    out
}
