//! Phase B4 judge logic: the strict parser, the winner policy, the rubric
//! and the judge policy digest (JDG-03, JDG-05, JDG-06, JDG-07, SEC-06).

use std::collections::{BTreeMap, BTreeSet};

use horch_core::evaluation::judgment::{
    CandidateAssessment, Judgment, JudgmentVerdict, JUDGMENT_FIELDS,
};
use horch_core::evaluation::parser::{parse_judgment, ParseError};
use horch_core::evaluation::rubric::{
    components, judge_policy_digest, rubric_text, schema_text, RUBRIC_VERSION,
};
use horch_core::evaluation::winner::{
    decide_winner, RejectReason, TieBreak, WinnerOutcome, WinnerPolicy,
};
use serde_json::{json, Value};

const CAP: usize = 1024 * 1024;

fn labels() -> Vec<String> {
    vec!["A".into(), "B".into()]
}

fn scores(n: f64) -> Value {
    json!({ "correctness": n, "tests": n, "scope": n, "maintainability": n, "risk": n })
}

/// A valid answer for labels A and B, with A the winner.
fn valid() -> Value {
    json!({
        "schema_version": "1.0.1",
        "verdict": "winner",
        "winner": "A",
        "ranking": ["A", "B"],
        "candidates": {
            "A": { "scores": scores(8.0), "acceptable": true, "notes": "good" },
            "B": { "scores": scores(5.0), "acceptable": true, "notes": "fine" }
        },
        "confidence": 0.9,
        "rationale": "A covers the edge case."
    })
}

fn parse(v: &Value) -> Result<Judgment, ParseError> {
    parse_judgment(v.to_string().as_bytes(), &labels(), CAP)
}

#[test]
fn a_valid_answer_parses() {
    let j = parse(&valid()).unwrap();
    assert_eq!(j.verdict, JudgmentVerdict::Winner);
    assert_eq!(j.winner.as_deref(), Some("A"));
    // Component scores are kept as given.
    assert_eq!(j.candidates["B"].scores["tests"], 5.0);
    // Surrounding whitespace is not a repair.
    let padded = format!("\n  {}  \n", valid());
    assert!(parse_judgment(padded.as_bytes(), &labels(), CAP).is_ok());
}

#[test]
fn jdg_03_policy_digest_changes_with_inputs() {
    let policy = WinnerPolicy::default();
    let base = [
        "judge prose",
        rubric_text(),
        schema_text(),
        "opus",
        "high",
        "labels-1",
    ];
    let digest = |p: &[&str; 6], policy: &WinnerPolicy| {
        judge_policy_digest(p[0], p[1], p[2], p[3], p[4], policy, p[5])
    };
    let d0 = digest(&base, &policy);
    assert_eq!(d0, digest(&base, &policy), "same inputs, same digest");

    let mut seen = BTreeSet::from([d0]);
    for i in 0..base.len() {
        let mut changed = base;
        let edited = format!("{}x", base[i]);
        changed[i] = &edited;
        assert!(
            seen.insert(digest(&changed, &policy)),
            "input {i} must change the digest"
        );
    }
    let stricter = WinnerPolicy {
        min_confidence: 0.8,
        ..WinnerPolicy::default()
    };
    assert!(seen.insert(digest(&base, &stricter)));
    let utility = WinnerPolicy {
        tie_break: TieBreak::Utility { v: "u1".into() },
        ..WinnerPolicy::default()
    };
    assert!(seen.insert(digest(&base, &utility)));

    // Length prefixes: moving a byte across an input boundary changes it.
    let a = judge_policy_digest("ab", "c", "s", "m", "e", &policy, "l");
    let b = judge_policy_digest("a", "bc", "s", "m", "e", &policy, "l");
    assert_ne!(a, b);
    assert_eq!(RUBRIC_VERSION, "rubric-2");
}

#[test]
fn jdg_05_unknown_enum() {
    let mut v = valid();
    v["verdict"] = json!("draw");
    assert!(matches!(parse(&v), Err(ParseError::UnknownEnum(p)) if p.starts_with("$.verdict")));

    let mut v = valid();
    v["schema_version"] = json!("2.0.0");
    assert!(
        matches!(parse(&v), Err(ParseError::UnknownEnum(p)) if p.starts_with("$.schema_version"))
    );
}

#[test]
fn jdg_05_unknown_field() {
    let mut v = valid();
    v["extra"] = json!(1);
    assert_eq!(parse(&v), Err(ParseError::UnknownField("$.extra".into())));

    let mut v = valid();
    v["candidates"]["A"]["scores"]["style"] = json!(3);
    assert_eq!(
        parse(&v),
        Err(ParseError::UnknownField(
            "$.candidates.A.scores.style".into()
        ))
    );
}

