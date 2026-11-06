//! Reading a ticket map (`MAP.md`) for `horch ledger add-map` (section 5.5a).
//!
//! A map is a markdown table with 1 row per ticket. horch reads these
//! columns by header name, ignoring case: `#` (the ticket key), `Title`,
//! `Blocked by`, `Wave`, `Issue` (optional), and the first link in the row
//! that points at a ticket file. Each ticket file names its persona:
//!
//! ```text
//! **Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
//! **Check:** `cargo test -p horch-core`
//! **Own:** `src/a.rs`, `src/b/`
//! **Do not touch:** `src/c.rs`
//! ```
//!
//! A `Persona` or `Parallel` column in the map wins over the ticket's line.
//! `Blocked by` holds ticket keys (`01`, `1`) or issue numbers (`#30`) of
//! rows in the same map, or `T-<n>` tasks already in the run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::model::{ReviewMode, TaskKind, WorkKind, WorktreeMode};
use super::tasks::NewTask;

/// 1 ticket of a map, ready to queue.
#[derive(Debug, Clone)]
pub struct MapItem {
    pub key: String,
    pub task: NewTask,
}

/// A parsed map.
#[derive(Debug, Clone)]
pub struct Map {
    pub spec_ref: Option<String>,
    pub items: Vec<MapItem>,
}

fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if !t.starts_with('|') {
        return None;
    }
    let inner = t.trim_start_matches('|').trim_end_matches('|');
    Some(inner.split('|').map(|c| c.trim().to_string()).collect())
}

fn is_rule(cells: &[String]) -> bool {
    cells
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

/// Every `[text](target)` in `s`, as (text, target).
pub fn links(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else { break };
        let text = &after[..close];
        let tail = &after[close + 2..];
        let Some(end) = tail.find(')') else { break };
        out.push((text.to_string(), tail[..end].to_string()));
        rest = &tail[end + 1..];
    }
    out
}

/// Text with markdown links reduced to their text and backticks removed.
fn plain(s: &str) -> String {
    let mut out = s.to_string();
    for (text, target) in links(s) {
        out = out.replace(&format!("[{text}]({target})"), &text);
    }
    out.replace('`', "").trim().to_string()
}

/// A ticket key, normalised: `01` and `1` are the same key.
fn norm(key: &str) -> String {
    let k = key.trim().trim_start_matches('#');
    match k.parse::<u64>() {
        Ok(n) => n.to_string(),
        Err(_) => k.to_string(),
    }
}

/// The text with parenthesised titles dropped.
fn strip_parens(text: &str) -> String {
    let mut cleaned = String::new();
    let mut depth = 0;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => cleaned.push(ch),
            _ => {}
        }
    }
    cleaned
}

/// `T-<n>`: a task already in the run. `T-` with no number names nothing.
fn task_ref(tok: &str) -> Option<Vec<String>> {
    let t = tok.strip_prefix("T-").or(tok.strip_prefix("t-"))?;
    Some(if t.parse::<u64>().is_ok() {
        vec![format!("T-{t}")]
    } else {
        Vec::new()
    })
}

/// A key as written: `#<n>` for an issue number, else `<n>`.
fn keyed(n: impl std::fmt::Display, hash: bool) -> String {
    if hash {
        format!("#{n}")
    } else {
        n.to_string()
    }
}

/// A range like `#37-#40`, at most 1000 keys long.
fn number_range(tok: &str, hash: bool) -> Option<Vec<String>> {
    let (a, b) = tok.split_once('-')?;
    let a = a.trim_start_matches('#').parse::<u64>().ok()?;
    let b = b.trim_start_matches('#').parse::<u64>().ok()?;
    if a > b || b - a >= 1000 {
        return None;
    }
    Some((a..=b).map(|n| keyed(n, hash)).collect())
}

/// A single key: `01` or `#30`.
fn plain_number(tok: &str, hash: bool) -> Option<Vec<String>> {
    let digits = tok.trim_start_matches('#');
    digits.parse::<u64>().ok()?;
    Some(vec![keyed(digits, hash)])
}

/// The keys 1 token names; none when it names nothing, as `-` and `none`.
fn expand_token(tok: &str) -> Vec<String> {
    let hash = tok.starts_with('#');
    task_ref(tok)
        .or_else(|| number_range(tok, hash))
        .or_else(|| plain_number(tok, hash))
        .unwrap_or_default()
}

/// The keys in a `Blocked by` cell: `01, 02`, `#30 (title)`, `T-3`, `-`, `None`.
fn blocker_tokens(cell: &str) -> Vec<String> {
    strip_parens(&plain(cell))
        .split([',', ';', ' '])
        .map(str::trim)
        .flat_map(expand_token)
        .collect()
}

/// The first backticked word after `word` in `line`.
fn ticked_after(line: &str, word: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let at = lower.find(word)?;
    let rest = &line[at + word.len()..];
    let open = rest.find('`')?;
    let rest = &rest[open + 1..];
    let close = rest.find('`')?;
    Some(rest[..close].trim().to_string()).filter(|s| !s.is_empty())
}

/// Every backticked item in `line` after the first `:`.
fn ticked_list(line: &str) -> Vec<String> {
    let rest = line.split_once(":**").map(|(_, r)| r).unwrap_or(line);
    let mut out = Vec::new();
    let mut parts = rest.split('`');
    parts.next();
    while let Some(item) = parts.next() {
        if !item.trim().is_empty() {
            out.push(item.trim().to_string());
        }
        parts.next();
    }
    out
}

