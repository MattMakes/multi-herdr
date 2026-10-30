//! `horch worktree triage apply`: the reply table is parsed by code, and a
//! bad line refuses the whole reply (#308, section 2).

use horch_core::taskledger::model::TriageDecision;
use horch_core::taskledger::triage::reply::{self, Decided};
use horch_core::taskledger::triage::Row;

/// Triage rows W1 `T7-cache` and W2 `lh-target-3`, from JSON as `--json`
/// prints them; W3 `loose` has a detached HEAD.
fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for (id, name, branch) in [
        ("W1", "T7-cache", Some("fleet-T7-cache")),
        ("W2", "lh-target-3", Some("lh-target-3")),
        ("W3", "loose", None),
    ] {
        let mut r = Row::for_test(id, name);
        r.branch = branch.map(str::to_string);
        rows.push(r);
    }
    rows
}

const HEAD: &str = "| ID | Worktree | Suggested | Decision | Direction |\n|----|----------|-----------|----------|-----------|\n";

fn parse(body: &str, pr: bool) -> Result<Vec<Decided>, Vec<String>> {
    reply::parse(&format!("{HEAD}{body}"), &rows(), pr).map_err(|e| e.problems)
}

#[test]
fn a_filled_table_parses_into_decisions() {
    let got = parse(
        "| W1 | T7-cache | FINISH | finish | Fix the flaky test, then land |\n| w2 | lh-target-3 | DELETE | DELETE | |\n",
        false,
    )
    .unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].id, "W1");
    assert_eq!(got[0].decision, TriageDecision::Finish);
    assert_eq!(
        got[0].direction.as_deref(),
        Some("Fix the flaky test, then land")
    );
    assert_eq!(got[0].line, 3);
    assert_eq!(got[1].id, "W2");
    assert_eq!(got[1].decision, TriageDecision::Delete);
    assert_eq!(got[1].direction, None);
}

#[test]
fn every_bad_line_is_named_and_the_whole_reply_is_refused() {
    let problems = parse(
        "| W9 | ghost | | KEEP | |\n| W1 | T7-cache | FINISH | | |\n| W2 | lh-target-3 | | SHRED | |\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        problems,
        vec![
            "line 3: unknown ID 'W9'; this triage has W1 to W3".to_string(),
            "line 4: W1 has no Decision; write one of LAND, FINISH, ARCHIVE, KEEP, DELETE"
                .to_string(),
            "line 5: 'SHRED' is not a decision; write one of LAND, FINISH, ARCHIVE, KEEP, DELETE"
                .to_string(),
        ]
    );
}

#[test]
fn a_finish_row_with_no_direction_is_refused_even_next_to_good_rows() {
    let problems = parse(
        "| W2 | lh-target-3 | | DELETE | |\n| W1 | T7-cache | | FINISH | |\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        problems,
        vec!["line 4: W1 FINISH needs a Direction".to_string()]
    );
}

#[test]
fn pr_is_refused_without_gh_or_a_remote_and_offered_with_both() {
    let body = "| W1 | T7-cache | | PR | Cache the triage facts |\n";
    assert_eq!(
        parse(body, false).unwrap_err(),
        vec!["line 3: PR is not offered here: gh or a git remote is missing".to_string()]
    );
    let got = parse(body, true).unwrap();
    assert_eq!(got[0].decision, TriageDecision::Pr);
    let problems = parse("| W1 | T7-cache | | PR | |\n", true).unwrap_err();
    assert_eq!(
        problems,
        vec!["line 3: W1 PR needs a Direction".to_string()]
    );
}

#[test]
fn a_renamed_row_a_duplicate_and_a_detached_land_are_refused() {
    let problems = parse(
        "| W1 | other | | KEEP | |\n| W2 | lh-target-3 | | KEEP | |\n| W2 | lh-target-3 | | DELETE | |\n| W3 | loose | | LAND | |\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        problems,
        vec![
            "line 3: W1 is 'T7-cache', not 'other'; run horch worktree triage again".to_string(),
            "line 5: W2 appears twice".to_string(),
            "line 6: W3 has no branch; choose ARCHIVE, DELETE or KEEP".to_string(),
        ]
    );
}

#[test]
fn a_reply_with_no_rows_is_refused() {
    let problems = reply::parse("thanks!\n", &rows(), false)
        .unwrap_err()
        .problems;
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].starts_with("the reply has no rows"),
        "{problems:?}"
    );
}

#[test]
fn the_template_parses_back_once_decisions_are_filled() {
    let template = reply::template(&rows());
    let filled = template
        .lines()
        .map(|l| match l {
            l if l.starts_with("| W") => {
                let mut c: Vec<String> = l.split('|').map(str::to_string).collect();
                c[4] = " KEEP ".into();
                c.join("|")
            }
            l => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let got = reply::parse(&filled, &rows(), false).unwrap();
    let ids: Vec<_> = got.iter().map(|d| (d.id.as_str(), d.decision)).collect();
    assert_eq!(
        ids,
        vec![
            ("W1", TriageDecision::Keep),
            ("W2", TriageDecision::Keep),
            ("W3", TriageDecision::Keep)
        ]
    );
}
