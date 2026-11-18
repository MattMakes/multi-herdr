//! The verdict file of a judge task: 1 verdict, each finding ranked
//! CRITICAL, HIGH, MEDIUM or LOW, its structural check, and the outcome
//! critical_open that burns no attempt (docs/specs/fleetd/design.md, "Done
//! for each kind of task", Judge kind; RR-3, FM-33;
//! docs/tickets/fleetd-p6/39-judge-verdict.md, MAP.md Q18).

use horch_core::fleetd::task_execution::domain::judge_pair::{
    parse_verdict, Rank, VerdictError, VerdictWord,
};
use horch_core::fleetd::task_execution::domain::retry::Outcome;

const PASS: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [HIGH] The intro names 2 commands that do not exist.\n\
- [MEDIUM] Section 3 has no example.\n  It needs 1 for the flag.\n\
- [LOW] A typo in the title.\n";

const CRITICAL: &str = "## Verdict\n\ncritical_open\n\n## Findings\n\n\
- [CRITICAL] The sync order loses data on a crash.\n\
- [LOW] A typo.\n";

#[test]
fn pass_verdict_parses_with_its_ranked_findings() {
    let v = parse_verdict(PASS).expect("valid verdict");
    assert_eq!(v.word, VerdictWord::Pass);
    let ranks: Vec<Rank> = v.findings.iter().map(|f| f.rank).collect();
    assert_eq!(ranks, [Rank::High, Rank::Medium, Rank::Low]);
    assert_eq!(v.findings[0].text, "The intro names 2 commands that do not exist.");
    assert_eq!(v.outcome(), None);
}

#[test]
fn verdict_without_a_rank_fails_the_structural_check() {
    let text = "## Verdict\n\npass\n\n## Findings\n\n- [LOW] A typo.\n- The intro is long.\n";
    let errors = parse_verdict(text).unwrap_err();
    assert_eq!(errors, [VerdictError::FindingWithoutRank { line: 8 }]);
}

#[test]
fn unknown_rank_fails() {
    let text = "## Verdict\n\npass\n\n## Findings\n\n- [MAJOR] The intro is long.\n";
    let errors = parse_verdict(text).unwrap_err();
    assert_eq!(errors, [VerdictError::FindingWithoutRank { line: 7 }]);
}

#[test]
fn verdict_with_2_verdict_words_fails() {
    let text = "## Verdict\n\npass\ncritical_open\n\n## Findings\n\n- [CRITICAL] Data loss.\n";
    let errors = parse_verdict(text).unwrap_err();
    assert_eq!(errors, [VerdictError::VerdictWords { count: 2 }]);

    let twice = "## Verdict\n\npass\n\n## Verdict\n\npass\n";
    assert_eq!(parse_verdict(twice).unwrap_err(), [VerdictError::VerdictWords { count: 2 }]);
}

#[test]
fn file_with_no_verdict_fails() {
    let empty = "## Verdict\n\n## Findings\n\n- [LOW] A typo.\n";
    assert_eq!(parse_verdict(empty).unwrap_err(), [VerdictError::VerdictWords { count: 0 }]);

    let no_section = "# Review\n\n## Findings\n\n- [LOW] A typo.\n";
    assert_eq!(
        parse_verdict(no_section).unwrap_err(),
        [VerdictError::VerdictWords { count: 0 }]
    );
}

#[test]
fn unknown_verdict_word_fails() {
    let text = "## Verdict\n\nfine\n";
    assert_eq!(
        parse_verdict(text).unwrap_err(),
        [VerdictError::UnknownVerdict { word: "fine".into() }]
    );
}

#[test]
fn critical_item_gives_critical_open() {
    let v = parse_verdict(CRITICAL).expect("valid verdict");
    assert_eq!(v.word, VerdictWord::CriticalOpen);
    assert_eq!(v.findings[0].rank, Rank::Critical);
    assert_eq!(v.outcome(), Some(Outcome::CriticalOpen));
}

#[test]
fn critical_item_with_a_pass_verdict_fails() {
    let text = "## Verdict\n\npass\n\n## Findings\n\n- [CRITICAL] Data loss.\n";
    assert_eq!(parse_verdict(text).unwrap_err(), [VerdictError::CriticalNotOpen]);
}

#[test]
fn critical_open_with_no_critical_item_fails() {
    let text = "## Verdict\n\ncritical_open\n\n## Findings\n\n- [HIGH] A gap.\n";
    assert_eq!(parse_verdict(text).unwrap_err(), [VerdictError::CriticalOpenWithoutCritical]);
}

#[test]
fn critical_open_burns_no_attempt() {
    let outcome = parse_verdict(CRITICAL).unwrap().outcome().expect("an outcome");
    assert_eq!(outcome.as_str(), "critical_open");
    assert!(!outcome.burns_attempt());
}