/// What a ticket file says about itself.
#[derive(Debug, Clone, Default)]
pub struct TicketFacts {
    pub title: Option<String>,
    pub primary: Option<String>,
    pub fallback: Option<String>,
    pub parallel: Option<bool>,
    pub check: Option<String>,
    pub own: Vec<String>,
    pub no_touch: Vec<String>,
    /// The `<!-- horch-ticket: N -->` version; none for a ticket older than C1.
    pub version: Option<String>,
    pub kind: Option<WorkKind>,
    pub review: Option<ReviewMode>,
    /// `**Check:** none`: the ticket has no check on purpose.
    pub check_none: bool,
}

impl TicketFacts {
    /// The review mode: the explicit `Review`, else the default of `Kind`.
    pub fn review_mode(&self) -> Option<ReviewMode> {
        self.review.or(self.kind.map(default_review))
    }
}

/// The C1 ticket format version this horch reads.
pub const TICKET_VERSION: &str = "1";

/// The review a `Kind` gets when the ticket names none (C1).
pub fn default_review(kind: WorkKind) -> ReviewMode {
    match kind {
        WorkKind::Feature | WorkKind::Docs | WorkKind::Report => ReviewMode::NoReview,
        WorkKind::Quality | WorkKind::Refactor | WorkKind::Security => ReviewMode::Full,
    }
}

/// The version a `<!-- horch-ticket: N -->` line names.
fn version_of(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?;
    let v = inner.trim().strip_prefix("horch-ticket:")?.trim();
    Some(v.to_string())
}

/// The first word after `prefix` in a lowercased field line, without backticks.
fn field_word<'a>(lower: &'a str, prefix: &str) -> &'a str {
    lower[prefix.len()..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('`')
}

/// A heading's title: `03: Title` is `Title`.
fn ticket_title(h: &str) -> String {
    let h = h.trim();
    match h.split_once(':') {
        Some((num, rest)) if num.trim().chars().all(|c| c.is_ascii_digit()) => rest.trim(),
        _ => h,
    }
    .to_string()
}

/// The value of a `**Parallel:**` field, lowercased and trimmed at its start.
fn parse_parallel(v: &str) -> Option<bool> {
    if v.starts_with("yes") || v.starts_with("true") {
        Some(true)
    } else if v.starts_with("no") || v.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// The facts lines that hold a list of backticked items.
type ListField = (&'static str, fn(&mut TicketFacts) -> &mut Vec<String>);
const LIST_FIELDS: [ListField; 2] = [
    ("**own:**", |f| &mut f.own),
    ("**do not touch:**", |f| &mut f.no_touch),
];

/// Read the facts lines of a ticket's text.
pub fn ticket_facts(text: &str) -> TicketFacts {
    let mut f = TicketFacts::default();
    for line in text.lines() {
        let l = line.trim();
        if f.title.is_none() {
            f.title = l.strip_prefix("# ").map(ticket_title);
        }
        if f.version.is_none() {
            f.version = version_of(l);
        }
        let lower = l.to_lowercase();
        if lower.contains("**persona:**") {
            f.primary = ticked_after(l, "primary");
            f.fallback = ticked_after(l, "fallback");
        }
        if let Some(at) = lower.find("**parallel:**") {
            f.parallel = parse_parallel(lower[at + "**parallel:**".len()..].trim_start());
        }
        if lower.starts_with("**kind:**") {
            f.kind = WorkKind::parse(field_word(&lower, "**kind:**")).ok();
        }
        if lower.starts_with("**review:**") {
            f.review = ReviewMode::parse(field_word(&lower, "**review:**")).ok();
        }
        if lower.starts_with("**check:**") {
            f.check = ticked_after(l, "**check:**");
            f.check_none = f.check.is_none() && field_word(&lower, "**check:**") == "none";
        }
        for (prefix, field) in LIST_FIELDS {
            if lower.starts_with(prefix) {
                *field(&mut f) = ticked_list(l);
            }
        }
    }
    f
}

/// The C1 fields, in their order.
const FIELDS: [&str; 6] = ["Persona", "Kind", "Review", "Check", "Own", "Do not touch"];
const PERSONA: usize = 0;
const KIND: usize = 1;
const OWN: usize = 4;

/// The name of a `**Name:**` field line.
fn field_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("**")?;
    let at = rest.find(":**")?;
    Some(&rest[..at]).filter(|n| !n.contains('*'))
}

/// `'<word>' is not a <what>; use one of: a, b`.
fn not_one_of(what: &str, word: &str, all: &[&str]) -> String {
    format!("{what} '{word}' is not one of: {}", all.join(", "))
}

/// What is wrong with the value of field `idx` on `line`, if anything.
fn field_problem(idx: usize, line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let word = |prefix: &str| field_word(&lower, prefix).to_string();
    match FIELDS[idx] {
        "Persona" => {
            let parallel = lower
                .find("**parallel:**")
                .map(|at| parse_parallel(lower[at + "**parallel:**".len()..].trim_start()));
            if ticked_after(line, "primary").is_none() {
                Some("Persona names no primary teammate: write primary `<teammate>`".into())
            } else if parallel == Some(None) {
                Some("Parallel is not yes or no".into())
            } else {
                None
            }
        }
        "Kind" => {
            let w = word("**kind:**");
            let all: Vec<&str> = WorkKind::ALL.iter().map(|k| k.as_str()).collect();
            WorkKind::parse(&w)
                .err()
                .map(|_| not_one_of("Kind", &w, &all))
        }
        "Review" => {
            let w = word("**review:**");
            let all: Vec<&str> = ReviewMode::ALL.iter().map(|k| k.as_str()).collect();
            ReviewMode::parse(&w)
                .err()
                .map(|_| not_one_of("Review", &w, &all))
        }
        "Check" => (ticked_after(line, "**check:**").is_none() && word("**check:**") != "none")
            .then(|| "Check needs a backticked command or the word none".into()),
        _ => ticked_list(line).is_empty().then(|| {
            format!(
                "{} names no backticked path",
                field_name(line).unwrap_or("?")
            )
        }),
    }
}

/// The field block of a C1 ticket: its first line (the version comment)
/// and the field lines after it, as (0-based index, line).
fn field_block(lines: &[&str]) -> Result<(usize, Vec<usize>), (usize, String)> {
    let Some(h) = lines.iter().position(|l| l.trim().starts_with("# ")) else {
        return Err((1, "the ticket has no '# NN: Title' heading".into()));
    };
    let Some(start) = (h + 1..lines.len()).find(|&i| !lines[i].trim().is_empty()) else {
        return Err((
            h + 1,
            "the ticket has no field block after its heading".into(),
        ));
    };
    let block = (start + 1..lines.len())
        .take_while(|&i| !lines[i].trim().is_empty())
        .collect();
    Ok((start, block))
}

/// The C1 problems of a ticket's text, as (line, problem). Lines count from 1.
pub fn ticket_problems(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let (start, block) = match field_block(&lines) {
        Ok(b) => b,
        Err(p) => return vec![p],
    };
    match version_of(lines[start]) {
        None => {
            return vec![(
                start + 1,
                format!(
                    "the field block does not start with <!-- horch-ticket: {TICKET_VERSION} -->"
                ),
            )]
        }
        Some(v) if v != TICKET_VERSION => {
            return vec![(
                start + 1,
                format!(
                "horch-ticket version {v} is not known; this horch reads version {TICKET_VERSION}"
            ),
            )]
        }
        Some(_) => {}
    }
    let mut out = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for i in block {
        let l = lines[i].trim();
        let Some(name) = field_name(l) else {
            out.push((
                i + 1,
                format!("'{l}' is not a field line: a field line starts with **<Name>:**"),
            ));
            continue;
        };
        let Some(idx) = FIELDS.iter().position(|f| f.eq_ignore_ascii_case(name)) else {
            out.push((
                i + 1,
                format!(
                    "unknown field '{name}'; the fields are {}",
                    FIELDS.join(", ")
                ),
            ));
            continue;
        };
        if seen.last().is_some_and(|&last| idx <= last) {
            out.push((
                i + 1,
                format!(
                    "field '{name}' is out of order or repeated; the order is {}",
                    FIELDS.join(", ")
                ),
            ));
            continue;
        }
        seen.push(idx);
        out.extend(field_problem(idx, l).map(|p| (i + 1, p)));
    }
    let kind = ticket_facts(text).kind;
    let missing = [
        (PERSONA, true),
        (KIND, true),
        (OWN, kind != Some(WorkKind::Report)),
    ];
    for (idx, needed) in missing {
        if needed && !seen.contains(&idx) {
            let only = if idx == OWN {
                "; only a report ticket omits it"
            } else {
                ""
            };
            out.push((
                start + 1,
                format!("the field block has no **{}:** field{only}", FIELDS[idx]),
            ));
        }
    }
    out
}

/// 1 thing wrong with a map or a ticket, at a file and a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file, relative to the project root when it is inside it.
    pub file: String,
    /// The line, counted from 1.
    pub line: usize,
    pub problem: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.problem)
    }
}

