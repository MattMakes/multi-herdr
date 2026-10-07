//! `horch cost` - what a fleet run cost, per worker, read from transcripts.
//!
//! Informational: spend telemetry to see where tokens go; not an input to
//! routing, model or teammate choice. No field of a report recommends
//! anything.
//!
//! Every worker in the project's ledger is priced from its own harness's
//! transcript (see `horch_core::usage`). Sessions that cannot be priced - no
//! session id, no transcript, an unread harness, an unknown model - are listed
//! separately and never folded into a total as zero.
//!
//! With no filter the command writes 3 window reports (30d, 7d, 24h, ending
//! now). A window counts each call by its own time, so a session that
//! started before the window shows only its calls inside it.
//!
//! The same pass audits skills: which skills each worker actually loaded,
//! and which of the skills its teammate file says it is expected to use it
//! never touched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Utc};
use horch_core::clock;
use horch_core::execution::legacy::Record;
use horch_core::execution::store::ExecutionStore;
use horch_core::roster::{Phase, Roster};
use horch_core::runtime::RuntimeContext;
use horch_core::usage::{self, Locations, Missing, Price, Session, Span, Tokens};
use serde::Serialize;

use crate::output;

mod render;
mod tables;

pub use render::render;
pub use tables::{SkillUse, Tables};

/// The windows a run with no filter reports, newest last in the files' order.
pub const WINDOWS: [(&str, i64); 3] = [("30d", 30 * 24), ("7d", 7 * 24), ("24h", 24)];

pub struct CostArgs {
    /// Human tables instead of JSON.
    pub text: bool,
    /// Where report files go.
    pub dir: Option<String>,
    pub reprice: Option<String>,
    pub pricing: Option<String>,
    /// Only calls at or after this time (RFC 3339, or a date at midnight UTC).
    pub since: Option<String>,
    /// Only these records (record or session ids).
    pub records: Vec<String>,
    /// Extra sessions outside the ledger, as `<agent>:<session-id>` - the
    /// orchestrator's own claude session, say.
    pub sessions: Vec<String>,
}

impl CostArgs {
    fn is_filtered(&self) -> bool {
        self.since.is_some() || !self.records.is_empty() || !self.sessions.is_empty()
    }
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub record_id: String,
    pub teammate: String,
    pub role: String,
    /// `worker` or `orchestrator`.
    pub kind: String,
    pub agent: String,
    pub model: String,
    pub effort: Option<String>,
    pub family: String,
    pub calls: u64,
    pub tokens: Tokens,
    /// Dollars for the models the price table knows.
    pub cost: f64,
    /// What the same tokens would cost on `--reprice`'s model.
    pub reprice_cost: Option<f64>,
    /// Models in the transcript the price table does not know. Their tokens
    /// are counted but not priced.
    pub unpriced_models: Vec<String>,
    pub transcript: Option<PathBuf>,
    pub skills_used: BTreeMap<String, u64>,
    pub expected_unused: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NotPriced {
    pub record_id: String,
    pub teammate: String,
    pub agent: String,
    pub reason: Missing,
}

#[derive(Debug, Default, Serialize)]
pub struct Rollup {
    pub sessions: u64,
    pub calls: u64,
    pub tokens: Tokens,
    pub cost: f64,
    pub reprice_cost: Option<f64>,
}

/// The time range a report covers. `start` is absent for an open start.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    /// `30d`, `7d`, `24h`, or `custom` for a filtered run.
    pub name: String,
    pub start: Option<String>,
    pub end: Option<String>,
}

/// A priced session whose transcript also names a model with no price.
#[derive(Debug, Serialize)]
pub struct PartlyPriced {
    pub record_id: String,
    pub teammate: String,
    pub agent: String,
    pub models: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub window: Window,
    pub prices_as_of: &'static str,
    pub reprice: Option<String>,
    pub rows: Vec<Row>,
    pub not_priced: Vec<NotPriced>,
    pub by_teammate: BTreeMap<String, Rollup>,
    pub by_harness: BTreeMap<String, Rollup>,
    pub by_family: BTreeMap<String, Rollup>,
    pub total: Rollup,
    /// Per tier (workers only), the orchestrator, and per teammate, with
    /// means per session: the tables of `ai_docs/reports/wave2/baseline.md`.
    pub tables: Tables,
    /// Per teammate: how many sessions loaded each expected skill.
    pub skill_use: Vec<SkillUse>,
    pub partly_priced: Vec<PartlyPriced>,
}