#[test]
fn jdg_05_missing_field() {
    let mut v = valid();
    v.as_object_mut().unwrap().remove("confidence");
    assert_eq!(
        parse(&v),
        Err(ParseError::MissingField("$.confidence".into()))
    );

    let mut v = valid();
    v["candidates"]["B"]
        .as_object_mut()
        .unwrap()
        .remove("notes");
    assert_eq!(
        parse(&v),
        Err(ParseError::MissingField("$.candidates.B.notes".into()))
    );

    let mut v = valid();
    v["candidates"]["A"]["scores"]
        .as_object_mut()
        .unwrap()
        .remove("risk");
    assert_eq!(
        parse(&v),
        Err(ParseError::MissingField(
            "$.candidates.A.scores.risk".into()
        ))
    );
}

#[test]
fn jdg_05_duplicate() {
    let top = valid()
        .to_string()
        .replacen('{', r#"{"rationale":"first","#, 1);
    assert_eq!(
        parse_judgment(top.as_bytes(), &labels(), CAP),
        Err(ParseError::DuplicateKey("$.rationale".into()))
    );

    let nested =
        valid()
            .to_string()
            .replacen(r#""correctness":"#, r#""tests":1.0,"correctness":"#, 1);
    assert_eq!(
        parse_judgment(nested.as_bytes(), &labels(), CAP),
        Err(ParseError::DuplicateKey(
            "$.candidates.A.scores.tests".into()
        ))
    );
}

#[test]
fn jdg_05_impossible_label() {
    let impossible = |v: &Value| matches!(parse(v), Err(ParseError::ImpossibleLabel(_)));

    let mut v = valid();
    v["winner"] = json!("C");
    assert!(impossible(&v), "winner is not a label");

    let mut v = valid();
    v["winner"] = Value::Null;
    assert!(impossible(&v), "winner verdict without a winner");

    let mut v = valid();
    v.as_object_mut().unwrap().remove("winner");
    assert!(impossible(&v), "winner verdict with the winner left out");

    let mut v = valid();
    v["verdict"] = json!("tie");
    assert!(impossible(&v), "a winner on a tie");

    let mut v = valid();
    v["ranking"] = json!(["A", "A"]);
    assert!(impossible(&v), "a label ranked twice");

    let mut v = valid();
    v["ranking"] = json!(["A"]);
    assert!(impossible(&v), "a label not ranked");

    let mut v = valid();
    v["ranking"] = json!(["A", "B", "C"]);
    assert!(impossible(&v), "an unknown label ranked");

    let mut v = valid();
    v["candidates"].as_object_mut().unwrap().remove("B");
    assert!(impossible(&v), "a label not assessed");

    let mut v = valid();
    v["candidates"]["C"] = v["candidates"]["A"].clone();
    assert!(impossible(&v), "an unknown label assessed");

    // A tie with no winner is fine; so is an explicit null.
    let mut v = valid();
    v["verdict"] = json!("tie");
    v["winner"] = Value::Null;
    assert!(parse(&v).is_ok());
}

#[test]
fn jdg_05_non_finite() {
    let non_finite = |v: &Value| matches!(parse(v), Err(ParseError::NonFinite(_)));

    for c in [1.5, -0.1] {
        let mut v = valid();
        v["confidence"] = json!(c);
        assert!(non_finite(&v), "confidence {c}");
    }
    for s in [10.5, -1.0] {
        let mut v = valid();
        v["candidates"]["B"]["scores"]["scope"] = json!(s);
        assert_eq!(
            parse(&v),
            Err(ParseError::NonFinite(format!(
                "$.candidates.B.scores.scope: {s}"
            )))
        );
    }
    // JSON has no NaN or Infinity, so those are not JSON at all.
    for bad in ["NaN", "Infinity", "1e400"] {
        let raw = valid()
            .to_string()
            .replace(r#""confidence":0.9"#, &format!(r#""confidence":{bad}"#));
        assert!(
            matches!(
                parse_judgment(raw.as_bytes(), &labels(), CAP),
                Err(ParseError::NotJson(_))
            ),
            "{bad}"
        );
    }
    // The bounds themselves are in range.
    let mut v = valid();
    v["confidence"] = json!(1.0);
    v["candidates"]["A"]["scores"] = scores(10.0);
    v["candidates"]["B"]["scores"] = scores(0.0);
    assert!(parse(&v).is_ok());
}

#[test]
fn jdg_05_malformed_no_promotion() {
    let fenced = format!("```json\n{}\n```", valid());
    let text = valid().to_string();
    let trailing_comma = format!("{},}}", &text[..text.len() - 1]);
    let prose = format!("Here is my answer: {}", valid());
    let two_values = format!("{} {}", valid(), valid());
    let eligible: BTreeSet<String> = labels().into_iter().collect();
    for raw in [fenced, trailing_comma, prose, two_values] {
        let parsed = parse_judgment(raw.as_bytes(), &labels(), CAP);
        assert!(matches!(parsed, Err(ParseError::NotJson(_))), "{raw}");
        let outcome = decide_winner(parsed.as_ref().ok(), &eligible, &WinnerPolicy::default());
        assert!(
            matches!(outcome, WinnerOutcome::NeedsIntervention { .. }),
            "{outcome:?}"
        );
    }
    assert!(matches!(
        parse_judgment(&[0xff, 0xfe], &labels(), CAP),
        Err(ParseError::NotJson(_))
    ));
    assert!(matches!(
        parse_judgment(b"[]", &labels(), CAP),
        Err(ParseError::NotJson(_))
    ));
    let mut v = valid();
    v["confidence"] = json!("high");
    assert!(matches!(parse(&v), Err(ParseError::NotJson(_))));
}

fn judgment(verdict: JudgmentVerdict, winner: Option<&str>, confidence: f64) -> Judgment {
    let assess = |n: f64, acceptable: bool| CandidateAssessment {
        scores: components()
            .into_iter()
            .map(|c| (c.to_string(), n))
            .collect(),
        acceptable,
        notes: String::new(),
    };
    Judgment {
        schema_version: "1.0.1".into(),
        verdict,
        winner: winner.map(str::to_string),
        ranking: vec!["A".into(), "B".into(), "C".into()],
        candidates: BTreeMap::from([
            ("A".to_string(), assess(6.0, true)),
            ("B".to_string(), assess(7.0, true)),
            ("C".to_string(), assess(9.0, false)),
        ]),
        confidence,
        rationale: String::new(),
    }
}

fn set(labels: &[&str]) -> BTreeSet<String> {
    labels.iter().map(|l| l.to_string()).collect()
}

fn needs(outcome: &WinnerOutcome) -> bool {
    matches!(outcome, WinnerOutcome::NeedsIntervention { .. })
}

#[test]
fn jdg_06_policy_table() {
    use JudgmentVerdict::*;
    let disabled = WinnerPolicy::default();
    let utility = WinnerPolicy {
        tie_break: TieBreak::Utility { v: "u1".into() },
        ..WinnerPolicy::default()
    };
    let all = set(&["A", "B", "C"]);
    let winner_a = judgment(Winner, Some("A"), 0.9);

    // eligible empty (judge skipped) → Rejected{NoEligible}
    assert_eq!(
        decide_winner(Some(&winner_a), &BTreeSet::new(), &disabled),
        WinnerOutcome::Rejected {
            reason: RejectReason::NoEligible
        }
    );
    assert_eq!(
        decide_winner(None, &BTreeSet::new(), &disabled),
        WinnerOutcome::Rejected {
            reason: RejectReason::NoEligible
        }
    );
    // no valid judgment after 2 attempts → NeedsIntervention
    assert!(needs(&decide_winner(None, &all, &disabled)));
    // verdict RejectAll → Rejected{JudgeRejected}
    assert_eq!(
        decide_winner(Some(&judgment(RejectAll, None, 0.9)), &all, &disabled),
        WinnerOutcome::Rejected {
            reason: RejectReason::JudgeRejected
        }
    );
    // verdict Abstain → NeedsIntervention
    assert!(needs(&decide_winner(
        Some(&judgment(Abstain, None, 0.9)),
        &all,
        &disabled
    )));
    // verdict Tie, tie_break: Disabled → NeedsIntervention
    let tie = judgment(Tie, None, 0.9);
    assert!(needs(&decide_winner(Some(&tie), &all, &disabled)));
    // verdict Tie, tie_break: Utility → winner by utility among the tied
    // labels. C has the highest sum but is not acceptable, so B wins.
    assert_eq!(
        decide_winner(Some(&tie), &all, &utility),
        WinnerOutcome::Winner { label: "B".into() }
    );
    // An ineligible label is not among the tied labels.
    assert_eq!(
        decide_winner(Some(&tie), &set(&["A", "C"]), &utility),
        WinnerOutcome::Winner { label: "A".into() }
    );
    // Equal sums go to the first label in label order.
    let mut even = tie.clone();
    even.candidates.get_mut("B").unwrap().scores = even.candidates["A"].scores.clone();
    assert_eq!(
        decide_winner(Some(&even), &all, &utility),
        WinnerOutcome::Winner { label: "A".into() }
    );
    // No acceptable eligible label to break the tie with.
    assert!(needs(&decide_winner(Some(&tie), &set(&["C"]), &utility)));
    // verdict Winner, label not in eligible → Rejected{JudgeRejected}
    assert_eq!(
        decide_winner(Some(&winner_a), &set(&["B", "C"]), &disabled),
        WinnerOutcome::Rejected {
            reason: RejectReason::JudgeRejected
        }
    );
    // verdict Winner, confidence < min_confidence → NeedsIntervention
    assert!(needs(&decide_winner(
        Some(&judgment(Winner, Some("A"), 0.69)),
        &all,
        &disabled
    )));
    // verdict Winner, eligible, confident → Winner (the bound is confident)
    assert_eq!(
        decide_winner(Some(&winner_a), &all, &disabled),
        WinnerOutcome::Winner { label: "A".into() }
    );
    assert_eq!(
        decide_winner(Some(&judgment(Winner, Some("A"), 0.7)), &all, &disabled),
        WinnerOutcome::Winner { label: "A".into() }
    );
    // The judgment's component scores are untouched by the decision.
    assert_eq!(winner_a.candidates["A"].scores.len(), components().len());
}

#[test]
fn jdg_07_tie_needs_intervention_by_default() {
    let policy = WinnerPolicy::default();
    assert_eq!(policy.tie_break, TieBreak::Disabled);
    assert_eq!(policy.min_confidence, 0.7);
    let tie = judgment(JudgmentVerdict::Tie, None, 1.0);
    let outcome = decide_winner(Some(&tie), &set(&["A", "B", "C"]), &policy);
    assert!(needs(&outcome), "{outcome:?}");
}

#[test]
fn sec_06_caps_enforced() {
    let raw = valid().to_string();
    let len = raw.len();
    assert!(parse_judgment(raw.as_bytes(), &labels(), len).is_ok());
    assert_eq!(
        parse_judgment(raw.as_bytes(), &labels(), len - 1),
        Err(ParseError::TooLarge(len))
    );
}

#[test]
fn judgment_schema_matches_type() {
    let schema: Value = serde_json::from_str(schema_text()).unwrap();
    let keys = |v: &Value| -> BTreeSet<String> { v.as_object().unwrap().keys().cloned().collect() };

    let serialized = serde_json::to_value(judgment(JudgmentVerdict::Tie, None, 0.5)).unwrap();
    let type_fields = keys(&serialized);
    assert_eq!(keys(&schema["properties"]), type_fields);
    assert_eq!(
        type_fields,
        JUDGMENT_FIELDS.iter().map(|f| f.to_string()).collect()
    );
    assert_eq!(schema["additionalProperties"], false);

    let assessment = &schema["$defs"]["assessment"];
    assert_eq!(
        keys(&assessment["properties"]),
        keys(&serialized["candidates"]["A"])
    );
    assert_eq!(assessment["additionalProperties"], false);
    let score_keys = keys(&assessment["properties"]["scores"]["properties"]);
    assert_eq!(
        score_keys,
        components().into_iter().map(str::to_string).collect()
    );

    let verdicts: BTreeSet<String> = schema["properties"]["verdict"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let spelled: BTreeSet<String> = [
        JudgmentVerdict::Winner,
        JudgmentVerdict::Tie,
        JudgmentVerdict::Abstain,
        JudgmentVerdict::RejectAll,
    ]
    .iter()
    .map(|v| {
        serde_json::to_value(v)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    assert_eq!(verdicts, spelled);
    assert_eq!(schema["properties"]["confidence"]["minimum"], 0);
    assert_eq!(schema["properties"]["confidence"]["maximum"], 1);
}

#[test]
fn reject_reason_serde_is_snake_case() {
    let all = [
        (RejectReason::NoEligible, "no_eligible"),
        (RejectReason::JudgeRejected, "judge_rejected"),
        (RejectReason::BelowConfidence, "below_confidence"),
        (RejectReason::Tie, "tie"),
        (RejectReason::StaleJudgment, "stale_judgment"),
        (RejectReason::RevalidationFailed, "revalidation_failed"),
    ];
    for (reason, spelled) in all {
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(spelled));
        assert_eq!(reason.to_string(), spelled);
    }
}
