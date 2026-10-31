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

use super::model::{TaskKind, WorktreeMode};
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
        let lower = l.to_lowercase();
        if lower.contains("**persona:**") {
            f.primary = ticked_after(l, "primary");
            f.fallback = ticked_after(l, "fallback");
        }
        if let Some(at) = lower.find("**parallel:**") {
            f.parallel = parse_parallel(lower[at + "**parallel:**".len()..].trim_start());
        }
        if lower.starts_with("**check:**") {
            f.check = ticked_after(l, "**check:**");
        }
        for (prefix, field) in LIST_FIELDS {
            if lower.starts_with(prefix) {
                *field(&mut f) = ticked_list(l);
            }
        }
    }
    f
}

/// The first table of a map, and the `Spec:` line before it.
struct Table {
    spec_ref: Option<String>,
    header: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
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
    fn add(&mut self, c: Vec<String>) {
        if self.header.is_none() {
            self.header = Some(c.iter().map(|h| h.to_lowercase()).collect());
        } else if !is_rule(&c) {
            self.rows.push(c);
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
    for l in text.lines().map(str::trim) {
        if t.spec_ref.is_none() {
            t.spec_ref = l.strip_prefix("Spec:").map(spec_ref_of);
        }
        match cells(l) {
            Some(c) => t.add(c),
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

fn columns(header: &[String]) -> Result<Columns> {
    let col = |name: &str| header.iter().position(|h| h == name);
    Ok(Columns {
        key: col("#")
            .or(col("ticket"))
            .context("the map table has no '#' column")?,
        title: col("title"),
        blocked: col("blocked by"),
        wave: col("wave"),
        issue: col("issue"),
        persona: col("persona"),
        parallel: col("parallel"),
    })
}

/// What a row needs to become a ticket.
struct RowContext<'a> {
    cols: Columns,
    dir: &'a Path,
    relative_to: &'a Path,
    path: &'a Path,
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

/// Map one table row to a ticket and its raw blocker tokens. A row with an
/// empty key is skipped. Issue numbers of the row go into `issue_to_key`.
fn map_item(
    row: &[String],
    ctx: &RowContext,
    issue_to_key: &mut BTreeMap<String, String>,
) -> Result<Option<(MapItem, Vec<String>)>> {
    let cols = &ctx.cols;
    let key = norm(&plain(&cell(row, Some(cols.key))));
    if key.is_empty() {
        return Ok(None);
    }
    let issue = cell(row, cols.issue);
    let issue_no = plain(&issue).trim_start_matches('#').to_string();
    if !issue_no.is_empty() {
        issue_to_key.insert(format!("#{issue_no}"), key.clone());
    }
    let Some(ticket) = ticket_path(row, ctx.dir) else {
        bail!(
            "ticket {key} in {} links no ticket file that exists",
            ctx.path.display()
        );
    };
    let facts = ticket_facts(&std::fs::read_to_string(&ticket)?);
    let file = project_path(&ticket, ctx.relative_to);
    let (primary, fallback) = personas(&facts, cols.persona.map(|i| cell(row, Some(i))).as_deref());
    let Some(primary) = primary else {
        bail!("ticket {key} ({file}) names no primary persona: add a **Persona:** line");
    };
    let title = Some(plain(&cell(row, cols.title)))
        .filter(|t| !t.is_empty())
        .or(facts.title.clone())
        .unwrap_or_else(|| format!("Ticket {key}"));
    let parallel = match cols.parallel {
        Some(_) => !plain(&cell(row, cols.parallel))
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
        wave: plain(&cell(row, cols.wave)).parse::<i64>().ok(),
        worktree_mode: WorktreeMode::Worktree,
        land_check: facts.check.clone(),
        kind: TaskKind::Work,
        parent_task_id: None,
        blocked_by: Vec::new(),
        own: facts.own.clone(),
        no_touch: facts.no_touch.clone(),
    };
    let blockers = blocker_tokens(&cell(row, cols.blocked));
    Ok(Some((MapItem { key, task }, blockers)))
}

/// The key a blocker token names: issue numbers map to keys of this map,
/// `T-<n>` stays, and plain numbers are keys.
fn blocker_key(t: String, of: &str, issue_to_key: &BTreeMap<String, String>) -> Result<String> {
    if t.starts_with('#') {
        return issue_to_key
            .get(&t)
            .cloned()
            .with_context(|| format!("ticket {of} is blocked by {t}, which is not in the map"));
    }
    Ok(if t.starts_with("T-") { t } else { norm(&t) })
}

/// Set the blockers and the spec of each item from its raw tokens.
fn resolve_blockers(
    items: &mut [MapItem],
    raw: Vec<Vec<String>>,
    issue_to_key: &BTreeMap<String, String>,
    spec_ref: &Option<String>,
) -> Result<()> {
    for (item, tokens) in items.iter_mut().zip(raw) {
        for t in tokens {
            let key = blocker_key(t, &item.key, issue_to_key)?;
            item.task.blocked_by.push(key);
        }
        item.task.spec_ref = spec_ref.clone();
    }
    Ok(())
}

/// Parse the map at `path`. Ticket paths are kept as written relative to
/// `relative_to` (the project root) when they resolve inside it.
pub fn read(path: &Path, relative_to: &Path) -> Result<Map> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the ticket map {}", path.display()))?;
    let table = scan_table(&text);
    let Some(header) = &table.header else {
        bail!("{} has no ticket table", path.display());
    };
    let ctx = RowContext {
        cols: columns(header)?,
        dir: path.parent().unwrap_or(Path::new(".")),
        relative_to,
        path,
    };
    let mut issue_to_key = BTreeMap::new();
    let mapped = table
        .rows
        .iter()
        .filter_map(|row| map_item(row, &ctx, &mut issue_to_key).transpose())
        .collect::<Result<Vec<_>>>()?;
    let (mut items, raw): (Vec<MapItem>, Vec<Vec<String>>) = mapped.into_iter().unzip();
    resolve_blockers(&mut items, raw, &issue_to_key, &table.spec_ref)?;
    if items.is_empty() {
        bail!("{} lists no tickets", path.display());
    }
    Ok(Map {
        spec_ref: table.spec_ref,
        items,
    })
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
            format!("{} has no ticket table", p.display())
        );
    }

    #[test]
    fn a_map_without_a_key_column_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), "| Title |\n|---|\n| x |\n", &[]);
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(e.to_string(), "the map table has no '#' column");
    }

    #[test]
    fn a_map_with_no_rows_lists_no_tickets() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_map(dir.path(), "| # | Title |\n|---|---|\n", &[]);
        let e = read(&p, dir.path()).unwrap_err();
        assert_eq!(e.to_string(), format!("{} lists no tickets", p.display()));
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
            format!(
                "ticket 1 in {} links no ticket file that exists",
                p.display()
            )
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
            "ticket 1 (tickets/a.md) names no primary persona: add a **Persona:** line"
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
            "ticket 1 is blocked by #9, which is not in the map"
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
}