/// How strictly a map is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `add-map`: a ticket with a version comment must follow C1; an older
    /// ticket keeps the old rules. The ledger checks the blockers.
    Queue,
    /// `add-map --dry-run`: every ticket must follow C1, and the blockers,
    /// keys and cycles are checked without the ledger.
    DryRun,
}

/// The first table of a map, and the `Spec:` line before it. Each table
/// line keeps its line number, counted from 1.
struct Table {
    spec_ref: Option<String>,
    header: Option<(usize, Vec<String>)>,
    rows: Vec<(usize, Vec<String>)>,
}

/// The first link of `s`: its target.
fn first_link(s: &str) -> Option<String> {
    links(s).into_iter().next().map(|(_, t)| t)
}

/// The spec a `Spec:` line names: its first link, else its text.
fn spec_ref_of(rest: &str) -> String {
    let rest = rest.trim();
    first_link(rest).unwrap_or(plain(rest.split('·').next().unwrap_or(rest)))
}

impl Table {
    /// Take a table line: the header first, then the rows.
    fn add(&mut self, line: usize, c: Vec<String>) {
        if self.header.is_none() {
            self.header = Some((line, c.iter().map(|h| h.to_lowercase()).collect()));
        } else if !is_rule(&c) {
            self.rows.push((line, c));
        }
    }

    /// Whether the first table has a row, so a line that is not a table line ends it.
    fn has_rows(&self) -> bool {
        !self.rows.is_empty()
    }
}

/// Find the `Spec:` line and the first table: its header and its rows.
fn scan_table(text: &str) -> Table {
    let mut t = Table {
        spec_ref: None,
        header: None,
        rows: Vec::new(),
    };
    for (i, l) in text.lines().map(str::trim).enumerate() {
        if t.spec_ref.is_none() {
            t.spec_ref = l.strip_prefix("Spec:").map(spec_ref_of);
        }
        match cells(l) {
            Some(c) => t.add(i + 1, c),
            None if t.has_rows() => break,
            None => {}
        }
    }
    t
}

/// The column of each field, by header name.
struct Columns {
    key: usize,
    title: Option<usize>,
    blocked: Option<usize>,
    wave: Option<usize>,
    issue: Option<usize>,
    persona: Option<usize>,
    parallel: Option<usize>,
}