pub fn cost(ctx: &RuntimeContext, args: CostArgs) -> Result<()> {
    let store = ExecutionStore::open_in(ctx)?;
    let records = store.read()?;
    let project = ctx.paths.project()?;
    let roster: Option<Roster> = super::load_roster(ctx, None).ok();
    let out = run(
        &args,
        records,
        roster.as_ref(),
        &Locations::from_context(ctx),
        clock::now(),
        &project,
        &store.path().display().to_string(),
    )?;
    if args.text {
        output::print(&out);
    } else {
        output::println(&out);
    }
    Ok(())
}

/// What `horch cost` prints, after it wrote its files. `now` ends the
/// windows; its local time names the files.
pub fn run(
    args: &CostArgs,
    mut records: Vec<Record>,
    roster: Option<&Roster>,
    loc: &Locations,
    now: DateTime<Utc>,
    project: &Path,
    ledger: &str,
) -> Result<String> {
    let since =
        match &args.since {
            Some(s) => Some(clock::parse(s).map(clock::stamp).ok_or_else(|| {
                anyhow::anyhow!("--since: '{s}' is not a date or an RFC 3339 time")
            })?),
            None => None,
        };
    if !args.records.is_empty() {
        records.retain(|r| {
            args.records
                .iter()
                .any(|k| *k == r.record_id || r.session_id.as_deref() == Some(k.as_str()))
        });
    }
    for extra in &args.sessions {
        let Some((agent, id)) = extra.split_once(':') else {
            bail!("--session takes <agent>:<session-id>, e.g. claude:0f10e145-...");
        };
        records.push(Record {
            record_id: format!("extra:{id}"),
            session_id: Some(id.to_string()),
            agent: agent.to_string(),
            tier: "(outside ledger)".into(),
            model: String::new(),
            effort: None,
            phase: None,
            role: "extra".into(),
            status: String::new(),
            task: String::new(),
            history: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            ..Record::default()
        });
    }
    let prices = usage::load_prices(args.pricing.as_deref().map(Path::new))?;
    let reprice = match &args.reprice {
        Some(model) => match usage::cost_of(&prices, model, &Tokens::default()) {
            Some(_) => Some(model.as_str()),
            None => bail!(
                "--reprice: no price for '{model}' (known: {})",
                known(&prices)
            ),
        },
        None => None,
    };
    let local = now.with_timezone(&Local);
    let prefix = local.format("%Y_%m_%d_%H%M%S").to_string();
    let read = read_all(&records, loc);

    if args.is_filtered() {
        let window = Window {
            name: "custom".into(),
            start: since.clone(),
            end: None,
        };
        let report = report(&read, roster, &prices, reprice, window);
        if let Some(dir) = &args.dir {
            write_json(
                &Path::new(dir).join(format!("{prefix}_custom.json")),
                &report,
            )?;
        }
        return Ok(if args.text {
            render(&report, ledger)
        } else {
            serde_json::to_string_pretty(&report)?
        });
    }

    let dir = args
        .dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("ai_docs/reports/cost"));
    let mut files = BTreeMap::new();
    let mut windows = BTreeMap::new();
    let mut text = String::new();
    for (name, hours) in WINDOWS {
        let start = clock::stamp(now - chrono::Duration::hours(hours));
        let window = Window {
            name: name.to_string(),
            start: Some(start),
            end: Some(clock::stamp(now)),
        };
        let report = report(&read, roster, &prices, reprice, window);
        let path = dir.join(format!("{prefix}_{name}.json"));
        write_json(&path, &report)?;
        if args.text {
            text.push_str(&render(&report, ledger));
            text.push_str(&format!("\nFile: {}\n\n", path.display()));
        }
        windows.insert(name.to_string(), WindowTotals::of(&report));
        files.insert(name.to_string(), path);
    }
    if args.text {
        return Ok(text);
    }
    let summary = Summary {
        generated_at: local.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        project: project.to_path_buf(),
        prices_as_of: PRICES_AS_OF,
        files,
        windows,
    };
    Ok(serde_json::to_string_pretty(&summary)?)
}

const PRICES_AS_OF: &str = "2026-09-24";

/// What a run with no filter prints: where the 3 files are and each
/// window's totals.
#[derive(Debug, Serialize)]
pub struct Summary {
    /// Local time with its offset.
    pub generated_at: String,
    pub project: PathBuf,
    pub prices_as_of: &'static str,
    pub files: BTreeMap<String, PathBuf>,
    pub windows: BTreeMap<String, WindowTotals>,
}

