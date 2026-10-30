//! `horch cost --text`: one report as markdown tables, in the shapes of
//! `ai_docs/reports/wave2/baseline.md`.

use horch_core::usage::Missing;

use super::tables::{SkillUse, TableRow};
use super::Report;

fn money(x: f64) -> String {
    format!("${x:.2}")
}

/// `1234567` as `1,234,567`.
fn commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn mean(x: f64) -> String {
    commas(x.round() as u64)
}

fn table(out: &mut String, title: &str, head: &str, rows: &[TableRow], reprice: Option<&str>) {
    let (rep_head, rep_sep) = match reprice {
        Some(m) => (format!(" | cost on {m}"), "|---:"),
        None => (String::new(), ""),
    };
    out.push_str(&format!(
        "\n## {title}\n\n| {head} | sessions | calls | input | output | cache read | cache write | cost | mean input | mean output | mean cache read | mean cache write | mean cost{rep_head} |\n\
         |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:{rep_sep}|\n"
    ));
    if rows.is_empty() {
        out.push_str("| (none) |");
        out.push_str(&" |".repeat(12 + usize::from(reprice.is_some())));
        out.push('\n');
    }
    for r in rows {
        let flag = if r.partly_priced_sessions > 0 {
            " *"
        } else {
            ""
        };
        let rep = match (reprice, r.reprice_cost) {
            (Some(_), Some(c)) => format!(" | {}", money(c)),
            (Some(_), None) => " | -".to_string(),
            _ => String::new(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {}{flag} | {} | {} | {} | {} | {}{rep} |\n",
            r.key,
            r.sessions,
            commas(r.calls),
            commas(r.input),
            commas(r.output),
            commas(r.cache_read),
            commas(r.cache_write),
            money(r.cost),
            mean(r.mean_input),
            mean(r.mean_output),
            mean(r.mean_cache_read),
            mean(r.mean_cache_write),
            money(r.mean_cost),
        ));
    }
}

fn skills(out: &mut String, use_: &[SkillUse]) {
    out.push_str(
        "\n## Skill use\n\nSessions that loaded each skill, out of the teammate's sessions.\n\n\
         | teammate | sessions | expected skills | other skills loaded |\n|---|---:|---|---|\n",
    );
    let list = |xs: &[super::tables::SkillCount], of: u64| {
        if xs.is_empty() {
            "-".to_string()
        } else {
            xs.iter()
                .map(|c| format!("{} {}/{of}", c.skill, c.sessions))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    for s in use_ {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            s.teammate,
            s.sessions,
            list(&s.expected, s.sessions),
            list(&s.other, s.sessions),
        ));
    }
}