fn columns(header: &[String]) -> Option<Columns> {
    let col = |name: &str| header.iter().position(|h| h == name);
    Some(Columns {
        key: col("#").or(col("ticket"))?,
        title: col("title"),
        blocked: col("blocked by"),
        wave: col("wave"),
        issue: col("issue"),
        persona: col("persona"),
        parallel: col("parallel"),
    })
}

/// What a row needs to become a ticket, and where its problems go.
struct RowContext<'a> {
    cols: Columns,
    dir: &'a Path,
    relative_to: &'a Path,
    /// The map file as problems name it.
    map_file: String,
    mode: Mode,
}

/// The text of cell `i` of `row`; empty when there is none.
fn cell(row: &[String], i: Option<usize>) -> String {
    i.and_then(|i| row.get(i)).cloned().unwrap_or_default()
}

/// The ticket file: the first link in the row to a `.md` file that exists.
fn ticket_path(row: &[String], dir: &Path) -> Option<PathBuf> {
    row.iter()
        .flat_map(|c| links(c))
        .map(|(_, t)| dir.join(t))
        .find(|p| p.extension().is_some_and(|e| e == "md") && p.is_file())
}

/// `ticket` as written relative to `relative_to` when it resolves inside it.
fn project_path(ticket: &Path, relative_to: &Path) -> String {
    let canon = ticket.canonicalize().unwrap_or(ticket.to_path_buf());
    let base = relative_to
        .canonicalize()
        .unwrap_or(relative_to.to_path_buf());
    canon
        .strip_prefix(&base)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| canon.to_string_lossy().into_owned())
}

/// The personas of a ticket; a `Persona` cell wins over the ticket's line.
fn personas(facts: &TicketFacts, cell: Option<&str>) -> (Option<String>, Option<String>) {
    let (mut primary, mut fallback) = (facts.primary.clone(), facts.fallback.clone());
    if let Some(p) = cell {
        let first_word = || {
            Some(plain(p).split([' ', '(']).next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
        };
        primary = ticked_after(p, "primary").or_else(first_word).or(primary);
        fallback = ticked_after(p, "fallback").or(fallback);
    }
    (primary, fallback)
}

/// The C1 problems of 1 ticket in `mode`. A ticket without a version comment
/// is an older ticket: `add-map` reads it by the old rules, a dry run refuses it.
fn c1_problems(text: &str, facts: &TicketFacts, mode: Mode) -> Vec<(usize, String)> {
    match (&facts.version, mode) {
        (Some(_), _) => ticket_problems(text),
        (None, Mode::DryRun) => ticket_problems(text),
        (None, Mode::Queue) => Vec::new(),
    }
}

/// A row and the line of the map it is on.
struct Row<'a> {
    line: usize,
    cells: &'a [String],
}

/// The text and the project path of a row's ticket file.
fn read_ticket(
    row: &Row,
    key: &str,
    ctx: &RowContext,
    problems: &mut Vec<Problem>,
) -> Option<(String, String)> {
    let at_map = |problem: String| Problem {
        file: ctx.map_file.clone(),
        line: row.line,
        problem,
    };
    let Some(ticket) = ticket_path(row.cells, ctx.dir) else {
        problems.push(at_map(format!(
            "ticket {key} links no ticket file that exists"
        )));
        return None;
    };
    let file = project_path(&ticket, ctx.relative_to);
    match std::fs::read_to_string(&ticket) {
        Ok(text) => Some((text, file)),
        Err(e) => {
            problems.push(at_map(format!("ticket {key}: reading {file}: {e}")));
            None
        }
    }
}

/// Map one table row to a ticket and its raw blocker tokens. A row with an
/// empty key is skipped. Issue numbers of the row go into `issue_to_key`.
/// Every problem goes into `problems`; a row with one queues nothing.
fn map_item(
    row: &Row,
    ctx: &RowContext,
    issue_to_key: &mut BTreeMap<String, String>,
    problems: &mut Vec<Problem>,
) -> Option<(MapItem, Vec<String>)> {
    let cols = &ctx.cols;
    let key = norm(&plain(&cell(row.cells, Some(cols.key))));
    if key.is_empty() {
        return None;
    }
    let issue = cell(row.cells, cols.issue);
    let issue_no = plain(&issue).trim_start_matches('#').to_string();
    if !issue_no.is_empty() {
        issue_to_key.insert(format!("#{issue_no}"), key.clone());
    }
    let (text, file) = read_ticket(row, &key, ctx, problems)?;
    let facts = ticket_facts(&text);
    let c1 = c1_problems(&text, &facts, ctx.mode);
    let at_ticket = |(line, problem): (usize, String)| Problem {
        file: file.clone(),
        line,
        problem,
    };
    if !c1.is_empty() {
        problems.extend(c1.into_iter().map(at_ticket));
        return None;
    }
    let (primary, fallback) = personas(
        &facts,
        cols.persona.map(|i| cell(row.cells, Some(i))).as_deref(),
    );
    let Some(primary) = primary else {
        problems.push(at_ticket((
            1,
            format!("ticket {key} names no primary persona: add a **Persona:** line"),
        )));
        return None;
    };
    let title = Some(plain(&cell(row.cells, cols.title)))
        .filter(|t| !t.is_empty())
        .or(facts.title.clone())
        .unwrap_or_else(|| format!("Ticket {key}"));
    let parallel = match cols.parallel {
        Some(_) => !plain(&cell(row.cells, cols.parallel))
            .to_lowercase()
            .starts_with("no"),
        None => facts.parallel.unwrap_or(true),
    };
    let task = NewTask {
        title,
        task_file: file,
        issue_url: first_link(&issue),
        spec_ref: None,
        primary,
        fallback,
        phase: None,
        parallel,
        priority: 100,
        wave: plain(&cell(row.cells, cols.wave)).parse::<i64>().ok(),
        worktree_mode: WorktreeMode::Worktree,
        land_check: facts.check.clone(),
        kind: TaskKind::Work,
        parent_task_id: None,
        blocked_by: Vec::new(),
        own: facts.own.clone(),
        no_touch: facts.no_touch.clone(),
        work_kind: facts.kind,
        review_mode: facts.review_mode(),
        ticket_snapshot: Some(text),
        check_none: facts.check_none,
    };
    let blockers = blocker_tokens(&cell(row.cells, cols.blocked));
    Some((MapItem { key, task }, blockers))
}