#[derive(Debug, Serialize)]
pub struct WindowTotals {
    pub start: Option<String>,
    pub end: Option<String>,
    pub sessions: u64,
    pub calls: u64,
    pub tokens: Tokens,
    pub cost: f64,
    /// Sessions not priced at all, plus priced ones with an unpriced model.
    pub unpriced_sessions: u64,
}

impl WindowTotals {
    fn of(r: &Report) -> Self {
        WindowTotals {
            start: r.window.start.clone(),
            end: r.window.end.clone(),
            sessions: r.total.sessions,
            calls: r.total.calls,
            tokens: r.total.tokens,
            cost: r.total.cost,
            unpriced_sessions: (r.not_priced.len() + r.partly_priced.len()) as u64,
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

fn known(prices: &BTreeMap<String, Price>) -> String {
    prices.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// The role family a phase belongs to, as cezaar#40 groups spend.
fn family(phase: Option<Phase>, role: &str) -> String {
    match phase {
        _ if role == "extra" => "extra",
        Some(Phase::Research) => "research",
        Some(Phase::Plan) => "plan",
        Some(Phase::Implementation) => "build",
        Some(Phase::Validation) => "review",
        None => "other",
    }
    .to_string()
}

/// The skills a teammate is expected to use, as the transcripts name them:
/// bundled and operator ones bare (`tdd`), plugin ones qualified
/// (`code:review`).
fn expected_skills(roster: Option<&Roster>, teammate: &str) -> Vec<String> {
    let Some(t) = roster.and_then(|r| r.get(teammate)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = t.skills.clone();
    out.extend(t.operator_skills.iter().flat_map(|o| o.names.clone()));
    for (plugin, skills) in &t.plugin_skills {
        out.extend(skills.iter().map(|s| format!("{plugin}:{s}")));
    }
    out
}

/// Every record with its session read once, or why it could not be read.
pub fn read_all<'a>(
    records: &'a [Record],
    loc: &Locations,
) -> Vec<(&'a Record, std::result::Result<Session, Missing>)> {
    records
        .iter()
        .map(|r| {
            (
                r,
                usage::read_session_events(loc, &r.agent, r.session_id.as_deref()),
            )
        })
        .collect()
}

/// The whole-history report of `records`: every call, whenever it was made.
#[cfg(test)]
pub fn build(
    records: &[Record],
    roster: Option<&Roster>,
    loc: &Locations,
    prices: &BTreeMap<String, Price>,
    reprice: Option<&str>,
) -> Report {
    let window = Window {
        name: "all".into(),
        start: None,
        end: None,
    };
    report(&read_all(records, loc), roster, prices, reprice, window)
}

/// Whether an unreadable record may have run inside `span`: its ledger
/// life (created to last updated) overlaps it. A record with no times (a
/// `--session` extra) always may.
fn may_overlap(r: &Record, span: &Span) -> bool {
    if r.created_at.is_empty() {
        return true;
    }
    let last = r.updated_at.as_str().max(r.created_at.as_str());
    span.since.as_deref().is_none_or(|s| last >= s)
        && span
            .until
            .as_deref()
            .is_none_or(|u| r.created_at.as_str() < u)
}

/// The report of the calls inside `window`. With a bounded window, a
/// session appears only when it has a call inside it, with only those calls.
pub fn report(
    read: &[(&Record, std::result::Result<Session, Missing>)],
    roster: Option<&Roster>,
    prices: &BTreeMap<String, Price>,
    reprice: Option<&str>,
    window: Window,
) -> Report {
    let span = Span {
        since: window.start.clone(),
        until: window.end.clone(),
    };
    let mut report = Report {
        window,
        prices_as_of: PRICES_AS_OF,
        reprice: reprice.map(str::to_string),
        rows: Vec::new(),
        not_priced: Vec::new(),
        by_teammate: BTreeMap::new(),
        by_harness: BTreeMap::new(),
        by_family: BTreeMap::new(),
        total: Rollup::default(),
        tables: Tables::default(),
        skill_use: Vec::new(),
        partly_priced: Vec::new(),
    };
    for (r, found) in read {
        let session = match found {
            Ok(session) => session,
            Err(reason) => {
                if may_overlap(r, &span) {
                    report.not_priced.push(NotPriced {
                        record_id: r.record_id.clone(),
                        teammate: r.tier.clone(),
                        agent: r.agent.clone(),
                        reason: reason.clone(),
                    });
                }
                continue;
            }
        };
        let used = session.usage(&span);
        if !span.is_all() && used.by_model.is_empty() {
            continue;
        }
        let path = session.path.clone();
        let mut cost = 0.0;
        let mut unpriced = Vec::new();
        for (model, tokens) in &used.by_model {
            // A transcript that never names its model is priced as the
            // model the ledger launched.
            let model = if model.is_empty() { &r.model } else { model };
            match usage::cost_of(prices, model, tokens) {
                Some(c) => cost += c,
                None => unpriced.push(model.clone()),
            }
        }
        let tokens = used.tokens();
        let skills_used: BTreeMap<String, u64> = used
            .skills
            .iter()
            .map(|(k, v)| (k.strip_prefix("horch:").unwrap_or(k).to_string(), *v))
            .collect();
        let expected_unused = expected_skills(roster, &r.tier)
            .into_iter()
            .filter(|s| !skills_used.contains_key(s))
            .collect();
        let models: BTreeSet<&str> = used
            .by_model
            .keys()
            .map(|m| {
                if m.is_empty() {
                    r.model.as_str()
                } else {
                    m.as_str()
                }
            })
            .collect();
        let row = Row {
            record_id: r.record_id.clone(),
            teammate: r.tier.clone(),
            role: r.role.clone(),
            kind: r.kind.clone(),
            agent: r.agent.clone(),
            model: models.into_iter().collect::<Vec<_>>().join(", "),
            effort: r.effort.clone(),
            family: family(r.phase, &r.role),
            calls: used.calls,
            tokens,
            cost,
            reprice_cost: reprice.and_then(|m| usage::cost_of(prices, m, &tokens)),
            unpriced_models: unpriced,
            transcript: Some(path),
            skills_used,
            expected_unused,
        };
        for (map, key) in [
            (&mut report.by_teammate, row.teammate.clone()),
            (&mut report.by_harness, row.agent.clone()),
            (&mut report.by_family, row.family.clone()),
        ] {
            add(map.entry(key).or_default(), &row);
        }
        add(&mut report.total, &row);
        if !row.unpriced_models.is_empty() {
            report.partly_priced.push(PartlyPriced {
                record_id: row.record_id.clone(),
                teammate: row.teammate.clone(),
                agent: row.agent.clone(),
                models: row.unpriced_models.clone(),
            });
        }
        report.rows.push(row);
    }
    report.tables = Tables::of(&report.rows);
    report.skill_use = tables::skill_use(&report.rows, roster);
    report
}

fn add(roll: &mut Rollup, row: &Row) {
    roll.sessions += 1;
    roll.calls += row.calls;
    roll.tokens.add(&row.tokens);
    roll.cost += row.cost;
    if let Some(r) = row.reprice_cost {
        roll.reprice_cost = Some(roll.reprice_cost.unwrap_or(0.0) + r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        id: &str,
        agent: &str,
        tier: &str,
        sid: Option<&str>,
        phase: Option<Phase>,
    ) -> Record {
        Record {
            record_id: id.into(),
            session_id: sid.map(str::to_owned),
            agent: agent.into(),
            tier: tier.into(),
            model: String::new(),
            effort: Some("medium".into()),
            phase,
            role: format!("{tier}-1"),
            status: "done".into(),
            task: String::new(),
            history: Vec::new(),
            created_at: "2026-09-24T00:00:00Z".into(),
            updated_at: String::new(),
            ..Record::default()
        }
    }

    /// Priced rows roll up per family and harness; everything that cannot be
    /// priced is listed with its reason, and never counted as $0.
    #[test]
    fn a_run_is_priced_per_worker_and_the_rest_is_listed_not_zeroed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../horch-core/tests/fixtures/usage");
        let claude = home.join(".claude/projects/-proj");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::copy(fixtures.join("claude.jsonl"), claude.join("sid-c.jsonl")).unwrap();
        let codex = home.join(".codex/sessions/2026/09/24");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::copy(
            fixtures.join("codex.jsonl"),
            codex.join("rollout-2026-09-24T00-00-00-sid-x.jsonl"),
        )
        .unwrap();
        let loc = Locations {
            home: home.to_path_buf(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join(".local/share/opencode/opencode.db"),
            sqlite3: "sqlite3".into(),
        };
        let records = vec![
            record(
                "r1",
                "claude",
                "backend-developer",
                Some("sid-c"),
                Some(Phase::Implementation),
            ),
            record(
                "r2",
                "codex",
                "codex-reviewer",
                Some("sid-x"),
                Some(Phase::Validation),
            ),
            record(
                "r3",
                "opencode",
                "opencode-pickle",
                Some("ses_1"),
                Some(Phase::Implementation),
            ),
            record(
                "r4",
                "codex",
                "codex-sol",
                None,
                Some(Phase::Implementation),
            ),
        ];
        let roster = Roster::builtin().unwrap();
        let prices = usage::builtin_prices();
        let report = build(
            &records,
            Some(&roster),
            &loc,
            &prices,
            Some("claude-opus-5-5"),
        );

        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.not_priced.len(), 2);
        assert_eq!(
            report.not_priced[0].reason,
            Missing::NoTranscript,
            "no opencode.db here"
        );
        assert_eq!(report.not_priced[1].reason, Missing::NoSessionId);
        assert_eq!(report.by_family["build"].sessions, 1);
        assert_eq!(report.by_family["review"].sessions, 1);
        assert!(report.total.cost > 0.0);
        let sum: f64 = report.rows.iter().map(|r| r.cost).sum();
        assert!((report.total.cost - sum).abs() < 1e-12);
        assert!(report.total.reprice_cost.is_some());

        // The expected skills come from the roster that `build` read, so a
        // roster edit does not break this test. The transcript loaded tdd
        // only, so every other expected skill is reported unused.
        let backend = &report.rows[0];
        assert_eq!(backend.skills_used.get("tdd"), Some(&1));
        let backend_unused: Vec<String> = expected_skills(Some(&roster), "backend-developer")
            .into_iter()
            .filter(|s| s != "tdd")
            .collect();
        assert!(backend_unused.contains(&"security-review".to_string()));
        assert_eq!(backend.expected_unused, backend_unused);
        // The codex transcript loaded code-review and never security-review.
        let reviewer = &report.rows[1];
        let reviewer_unused: Vec<String> = expected_skills(Some(&roster), "codex-reviewer")
            .into_iter()
            .filter(|s| s != "code-review")
            .collect();
        assert!(reviewer_unused.contains(&"security-review".to_string()));
        assert_eq!(reviewer.expected_unused, reviewer_unused);

        let md = render(&report, "ledger.json");
        assert!(md.contains("| claude claude-opus-5-5 medium | 1 |"), "{md}");
        assert!(md.contains("## Not priced"), "{md}");
        assert!(
            md.contains("opencode-pickle (opencode, r3): no transcript found"),
            "{md}"
        );
        assert!(md.contains("| cost on claude-opus-5-5 |"), "{md}");
    }

    const NOW: &str = "2026-10-07T10:00:00Z";

    fn claude_line(at: &str, id: &str, out: u64, skill: Option<&str>) -> String {
        let content = match skill {
            Some(s) => serde_json::json!([{
                "type": "tool_use", "id": format!("tu-{id}"), "name": "Skill",
                "input": {"skill": s},
            }]),
            None => serde_json::json!([]),
        };
        serde_json::json!({
            "type": "assistant", "timestamp": at,
            "message": {
                "id": id, "role": "assistant", "model": "claude-opus-5-5",
                "content": content,
                "usage": {"input_tokens": 2, "cache_read_input_tokens": 1000,
                          "cache_creation_input_tokens": 50, "output_tokens": out},
            },
        })
        .to_string()
    }

    /// A home with 2 Claude sessions, and their ledger records:
    /// - `w1`, a worker that started 40 days before NOW (its ledger record
    ///   last updated 2 hours ago), with calls at -40d,
    ///   -10d, -3d and -2h, and a skill load at -40d and at -2h;
    /// - `o1`, the orchestrator, with 1 call at -1h;
    /// - `gone`, a worker with no session id, last updated 20 days ago.
    fn fleet(root: &Path) -> (Locations, Vec<Record>) {
        let home = root.join("home");
        let dir = home.join(".claude/projects/-p");
        std::fs::create_dir_all(&dir).unwrap();
        let w1 = [
            claude_line("2026-08-28T10:00:00Z", "a1", 100, Some("horch:tdd")),
            claude_line("2026-09-27T10:00:00Z", "a2", 200, None),
            claude_line("2026-10-04T10:00:00Z", "a3", 300, None),
            claude_line("2026-10-07T08:00:00Z", "a4", 400, Some("horch:check")),
        ];
        std::fs::write(dir.join("sid-w1.jsonl"), w1.join("\n") + "\n").unwrap();
        let o1 = [claude_line("2026-10-07T09:00:00Z", "b1", 1000, None)];
        std::fs::write(dir.join("sid-o1.jsonl"), o1.join("\n") + "\n").unwrap();
        let rec =
            |id: &str, sid: Option<&str>, tier: &str, kind: &str, life: (&str, &str)| Record {
                record_id: id.into(),
                session_id: sid.map(str::to_owned),
                agent: "claude".into(),
                tier: tier.into(),
                model: "claude-opus-5-5".into(),
                effort: Some("medium".into()),
                phase: Some(Phase::Implementation),
                role: format!("{tier}-1"),
                status: "done".into(),
                kind: kind.into(),
                project: Some("/p".into()),
                created_at: life.0.into(),
                updated_at: life.1.into(),
                ..Record::default()
            };
        let records = vec![
            rec(
                "w1",
                Some("sid-w1"),
                "backend-developer",
                "worker",
                ("2026-08-28T09:59:00Z", "2026-10-07T08:00:00Z"),
            ),
            rec(
                "o1",
                Some("sid-o1"),
                "orchestrator",
                "orchestrator",
                ("2026-10-07T08:59:00Z", "2026-10-07T09:00:00Z"),
            ),
            rec(
                "gone",
                None,
                "researcher",
                "worker",
                ("2026-09-17T00:00:00Z", "2026-09-17T00:00:00Z"),
            ),
        ];
        let loc = Locations::under_home(&home, &horch_core::runtime::Inherited::default());
        (loc, records)
    }

    fn args() -> CostArgs {
        CostArgs {
            text: false,
            dir: None,
            reprice: None,
            pricing: None,
            since: None,
            records: Vec::new(),
            sessions: Vec::new(),
        }
    }

    fn now() -> DateTime<Utc> {
        clock::parse(NOW).unwrap()
    }

    fn prefix() -> String {
        now()
            .with_timezone(&Local)
            .format("%Y_%m_%d_%H%M%S")
            .to_string()
    }

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// No filter: 3 files named `<local prefix>_30d.json`, `_7d.json` and
    /// `_24h.json` under `<project>/ai_docs/reports/cost`, and a JSON
    /// summary on stdout. Each window counts calls by their own time: the
    /// worker that started 40 days ago is in the 24h report with only its
    /// last call.
    #[test]
    fn tel_13_no_filter_writes_3_window_files_that_count_calls_by_their_own_time() {
        let tmp = tempfile::tempdir().unwrap();
        let (loc, records) = fleet(tmp.path());
        let project = tmp.path().join("proj");
        let out = run(&args(), records, None, &loc, now(), &project, "ledger.json").unwrap();

        let summary: serde_json::Value = serde_json::from_str(&out).unwrap();
        let dir = project.join("ai_docs/reports/cost");
        for name in ["30d", "7d", "24h"] {
            let want = dir.join(format!("{}_{name}.json", prefix()));
            assert!(want.is_file(), "{}", want.display());
            assert_eq!(summary["files"][name], want.to_str().unwrap());
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        assert_eq!(summary["project"], project.to_str().unwrap());
        let generated = summary["generated_at"].as_str().unwrap();
        assert!(
            chrono::DateTime::parse_from_rfc3339(generated).is_ok(),
            "{generated}"
        );
        assert!(!generated.ends_with('Z') || Local::now().offset().local_minus_utc() == 0);

        // Calls per window: 24h = a4 + b1, 7d = a3 + a4 + b1, 30d = a2..a4 + b1.
        for (name, calls, sessions) in [("24h", 2, 2), ("7d", 3, 2), ("30d", 4, 2)] {
            let w = &summary["windows"][name];
            assert_eq!(w["calls"], calls, "{name}");
            assert_eq!(w["sessions"], sessions, "{name}");
            assert_eq!(w["end"], NOW, "{name}");
        }
        assert_eq!(summary["windows"]["24h"]["start"], "2026-10-06T10:00:00Z");
        assert_eq!(summary["windows"]["30d"]["start"], "2026-09-07T10:00:00Z");

        let day = read_json(&dir.join(format!("{}_24h.json", prefix())));
        assert_eq!(day["window"]["name"], "24h");
        assert_eq!(day["window"]["start"], "2026-10-06T10:00:00Z");
        assert_eq!(day["prices_as_of"], "2026-09-24");
        let w1 = day["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["record_id"] == "w1")
            .unwrap();
        assert_eq!(w1["calls"], 1);
        assert_eq!(w1["tokens"]["output"], 400);
        assert_eq!(w1["skills_used"], serde_json::json!({"check": 1}));
        // The worker with no session id last ran 20 days ago: listed in the
        // 30d report, not in the 24h one.
        assert_eq!(day["not_priced"].as_array().unwrap().len(), 0);
        let month = read_json(&dir.join(format!("{}_30d.json", prefix())));
        assert_eq!(month["not_priced"][0]["record_id"], "gone");
        assert_eq!(summary["windows"]["30d"]["unpriced_sessions"], 1);

        // The tables: tier (workers only) with an `all` line, the
        // orchestrator apart, per teammate with every session; means per
        // session.
        let tiers = day["tables"]["tiers"].as_array().unwrap();
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0]["key"], "claude claude-opus-5-5 medium");
        assert_eq!(tiers[1]["key"], "all");
        assert_eq!(tiers[1]["sessions"], 1);
        let orch = day["tables"]["orchestrator"].as_array().unwrap();
        assert_eq!(orch[0]["output"], 1000);
        let mates = month["tables"]["teammates"].as_array().unwrap();
        let keys: Vec<&str> = mates.iter().map(|t| t["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["backend-developer", "orchestrator"]);
        assert_eq!(mates[0]["calls"], 3);
        assert_eq!(mates[0]["output"], 900);
        assert_eq!(mates[0]["mean_output"], 900.0);
        assert_eq!(mates[0]["cache_write"], 150);
        for key in [
            "sessions",
            "calls",
            "input",
            "output",
            "cache_read",
            "cache_write",
            "cost",
            "mean_input",
            "mean_output",
            "mean_cache_read",
            "mean_cache_write",
            "mean_cost",
        ] {
            assert!(mates[0].get(key).is_some(), "{key}");
        }
        assert!(day["skill_use"].is_array());
    }

    /// Every key in a report and in the summary, at any depth.
    fn keys(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    out.push(k.clone());
                    keys(x, out);
                }
            }
            serde_json::Value::Array(xs) => xs.iter().for_each(|x| keys(x, out)),
            _ => {}
        }
    }

    /// The reports are informational: no field names a recommendation, a
    /// ranking or a choice, and the text says what the numbers are for.
    #[test]
    fn tel_15_no_report_field_recommends_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let (loc, records) = fleet(tmp.path());
        let project = tmp.path().join("proj");
        let roster = Roster::builtin().unwrap();
        let out = run(&args(), records, Some(&roster), &loc, now(), &project, "l").unwrap();
        let mut all = Vec::new();
        keys(&serde_json::from_str(&out).unwrap(), &mut all);
        let dir = project.join("ai_docs/reports/cost");
        for f in std::fs::read_dir(&dir).unwrap() {
            keys(&read_json(&f.unwrap().path()), &mut all);
        }
        assert!(all.contains(&"mean_cost".to_string()));
        for key in &all {
            for word in [
                "recommend",
                "should",
                "suggest",
                "best",
                "prefer",
                "rank",
                "advice",
            ] {
                assert!(!key.contains(word), "{key}");
            }
        }
        let a = CostArgs {
            since: Some("2026-10-01".into()),
            text: true,
            ..args()
        };
        let (loc, records) = fleet(tmp.path());
        let text = run(&a, records, None, &loc, now(), &project, "l").unwrap();
        assert!(
            text.contains("not an input to routing, model or teammate choice"),
            "{text}"
        );
    }

    /// `--text` prints the tables of each window and still writes the
    /// files; `--dir` moves them.
    #[test]
    fn tel_12_text_prints_the_tables_and_dir_moves_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (loc, records) = fleet(tmp.path());
        let project = tmp.path().join("proj");
        let dir = tmp.path().join("elsewhere");
        let a = CostArgs {
            text: true,
            dir: Some(dir.to_str().unwrap().into()),
            ..args()
        };
        let roster = Roster::builtin().unwrap();
        let out = run(
            &a,
            records,
            Some(&roster),
            &loc,
            now(),
            &project,
            "ledger.json",
        )
        .unwrap();
        assert!(!project.join("ai_docs").exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
        for name in ["30d", "7d", "24h"] {
            assert!(out.contains(&format!("# Fleet cost: {name}\n")), "{out}");
        }
        assert!(
            out.contains("## Per tier, workers only (orchestrator excluded)"),
            "{out}"
        );
        assert!(out.contains("## Orchestrator"), "{out}");
        assert!(out.contains("## Per teammate"), "{out}");
        assert!(out.contains("## Skill use"), "{out}");
        assert!(out.contains("Informational: spend telemetry"), "{out}");
        assert!(
            out.contains("| backend-developer | 1 | 1 | 2 | 400 | 1,000 | 50 | $"),
            "{out}"
        );
        // tdd was loaded 40 days ago, outside every window; check 2 hours ago.
        assert!(out.contains("tdd 0/1"), "{out}");
        assert!(!out.contains("tdd 1/1"), "{out}");
        assert!(out.contains("check 1/1"), "{out}");
    }

    /// A filter makes 1 ad-hoc report on stdout, the full JSON by default,
    /// and no file unless `--dir` is given. `--since` counts calls by their
    /// own time too.
    #[test]
    fn tel_14_a_filter_prints_one_report_and_writes_only_with_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (loc, records) = fleet(tmp.path());
        let project = tmp.path().join("proj");
        let a = CostArgs {
            since: Some("2026-10-04".into()),
            ..args()
        };
        let out = run(&a, records.clone(), None, &loc, now(), &project, "l").unwrap();
        assert!(!project.exists());
        let r: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(r["window"]["name"], "custom");
        assert_eq!(r["window"]["start"], "2026-10-04T00:00:00Z");
        assert_eq!(r["total"]["calls"], 3);
        assert_eq!(r["rows"][0]["tokens"]["output"], 700);

        let a = CostArgs {
            records: vec!["o1".into()],
            dir: Some(tmp.path().join("d").to_str().unwrap().into()),
            ..args()
        };
        let out = run(&a, records.clone(), None, &loc, now(), &project, "l").unwrap();
        let r: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(r["rows"].as_array().unwrap().len(), 1);
        assert_eq!(r["window"]["start"], serde_json::Value::Null);
        let file = tmp
            .path()
            .join("d")
            .join(format!("{}_custom.json", prefix()));
        assert_eq!(read_json(&file), r);

        let a = CostArgs {
            since: Some("last tuesday".into()),
            ..args()
        };
        let err = run(&a, records, None, &loc, now(), &project, "l").unwrap_err();
        assert!(err.to_string().contains("--since"), "{err}");
    }

    /// TEL-10 per window: the collector's stored events in the 24h span,
    /// summed per record, equal the 24h report's rows, class by class and
    /// cost to 1e-6 USD.
    #[test]
    fn tel_10_a_window_equals_horch_usage_over_the_same_range() {
        use horch_core::telemetry::collect::{Collector, Probing};
        use horch_core::telemetry::store;
        let tmp = tempfile::tempdir().unwrap();
        let (loc, records) = fleet(tmp.path());
        let state = tmp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(
            state.join("-p.json"),
            serde_json::to_string(&records).unwrap(),
        )
        .unwrap();
        Collector::open_at(&state, loc.clone(), Probing::Never, now())
            .unwrap()
            .tick(now())
            .unwrap();
        let events = store::read_all(&horch_core::telemetry::dir(&state));
        assert!(!events.is_empty());

        let prices = usage::builtin_prices();
        for (name, hours) in WINDOWS {
            let start = clock::stamp(now() - chrono::Duration::hours(hours));
            let span = Span::new(Some(&start), Some(NOW));
            let window = Window {
                name: name.into(),
                start: span.since.clone(),
                end: span.until.clone(),
            };
            let report = report(&read_all(&records, &loc), None, &prices, None, window);
            let mut want: BTreeMap<String, (Tokens, f64)> = BTreeMap::new();
            for e in events.iter().filter(|e| span.contains(&e.ts)) {
                let w = want.entry(e.record_id.clone()).or_default();
                w.0.add(&e.tokens.priced());
                w.1 += e.cost_usd.unwrap();
            }
            assert_eq!(report.rows.len(), want.len(), "{name}");
            for row in &report.rows {
                let (tokens, cost) = &want[&row.record_id];
                assert_eq!(&row.tokens, tokens, "{name} {}", row.record_id);
                assert!((row.cost - cost).abs() < 1e-6, "{name} {}", row.record_id);
            }
        }
    }
}