pub fn render(report: &Report, ledger: &str) -> String {
    let w = &report.window;
    let range = match (&w.start, &w.end) {
        (Some(s), Some(e)) => format!("calls from {s} to {e}"),
        (Some(s), None) => format!("calls from {s}"),
        (None, Some(e)) => format!("calls before {e}"),
        (None, None) => "every call".to_string(),
    };
    let mut out = format!(
        "# Fleet cost: {}\n\nWindow: {range} (each call by its own time).\nLedger: {ledger}\n\
         Prices as of {} (USD per MTok).\n\
         Informational: spend telemetry to see where tokens go; not an input to routing, model or teammate choice.\n\
         \"input\" is uncached input. \"Mean\" is per session.\n",
        w.name, report.prices_as_of
    );
    let reprice = report.reprice.as_deref();
    table(
        &mut out,
        "Per tier, workers only (orchestrator excluded)",
        "tier (harness model effort)",
        &report.tables.tiers,
        reprice,
    );
    table(
        &mut out,
        "Orchestrator",
        "tier",
        &report.tables.orchestrator,
        reprice,
    );
    table(
        &mut out,
        "Per teammate",
        "teammate",
        &report.tables.teammates,
        reprice,
    );
    out.push_str(&format!(
        "\n**Total: {}**{} across {} priced session(s), {} calls.\n",
        money(report.total.cost),
        report
            .reprice
            .as_ref()
            .zip(report.total.reprice_cost)
            .map(|(m, v)| format!(" ({} on {m})", money(v)))
            .unwrap_or_default(),
        report.total.sessions,
        commas(report.total.calls),
    ));
    if !report.partly_priced.is_empty() {
        let names: Vec<String> = report
            .partly_priced
            .iter()
            .map(|p| format!("{} ({})", p.teammate, p.models.join(", ")))
            .collect();
        out.push_str(&format!(
            "\n\\* partly unpriced: no price for {}. Add them with --pricing.\n",
            names.join("; ")
        ));
    }
    if !report.not_priced.is_empty() {
        out.push_str("\n## Not priced\n\n");
        for n in &report.not_priced {
            let why = match &n.reason {
                Missing::NoSessionId => "the ledger never learned its session id".to_string(),
                Missing::NoTranscript => "no transcript found for its session id".to_string(),
                Missing::NotRead => {
                    "this harness's usage is not read here (agent none, or no sqlite3)".to_string()
                }
                Missing::Failed(why) => format!("could not be read: {why}"),
            };
            out.push_str(&format!(
                "- {} ({}, {}): {why}\n",
                n.teammate, n.agent, n.record_id
            ));
        }
    }
    skills(&mut out, &report.skill_use);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::cost::{NotPriced, PartlyPriced, Window};

    fn report(window: Window) -> Report {
        Report {
            window,
            prices_as_of: "2026-09-24",
            reprice: None,
            rows: Vec::new(),
            not_priced: Vec::new(),
            by_teammate: Default::default(),
            by_harness: Default::default(),
            by_family: Default::default(),
            total: Default::default(),
            tables: Default::default(),
            skill_use: Vec::new(),
            partly_priced: Vec::new(),
        }
    }

    fn window(start: Option<&str>, end: Option<&str>) -> Window {
        Window {
            name: "custom".into(),
            start: start.map(str::to_string),
            end: end.map(str::to_string),
        }
    }

    #[test]
    fn the_window_line_names_each_range() {
        for (start, end, want) in [
            (Some("A"), Some("B"), "Window: calls from A to B (each"),
            (Some("A"), None, "Window: calls from A (each"),
            (None, Some("B"), "Window: calls before B (each"),
            (None, None, "Window: every call (each"),
        ] {
            let text = render(&report(window(start, end)), "l.json");
            assert!(text.contains(want), "{want}: {text}");
        }
    }

    #[test]
    fn the_notes_and_totals_render_exactly() {
        let mut r = report(window(None, None));
        r.reprice = Some("opus".into());
        r.total.cost = 12.5;
        r.total.reprice_cost = Some(30.0);
        r.total.sessions = 2;
        r.total.calls = 1234;
        r.partly_priced = vec![
            PartlyPriced {
                record_id: "r1".into(),
                teammate: "a".into(),
                agent: "x".into(),
                models: vec!["m1".into(), "m2".into()],
            },
            PartlyPriced {
                record_id: "r2".into(),
                teammate: "b".into(),
                agent: "x".into(),
                models: vec!["m3".into()],
            },
        ];
        r.not_priced = [
            Missing::NoSessionId,
            Missing::NoTranscript,
            Missing::NotRead,
            Missing::Failed("bad".into()),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, reason)| NotPriced {
            record_id: format!("r{i}"),
            teammate: format!("t{i}"),
            agent: "claude".into(),
            reason,
        })
        .collect();
        let text = render(&r, "l.json");
        let tail = text.split("\n**Total").nth(1).unwrap();
        let (total, rest) = tail.split_once("\n\n").unwrap();
        assert_eq!(
            total,
            ": $12.50** ($30.00 on opus) across 2 priced session(s), 1,234 calls."
        );
        assert!(
            rest.starts_with(
                "\\* partly unpriced: no price for a (m1, m2); b (m3). Add them with --pricing.\n\n\
                 ## Not priced\n\n\
                 - t0 (claude, r0): the ledger never learned its session id\n\
                 - t1 (claude, r1): no transcript found for its session id\n\
                 - t2 (claude, r2): this harness's usage is not read here (agent none, or no sqlite3)\n\
                 - t3 (claude, r3): could not be read: bad\n\
                 \n## Skill use\n"
            ),
            "{rest}"
        );
        let plain = render(&report(window(None, None)), "l.json");
        assert!(
            plain.contains(
                "\n**Total: $0.00** across 0 priced session(s), 0 calls.\n\n## Skill use"
            ),
            "{plain}"
        );
        assert!(!plain.contains("partly unpriced") && !plain.contains("Not priced"));
    }

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(127095766), "127,095,766");
    }
}