/// The key a blocker token names: issue numbers map to keys of this map,
/// `T-<n>` stays, and plain numbers are keys.
fn blocker_key(
    t: String,
    of: &str,
    issue_to_key: &BTreeMap<String, String>,
) -> Result<String, String> {
    if t.starts_with('#') {
        return issue_to_key
            .get(&t)
            .cloned()
            .ok_or_else(|| format!("ticket {of} is blocked by {t}, which is not in the map"));
    }
    Ok(if t.starts_with("T-") { t } else { norm(&t) })
}

/// The rows of the map that became items, with the map line of each.
struct Rows {
    items: Vec<MapItem>,
    lines: Vec<usize>,
    raw: Vec<Vec<String>>,
}

/// Set the blockers and the spec of each item from its raw tokens. A dry
/// run also checks that a plain key names a row of this map.
fn resolve_blockers(
    rows: &mut Rows,
    issue_to_key: &BTreeMap<String, String>,
    spec_ref: &Option<String>,
    ctx: &RowContext,
    problems: &mut Vec<Problem>,
) {
    let keys: Vec<String> = rows.items.iter().map(|i| i.key.clone()).collect();
    for ((item, tokens), &line) in rows
        .items
        .iter_mut()
        .zip(rows.raw.drain(..))
        .zip(&rows.lines)
    {
        for t in tokens {
            let key = blocker_key(t, &item.key, issue_to_key).and_then(|k| {
                let in_map = k.starts_with("T-") || keys.contains(&k);
                match (in_map, ctx.mode) {
                    (false, Mode::DryRun) => Err(format!(
                        "ticket {} is blocked by {k}, which is not in the map",
                        item.key
                    )),
                    _ => Ok(k),
                }
            });
            match key {
                Ok(k) => item.task.blocked_by.push(k),
                Err(problem) => problems.push(Problem {
                    file: ctx.map_file.clone(),
                    line,
                    problem,
                }),
            }
        }
        item.task.spec_ref = spec_ref.clone();
    }
}

/// Keys that 2 rows share, and a cycle among the rows (a dry run only:
/// `add-map` gets these from the ledger).
fn order_problems(rows: &Rows, ctx: &RowContext, problems: &mut Vec<Problem>) {
    let mut first: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, item) in rows.items.iter().enumerate() {
        if first.insert(item.key.as_str(), i).is_some() {
            problems.push(Problem {
                file: ctx.map_file.clone(),
                line: rows.lines[i],
                problem: format!("2 tickets in the map share the number {}", item.key),
            });
        }
    }
    if ctx.mode != Mode::DryRun {
        return;
    }
    let edges: Vec<Vec<usize>> = rows
        .items
        .iter()
        .map(|i| {
            i.task
                .blocked_by
                .iter()
                .filter_map(|b| first.get(b.as_str()).copied())
                .collect()
        })
        .collect();
    if let Some(cycle) = super::tasks::find_cycle(&edges) {
        let names: Vec<&str> = cycle.iter().map(|&i| rows.items[i].key.as_str()).collect();
        problems.push(Problem {
            file: ctx.map_file.clone(),
            line: rows.lines[cycle[0]],
            problem: format!("the map has a dependency cycle: {}", names.join(" -> ")),
        });
    }
}

/// A problem with the map file as a whole.
fn map_problem(map_file: &str, line: usize, problem: String) -> Vec<Problem> {
    vec![Problem {
        file: map_file.to_string(),
        line,
        problem,
    }]
}

/// Parse the map at `path` in `mode`: the tickets, and every problem.
fn parse(path: &Path, relative_to: &Path, mode: Mode) -> Result<(Map, Vec<Problem>)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the ticket map {}", path.display()))?;
    let map_file = project_path(path, relative_to);
    let table = scan_table(&text);
    let empty = |problems| {
        let map = Map {
            spec_ref: table.spec_ref.clone(),
            items: Vec::new(),
        };
        Ok((map, problems))
    };
    let Some((header_line, header)) = &table.header else {
        return empty(map_problem(
            &map_file,
            1,
            "the map has no ticket table".into(),
        ));
    };
    let Some(cols) = columns(header) else {
        return empty(map_problem(
            &map_file,
            *header_line,
            "the map table has no '#' column".into(),
        ));
    };
    let ctx = RowContext {
        cols,
        dir: path.parent().unwrap_or(Path::new(".")),
        relative_to,
        map_file,
        mode,
    };
    let mut problems = Vec::new();
    let mut issue_to_key = BTreeMap::new();
    let mut rows = Rows {
        items: Vec::new(),
        lines: Vec::new(),
        raw: Vec::new(),
    };
    for (line, cells) in &table.rows {
        let row = Row { line: *line, cells };
        if let Some((item, raw)) = map_item(&row, &ctx, &mut issue_to_key, &mut problems) {
            rows.items.push(item);
            rows.lines.push(*line);
            rows.raw.push(raw);
        }
    }
    resolve_blockers(
        &mut rows,
        &issue_to_key,
        &table.spec_ref,
        &ctx,
        &mut problems,
    );
    order_problems(&rows, &ctx, &mut problems);
    if rows.items.is_empty() && problems.is_empty() {
        problems = map_problem(
            &ctx.map_file,
            *header_line,
            "the map lists no tickets".into(),
        );
    }
    Ok((
        Map {
            spec_ref: table.spec_ref,
            items: rows.items,
        },
        problems,
    ))
}

