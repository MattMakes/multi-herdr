use horch_core::fleetd::task_execution::domain::judge_pair::{
    parse_verdict, Rank, VerdictError, VerdictWord,
};
use horch_core::fleetd::task_execution::domain::retry::Outcome;
use horch_core::fleetd::task_execution::domain::structural::{check_judge, Output, FAILED};

fn verdict_file(text: &str) -> Output {
    Output {
        path: "docs/sync.review.md".into(),
        text: Some(text.into()),
    }
}

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
    assert_eq!(
        v.findings[0].text,
        "The intro names 2 commands that do not exist."
    );
    assert_eq!(v.outcome(), None);
}

#[test]
fn verdict_without_a_rank_fails_the_structural_check() {
    let text = "## Verdict\n\npass\n\n## Findings\n\n- [LOW] A typo.\n- The intro is long.\n";
    let errors = parse_verdict(text).unwrap_err();
    assert_eq!(errors, [VerdictError::FindingWithoutRank { line: 8 }]);

    let c = check_judge(&[verdict_file(text)]);
    assert!(!c.passed());
    assert_eq!(
        c.failures,
        ["docs/sync.review.md: the finding at line 8 has no rank"]
    );
    assert!(c.reason().starts_with(FAILED), "{}", c.reason());
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
    assert_eq!(
        parse_verdict(twice).unwrap_err(),
        [VerdictError::VerdictWords { count: 2 }]
    );

    let c = check_judge(&[verdict_file(text)]);
    assert_eq!(
        c.failures,
        ["docs/sync.review.md: the verdict has 2 words, not 1"]
    );
}

#[test]
fn missing_verdict_file_fails_the_structural_check() {
    let c = check_judge(&[Output {
        path: "docs/sync.review.md".into(),
        text: None,
    }]);
    assert_eq!(c.failures, ["docs/sync.review.md: not at the tip"]);
    let c = check_judge(&[]);
    assert_eq!(c.failures, ["the commits change no file of Own"]);
}

#[test]
fn pass_verdict_passes_the_structural_check_with_no_outcome() {
    let c = check_judge(&[verdict_file(PASS)]);
    assert!(c.passed(), "{c:?}");
    assert_eq!(c.outcome, None);
    let no_findings = check_judge(&[verdict_file("## Verdict\n\npass\n\n## Findings\n")]);
    assert!(no_findings.passed(), "{no_findings:?}");
}

#[test]
fn file_with_no_verdict_fails() {
    let empty = "## Verdict\n\n## Findings\n\n- [LOW] A typo.\n";
    assert_eq!(
        parse_verdict(empty).unwrap_err(),
        [VerdictError::VerdictWords { count: 0 }]
    );

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
        [VerdictError::UnknownVerdict {
            word: "fine".into()
        }]
    );
}

#[test]
fn critical_item_gives_critical_open() {
    let v = parse_verdict(CRITICAL).expect("valid verdict");
    assert_eq!(v.word, VerdictWord::CriticalOpen);
    assert_eq!(v.findings[0].rank, Rank::Critical);
    assert_eq!(v.outcome(), Some(Outcome::CriticalOpen));

    let c = check_judge(&[verdict_file(CRITICAL)]);
    assert!(c.passed(), "{c:?}");
    assert_eq!(c.outcome, Some(Outcome::CriticalOpen));
}

#[test]
fn critical_item_with_a_pass_verdict_fails() {
    let text = "## Verdict\n\npass\n\n## Findings\n\n- [CRITICAL] Data loss.\n";
    assert_eq!(
        parse_verdict(text).unwrap_err(),
        [VerdictError::CriticalNotOpen]
    );
}

#[test]
fn critical_open_with_no_critical_item_fails() {
    let text = "## Verdict\n\ncritical_open\n\n## Findings\n\n- [HIGH] A gap.\n";
    assert_eq!(
        parse_verdict(text).unwrap_err(),
        [VerdictError::CriticalOpenWithoutCritical]
    );
}

#[test]
fn critical_open_burns_no_attempt() {
    let outcome = parse_verdict(CRITICAL)
        .unwrap()
        .outcome()
        .expect("an outcome");
    assert_eq!(outcome.as_str(), "critical_open");
    assert!(!outcome.burns_attempt());

    let outcome = check_judge(&[verdict_file(CRITICAL)])
        .outcome
        .expect("an outcome");
    assert!(!outcome.burns_attempt());
}