/// Every problem, 1 per line.
fn joined(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse the map at `path` for `add-map`. Ticket paths are kept as written
/// relative to `relative_to` (the project root) when they resolve inside
/// it. Any problem refuses the whole map.
pub fn read(path: &Path, relative_to: &Path) -> Result<Map> {
    let (map, problems) = parse(path, relative_to, Mode::Queue)?;
    if !problems.is_empty() {
        bail!("{}", joined(&problems));
    }
    Ok(map)
}

/// Check the map at `path` and every ticket it links against C1, for
/// `add-map --dry-run`. Nothing is written. The error is only for a map
/// file that cannot be read.
pub fn check(path: &Path, relative_to: &Path) -> Result<(Map, Vec<Problem>)> {
    parse(path, relative_to, Mode::DryRun)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_by_cells() {
        assert_eq!(
            blocker_tokens("None (can start immediately)"),
            Vec::<String>::new()
        );
        assert_eq!(blocker_tokens("-"), Vec::<String>::new());
        assert_eq!(blocker_tokens("01, 02"), vec!["01", "02"]);
        assert_eq!(
            blocker_tokens("#33, #37-#40, #42"),
            vec!["#33", "#37", "#38", "#39", "#40", "#42"]
        );
        assert_eq!(blocker_tokens("02 (The store), T-4"), vec!["02", "T-4"]);
    }

    #[test]
    fn ticket_facts_lines() {
        let f = ticket_facts(
            "# 03: Queue tasks\n\n**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** no\n**Check:** `cargo test -p x`\n**Own:** `src/a.rs`, `src/b/`\n",
        );
        assert_eq!(f.title.as_deref(), Some("Queue tasks"));
        assert_eq!(f.primary.as_deref(), Some("backend-developer"));
        assert_eq!(f.fallback.as_deref(), Some("opus"));
        assert_eq!(f.parallel, Some(false));
        assert_eq!(f.check.as_deref(), Some("cargo test -p x"));
        assert_eq!(f.own, vec!["src/a.rs", "src/b/"]);
    }

    #[test]
    fn a_map_with_issue_blockers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let t = root.join("tickets");
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("01-a.md"), "# 01: A\n**Persona:** primary `opus`\n").unwrap();
        std::fs::write(
            t.join("02-b.md"),
            "# 02: B\n**Persona:** primary `sonnet`, fallback `opus` · **Parallel:** yes\n",
        )
        .unwrap();
        std::fs::write(
            t.join("MAP.md"),
            "# Map\nSpec: [#28](https://x/28)\n\n| # | Issue | Title | Blocked by | Wave |\n|---|---|---|---|---|\n\
             | [01](01-a.md) | [#29](https://x/29) | A | - | 1 |\n\
             | [02](02-b.md) | [#30](https://x/30) | B | #29 | 2 |\n\n## Waves\n",
        )
        .unwrap();
        let map = read(&t.join("MAP.md"), root).unwrap();
        assert_eq!(map.spec_ref.as_deref(), Some("https://x/28"));
        assert_eq!(map.items.len(), 2);
        assert_eq!(map.items[0].task.task_file, "tickets/01-a.md");
        assert_eq!(map.items[1].task.blocked_by, vec!["1"]);
        assert_eq!(map.items[1].task.fallback.as_deref(), Some("opus"));
        assert_eq!(map.items[1].task.issue_url.as_deref(), Some("https://x/30"));
    }

    fn write_map(root: &Path, map: &str, tickets: &[(&str, &str)]) -> PathBuf {
        let t = root.join("tickets");
        std::fs::create_dir_all(&t).unwrap();
        for (n, body) in tickets {
            std::fs::write(t.join(n), body).unwrap();
        }
        std::fs::write(t.join("MAP.md"), map).unwrap();
        t.join("MAP.md")
    }

    const PERSONA: &str = "# 01: A\n**Persona:** primary `opus`\n";

    #[test]
    fn blocker_ranges_tokens_and_limits() {
        assert_eq!(blocker_tokens("t-5"), vec!["T-5"]);
        assert_eq!(blocker_tokens("T-x"), Vec::<String>::new());
        assert_eq!(blocker_tokens("3-5"), vec!["3", "4", "5"]);
        assert_eq!(blocker_tokens("5-3"), Vec::<String>::new());
        assert_eq!(blocker_tokens("#007; 08 ;none").len(), 2);
        assert_eq!(blocker_tokens("#007; 08"), vec!["#007", "08"]);
        assert_eq!(blocker_tokens("1-1000").len(), 1000);
        assert_eq!(blocker_tokens("1-1001"), Vec::<String>::new());
        assert_eq!(blocker_tokens("[01](a.md), word"), vec!["01"]);
        assert_eq!(blocker_tokens("(only a title) 4"), vec!["4"]);
        assert_eq!(blocker_tokens("NONE"), Vec::<String>::new());
    }

    #[test]
    fn ticket_facts_titles_and_fields() {
        assert_eq!(
            ticket_facts("# Plain title\n").title.as_deref(),
            Some("Plain title")
        );
        assert_eq!(ticket_facts("# a: b\n").title.as_deref(), Some("a: b"));
        assert_eq!(
            ticket_facts("# 1: first\n# 2: second\n").title.as_deref(),
            Some("first")
        );
        assert_eq!(ticket_facts("**Parallel:** yes\n").parallel, Some(true));
        assert_eq!(ticket_facts("**Parallel:** True\n").parallel, Some(true));
        assert_eq!(ticket_facts("**Parallel:** no\n").parallel, Some(false));
        assert_eq!(ticket_facts("**Parallel:** False\n").parallel, Some(false));
        assert_eq!(ticket_facts("**Parallel:** maybe\n").parallel, None);
        let f = ticket_facts("**Do not touch:** `a.rs`, `b/`\n**Own:** `c.rs`\nOwn: `no`\n");
        assert_eq!(f.no_touch, vec!["a.rs", "b/"]);
        assert_eq!(f.own, vec!["c.rs"]);
        assert_eq!(
            ticket_facts("**Check:** `just t`\n").check.as_deref(),
            Some("just t")
        );
    }

    #[test]
    fn a_map_without_a_table_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), "# Map\nno table\n", &[]);
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "tickets/MAP.md:1: the map has no ticket table"
        );
    }

    #[test]
    fn a_map_without_a_key_column_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), "| Title |\n|---|\n| x |\n", &[]);
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "tickets/MAP.md:1: the map table has no '#' column"
        );
    }

    #[test]
    fn a_map_with_no_rows_lists_no_tickets() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), "| # | Title |\n|---|---|\n", &[]);
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(e.to_string(), "tickets/MAP.md:1: the map lists no tickets");
    }

    #[test]
    fn a_row_without_a_ticket_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(
            dir.path(),
            "| # | Title |\n|---|---|\n| 01 | [x](gone.md) |\n",
            &[],
        );
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "tickets/MAP.md:3: ticket 1 links no ticket file that exists"
        );
    }

    #[test]
    fn a_ticket_without_a_persona_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(
            dir.path(),
            "| # | Title |\n|---|---|\n| [01](a.md) | A |\n",
            &[("a.md", "# 01: A\n")],
        );
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "tickets/a.md:1: ticket 1 names no primary persona: add a **Persona:** line"
        );
    }

    #[test]
    fn a_blocker_outside_the_map_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(
            dir.path(),
            "| # | Issue | Blocked by |\n|---|---|---|\n| [01](a.md) | #5 | #9 |\n",
            &[("a.md", PERSONA)],
        );
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "tickets/MAP.md:3: ticket 1 is blocked by #9, which is not in the map"
        );
    }

    #[test]
    fn columns_in_the_map_win_over_the_ticket() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(
            dir.path(),
            "Spec: plain words · more\n\n| Ticket | Title | Persona | Parallel | Wave | Blocked by |\n|---|---|---|---|---|---|\n\
             | [01](a.md) | | primary `sonnet`, fallback `haiku` | No | 3 | - |\n\
             | [02](b.md) | Second | opus (x) | yes | z | 01, T-9, 1 |\n\nafter\n| 03 | ignored |\n",
            &[("a.md", "# 01: From file\n**Persona:** primary `opus`\n"), ("b.md", PERSONA)],
        );
        let m = read(&p, dir.path()).unwrap();
        assert_eq!(m.spec_ref.as_deref(), Some("plain words"));
        assert_eq!(m.items.len(), 2);
        let (a, b) = (&m.items[0].task, &m.items[1].task);
        assert_eq!(a.title, "From file");
        assert_eq!(a.primary, "sonnet");
        assert_eq!(a.fallback.as_deref(), Some("haiku"));
        assert!(!a.parallel);
        assert_eq!(a.wave, Some(3));
        assert_eq!(a.spec_ref.as_deref(), Some("plain words"));
        assert_eq!(b.title, "Second");
        assert_eq!(b.primary, "opus");
        assert!(b.parallel);
        assert_eq!(b.wave, None);
        assert_eq!(b.blocked_by, vec!["1", "T-9", "1"]);
        assert_eq!(b.task_file, "tickets/b.md");
    }

    /// A C1 ticket with `fields` as its field block after the version line.
    fn c1(fields: &str) -> String {
        format!("# 01: A\n\n<!-- horch-ticket: 1 -->\n{fields}\n\nSpec: x.\n\n## Goal\n\nG.\n")
    }

    const GOOD: &str = "**Persona:** primary `opus`, fallback `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`";

    const ONE_ROW: &str = "| # | Title | Blocked by |\n|---|---|---|\n| [01](a.md) | A | - |\n";

    #[test]
    fn trf_11_a_c1_ticket_has_no_problems() {
        assert_eq!(ticket_problems(&c1(GOOD)), Vec::<(usize, String)>::new());
        let report = c1("**Persona:** primary `opus`\n**Kind:** report\n**Check:** none");
        assert_eq!(ticket_problems(&report), Vec::<(usize, String)>::new());
    }

    #[test]
    fn trf_11_ticket_problems_name_each_line() {
        let text = c1(
            "**Persona:** fallback `dev` · **Parallel:** maybe\n**Review:** full\n**Kind:** chore\n**Areas:** `x`\nplain words\n**Check:** cargo test\n**Own:** none",
        );
        let got = ticket_problems(&text);
        let lines: Vec<usize> = got.iter().map(|(l, _)| *l).collect();
        assert_eq!(lines, vec![4, 6, 7, 8, 9, 10, 3], "{got:?}");
        assert!(got[0].1.contains("no primary teammate"), "{got:?}");
        assert!(got[1].1.contains("field 'Kind' is out of order"), "{got:?}");
        assert!(got[2].1.contains("unknown field 'Areas'"), "{got:?}");
        assert!(got[3].1.contains("is not a field line"), "{got:?}");
        assert!(
            got[4].1.contains("Check needs a backticked command"),
            "{got:?}"
        );
        assert!(got[5].1.contains("Own names no backticked path"), "{got:?}");
        assert!(got[6].1.contains("no **Kind:** field"), "{got:?}");
        let bad = ticket_problems(&c1("**Persona:** primary `o`\n**Kind:** feature\n**Review:** heavy\n**Check:** x\n**Own:** `a`"));
        assert_eq!(
            bad[0],
            (
                6,
                "Review 'heavy' is not one of: none, light, full".to_string()
            )
        );
        assert_eq!(
            bad[1],
            (
                7,
                "Check needs a backticked command or the word none".to_string()
            )
        );
        let no_own = ticket_problems(&c1("**Persona:** primary `o`\n**Kind:** docs"));
        assert_eq!(
            no_own,
            vec![(
                3,
                "the field block has no **Own:** field; only a report ticket omits it".to_string()
            )]
        );
    }

    #[test]
    fn trf_11_an_unknown_version_or_no_version_is_refused() {
        let v2 = c1(GOOD).replace("horch-ticket: 1", "horch-ticket: 2");
        assert_eq!(
            ticket_problems(&v2),
            vec![(
                3,
                "horch-ticket version 2 is not known; this horch reads version 1".to_string()
            )]
        );
        assert_eq!(
            ticket_problems("# 01: A\n\n**Persona:** primary `o`\n"),
            vec![(
                3,
                "the field block does not start with <!-- horch-ticket: 1 -->".to_string()
            )]
        );
        assert_eq!(ticket_problems("no heading\n")[0].0, 1);
        // add-map refuses a future version too, not only the dry run.
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), ONE_ROW, &[("a.md", &v2)]);
        let e = read(&p, dir.path()).unwrap_err().to_string();
        assert_eq!(
            e,
            "tickets/a.md:3: horch-ticket version 2 is not known; this horch reads version 1"
        );
    }

    #[test]
    fn trf_11_dry_run_names_every_problem_with_its_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let map = "# Map\n\n| # | Title | Blocked by |\n|---|---|---|\n\
                   | [01](a.md) | A | 03 |\n| [02](b.md) | B | 09 |\n| [03](c.md) | C | 01 |\n\
                   | [04](d.md) | D | - |\n| [05](gone.md) | E | - |\n";
        let good = c1(GOOD);
        let p = write_map(
            dir.path(),
            map,
            &[
                ("a.md", &good),
                ("b.md", &good),
                ("c.md", &good),
                ("d.md", "# 04: Old\n\n**Persona:** primary `opus`\n"),
            ],
        );
        let (parsed, problems) = check(&p, dir.path()).unwrap();
        let got: Vec<String> = problems.iter().map(Problem::to_string).collect();
        assert_eq!(
            got,
            vec![
                "tickets/a.md:3: the field block does not start with <!-- horch-ticket: 1 -->"
                    .replace("a.md", "d.md"),
                "tickets/MAP.md:9: ticket 5 links no ticket file that exists".to_string(),
                "tickets/MAP.md:6: ticket 2 is blocked by 9, which is not in the map".to_string(),
                "tickets/MAP.md:5: the map has a dependency cycle: 1 -> 3 -> 1".to_string(),
            ]
        );
        assert_eq!(parsed.items.len(), 3);
        // The same map without the problem rows passes.
        let p = write_map(dir.path(), ONE_ROW, &[("a.md", &good)]);
        assert_eq!(check(&p, dir.path()).unwrap().1, Vec::<Problem>::new());
    }

    #[test]
    fn trf_11_an_older_ticket_still_queues_without_a_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), ONE_ROW, &[("a.md", PERSONA)]);
        assert_eq!(read(&p, dir.path()).unwrap().items.len(), 1);
        assert_eq!(check(&p, dir.path()).unwrap().1.len(), 1);
    }

    #[test]
    fn trf_14_kind_sets_the_default_review() {
        let want = [
            ("feature", ReviewMode::NoReview),
            ("docs", ReviewMode::NoReview),
            ("report", ReviewMode::NoReview),
            ("quality", ReviewMode::Full),
            ("refactor", ReviewMode::Full),
            ("security", ReviewMode::Full),
        ];
        for (kind, review) in want {
            let f = ticket_facts(&format!("**Kind:** {kind}\n"));
            assert_eq!(f.kind.map(WorkKind::as_str), Some(kind));
            assert_eq!(f.review_mode(), Some(review), "{kind}");
        }
        assert_eq!(ticket_facts("# x\n").review_mode(), None);
    }

    #[test]
    fn trf_14_an_explicit_review_wins_and_check_none_is_stored() {
        let f = ticket_facts("**Kind:** refactor\n**Review:** light\n**Check:** none\n");
        assert_eq!(f.review_mode(), Some(ReviewMode::Light));
        assert!(f.check_none);
        assert_eq!(f.check, None);
        let f = ticket_facts("**Kind:** feature\n**Review:** full\n**Check:** `none`\n");
        assert_eq!(f.review_mode(), Some(ReviewMode::Full));
        assert!(!f.check_none, "a backticked `none` is a command");
        assert_eq!(f.check.as_deref(), Some("none"));
        assert!(!ticket_facts("# no check\n").check_none);

        let dir = tempfile::tempdir().unwrap();
        let text =
            c1("**Persona:** primary `opus`\n**Kind:** security\n**Check:** none\n**Own:** `a.rs`");
        let p = write_map(dir.path(), ONE_ROW, &[("a.md", &text)]);
        let t = read(&p, dir.path()).unwrap().items.remove(0).task;
        assert_eq!(t.work_kind, Some(WorkKind::Security));
        assert_eq!(t.review_mode, Some(ReviewMode::Full));
        assert!(t.check_none);
        assert_eq!(t.land_check, None);
        assert_eq!(t.ticket_snapshot.as_deref(), Some(text.as_str()));
    }
}
