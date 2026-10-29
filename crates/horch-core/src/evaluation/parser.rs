//! The strict judgment parser (JDG-05, SEC-06).
//!
//! The judge's output is untrusted until this parser accepts it. The parser
//! never repairs: a code fence, a trailing comma, prose around the object, a
//! duplicate key or an unknown field each reject the whole answer. A
//! rejected answer is an attempt that produced no judgment; the winner
//! policy then never promotes.
//!
//! The checks run in three passes, so that each failure gets its own
//! [`ParseError`] variant without reading serde's error messages:
//! 1. A `Node` tree keeps every object member, including duplicates,
//!    which `serde_json::Value` would silently drop.
//! 2. A shape walk over the `serde_json::Value` compares field names and
//!    enum spellings with the constants in [`super::judgment`].
//! 3. The typed [`Judgment`] is checked against the labels and the rubric.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use super::judgment::{
    Judgment, JudgmentVerdict, ASSESSMENT_FIELDS, JUDGMENT_FIELDS, JUDGMENT_OPTIONAL_FIELDS,
    JUDGMENT_SCHEMA_VERSION, VERDICTS,
};
use super::rubric::{components, SCORE_MAX, SCORE_MIN};

/// Why a judge answer was rejected. Each string names the JSON path, such
/// as `$.candidates.A.scores.tests`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not UTF-8, not exactly one JSON value, or a value of the wrong type.
    NotJson(String),
    UnknownField(String),
    UnknownEnum(String),
    MissingField(String),
    DuplicateKey(String),
    /// A label that is not in the round, missing, repeated, or a winner
    /// that does not match the verdict.
    ImpossibleLabel(String),
    /// A number that is not finite or is out of its range.
    NonFinite(String),
    /// The answer's size in bytes, which is over the cap.
    TooLarge(usize),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NotJson(why) => write!(f, "judgment is not valid JSON: {why}"),
            ParseError::UnknownField(path) => write!(f, "judgment has an unknown field: {path}"),
            ParseError::UnknownEnum(path) => write!(f, "judgment has an unknown value: {path}"),
            ParseError::MissingField(path) => write!(f, "judgment misses a field: {path}"),
            ParseError::DuplicateKey(path) => write!(f, "judgment repeats a key: {path}"),
            ParseError::ImpossibleLabel(why) => {
                write!(f, "judgment has an impossible label: {why}")
            }
            ParseError::NonFinite(path) => write!(f, "judgment has an out-of-range number: {path}"),
            ParseError::TooLarge(len) => write!(f, "judgment is too large: {len} bytes"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses the judge's raw answer. Strict: never repairs (no fence
/// stripping, no trailing-comma fix). `labels` are the round's candidate
/// labels; `cap_bytes` is the largest answer accepted.
pub fn parse_judgment(
    raw: &[u8],
    labels: &[String],
    cap_bytes: usize,
) -> Result<Judgment, ParseError> {
    let node = read_node(raw, cap_bytes)?;
    let value = node.into_value();
    check_shape(&value)?;
    // The shape walk has ruled out unknown fields, missing fields and unknown
    // enum values, so serde can only fail here on a wrong JSON type.
    let judgment: Judgment = serde_json::from_value(value)
        .map_err(|e| ParseError::NotJson(format!("wrong type: {e}")))?;
    check_numbers(&judgment)?;
    check_labels(&judgment, labels)?;
    Ok(judgment)
}

/// The size, UTF-8 and single-value checks, and the repeated-key check.
fn read_node(raw: &[u8], cap_bytes: usize) -> Result<Node, ParseError> {
    check_size(raw, cap_bytes)?;
    let text =
        std::str::from_utf8(raw).map_err(|e| ParseError::NotJson(format!("not UTF-8: {e}")))?;
    // `from_str` rejects anything but whitespace after the value.
    let node: Node = serde_json::from_str(text).map_err(|e| ParseError::NotJson(e.to_string()))?;
    check_no_duplicate(&node)?;
    Ok(node)
}

fn check_no_duplicate(node: &Node) -> Result<(), ParseError> {
    match node.duplicate_key("$") {
        Some(path) => Err(ParseError::DuplicateKey(path)),
        None => Ok(()),
    }
}

fn check_size(raw: &[u8], cap_bytes: usize) -> Result<(), ParseError> {
    if raw.len() > cap_bytes {
        Err(ParseError::TooLarge(raw.len()))
    } else {
        Ok(())
    }
}

/// A JSON value that keeps every object member in order, duplicates too.
enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    /// The path of the first repeated key, depth first.
    fn duplicate_key(&self, path: &str) -> Option<String> {
        match self {
            Node::Array(items) => items
                .iter()
                .enumerate()
                .find_map(|(i, item)| item.duplicate_key(&format!("{path}[{i}]"))),
            Node::Object(members) => {
                let mut seen = BTreeSet::new();
                for (key, _) in members {
                    if !seen.insert(key.as_str()) {
                        return Some(format!("{path}.{key}"));
                    }
                }
                members
                    .iter()
                    .find_map(|(key, value)| value.duplicate_key(&format!("{path}.{key}")))
            }
            _ => None,
        }
    }

    /// Only called after [`Node::duplicate_key`] found none, so no member
    /// is lost.
    fn into_value(self) -> Value {
        match self {
            Node::Null => Value::Null,
            Node::Bool(b) => Value::Bool(b),
            Node::Number(n) => Value::Number(n),
            Node::String(s) => Value::String(s),
            Node::Array(items) => Value::Array(items.into_iter().map(Node::into_value).collect()),
            Node::Object(members) => Value::Object(
                members
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_bool<E>(self, b: bool) -> Result<Node, E> {
        Ok(Node::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Node, E> {
        Ok(Node::Number(n.into()))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Node, E> {
        Ok(Node::Number(n.into()))
    }

    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Node, E> {
        Number::from_f64(n)
            .map(Node::Number)
            .ok_or_else(|| E::custom("a number is not finite"))
    }

    fn visit_str<E>(self, s: &str) -> Result<Node, E> {
        Ok(Node::String(s.to_string()))
    }

    fn visit_string<E>(self, s: String) -> Result<Node, E> {
        Ok(Node::String(s))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Node::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut members = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Node>()? {
            members.push((key, value));
        }
        Ok(Node::Object(members))
    }
}

/// Field names and enum spellings. Values of the wrong JSON type pass here
/// and fail in the typed step.
fn check_shape(value: &Value) -> Result<(), ParseError> {
    let Value::Object(top) = value else {
        return Err(ParseError::NotJson("expected an object at $".into()));
    };
    check_fields(top, "$", JUDGMENT_FIELDS, JUDGMENT_OPTIONAL_FIELDS)?;
    check_enum(top, "schema_version", &[JUDGMENT_SCHEMA_VERSION])?;
    check_enum(top, "verdict", VERDICTS)?;
    match top.get("candidates") {
        Some(Value::Object(candidates)) => check_assessments(candidates),
        _ => Ok(()),
    }
}

/// `key` holds a string that is one of `allowed`; any other type passes
/// here and fails in the typed step.
fn check_enum(top: &Map<String, Value>, key: &str, allowed: &[&str]) -> Result<(), ParseError> {
    match top.get(key) {
        Some(Value::String(v)) if !allowed.contains(&v.as_str()) => {
            Err(ParseError::UnknownEnum(format!("$.{key}: {v:?}")))
        }
        _ => Ok(()),
    }
}

fn check_assessments(candidates: &Map<String, Value>) -> Result<(), ParseError> {
    let components = components();
    candidates
        .iter()
        .filter_map(|(label, a)| a.as_object().map(|a| (label, a)))
        .try_for_each(|(label, assessment)| {
            let path = format!("$.candidates.{label}");
            check_fields(assessment, &path, ASSESSMENT_FIELDS, &[])?;
            match assessment.get("scores") {
                Some(Value::Object(scores)) => {
                    check_fields(scores, &format!("{path}.scores"), &components, &[])
                }
                _ => Ok(()),
            }
        })
}

fn check_fields(
    object: &Map<String, Value>,
    path: &str,
    known: &[&str],
    optional: &[&str],
) -> Result<(), ParseError> {
    if let Some(key) = object.keys().find(|k| !known.contains(&k.as_str())) {
        Err(ParseError::UnknownField(format!("{path}.{key}")))
    } else if let Some(field) = known
        .iter()
        .find(|f| !optional.contains(f) && !object.contains_key(**f))
    {
        Err(ParseError::MissingField(format!("{path}.{field}")))
    } else {
        Ok(())
    }
}

fn check_numbers(j: &Judgment) -> Result<(), ParseError> {
    let bad_score = j
        .candidates
        .iter()
        .flat_map(|(l, a)| a.scores.iter().map(move |(c, s)| (l, c, s)))
        .find(|(_, _, s)| !(s.is_finite() && (SCORE_MIN..=SCORE_MAX).contains(*s)));
    if !(j.confidence.is_finite() && (0.0..=1.0).contains(&j.confidence)) {
        Err(ParseError::NonFinite(format!(
            "$.confidence: {}",
            j.confidence
        )))
    } else if let Some((label, component, score)) = bad_score {
        Err(ParseError::NonFinite(format!(
            "$.candidates.{label}.scores.{component}: {score}"
        )))
    } else {
        Ok(())
    }
}

fn check_labels(j: &Judgment, labels: &[String]) -> Result<(), ParseError> {
    let expected: BTreeSet<&str> = labels.iter().map(String::as_str).collect();
    let assessed: BTreeSet<&str> = j.candidates.keys().map(String::as_str).collect();
    if assessed != expected {
        Err(ParseError::ImpossibleLabel(format!(
            "$.candidates has {assessed:?}, the round has {expected:?}"
        )))
    } else {
        check_ranking(&j.ranking, &expected)?;
        check_winner(j.verdict, j.winner.as_ref(), &expected)
    }
}

/// Every label exactly once.
fn check_ranking(ranking: &[String], expected: &BTreeSet<&str>) -> Result<(), ParseError> {
    let mut ranked = BTreeSet::new();
    ranking.iter().try_for_each(|label| {
        if !expected.contains(label.as_str()) {
            Err(ParseError::ImpossibleLabel(format!(
                "$.ranking: {label:?} is not a label"
            )))
        } else if !ranked.insert(label.as_str()) {
            Err(ParseError::ImpossibleLabel(format!(
                "$.ranking: {label:?} appears twice"
            )))
        } else {
            Ok(())
        }
    })?;
    if ranked.len() == expected.len() {
        Ok(())
    } else {
        Err(ParseError::ImpossibleLabel(
            "$.ranking misses a label".into(),
        ))
    }
}

/// A winner is named exactly when the verdict is `winner`, and it is a label.
fn check_winner(
    verdict: JudgmentVerdict,
    winner: Option<&String>,
    expected: &BTreeSet<&str>,
) -> Result<(), ParseError> {
    match (verdict, winner) {
        (JudgmentVerdict::Winner, Some(w)) if expected.contains(w.as_str()) => Ok(()),
        (JudgmentVerdict::Winner, Some(w)) => Err(ParseError::ImpossibleLabel(format!(
            "$.winner: {w:?} is not a label"
        ))),
        (JudgmentVerdict::Winner, None) => Err(ParseError::ImpossibleLabel(
            "$.winner is required when the verdict is winner".into(),
        )),
        (_, Some(w)) => Err(ParseError::ImpossibleLabel(format!(
            "$.winner: {w:?} is set but the verdict is not winner"
        ))),
        (_, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CAP: usize = 64 * 1024;

    fn labels() -> Vec<String> {
        vec!["A".into(), "B".into()]
    }

    fn scores(n: f64) -> Value {
        let map: Map<String, Value> = components()
            .into_iter()
            .map(|c| (c.to_string(), json!(n)))
            .collect();
        Value::Object(map)
    }

    fn valid() -> Value {
        json!({
            "schema_version": JUDGMENT_SCHEMA_VERSION,
            "verdict": "winner",
            "winner": "A",
            "ranking": ["A", "B"],
            "candidates": {
                "A": {"scores": scores(7.0), "acceptable": true, "notes": "n"},
                "B": {"scores": scores(5.0), "acceptable": true, "notes": "n"},
            },
            "confidence": 0.9,
            "rationale": "r",
        })
    }

    fn parse(v: &Value) -> Result<Judgment, ParseError> {
        parse_judgment(v.to_string().as_bytes(), &labels(), CAP)
    }

    fn message(v: &Value) -> String {
        parse(v).unwrap_err().to_string()
    }

    #[test]
    fn a_valid_answer_parses() {
        let j = parse(&valid()).unwrap();
        assert_eq!(j.winner.as_deref(), Some("A"));
        assert_eq!(j.ranking, ["A", "B"]);
    }

    #[test]
    fn size_utf8_and_json_errors_name_their_cause() {
        let raw = valid().to_string();
        assert_eq!(
            parse_judgment(raw.as_bytes(), &labels(), raw.len() - 1),
            Err(ParseError::TooLarge(raw.len()))
        );
        match parse_judgment(&[0xff, 0xfe], &labels(), CAP) {
            Err(ParseError::NotJson(why)) => assert!(why.starts_with("not UTF-8"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse_judgment(b"[]", &labels(), CAP),
            Err(ParseError::NotJson("expected an object at $".into()))
        );
        // The size cap is checked before the content is read.
        assert_eq!(
            parse_judgment(&[0xff; 10], &labels(), 5),
            Err(ParseError::TooLarge(10))
        );
    }

    #[test]
    fn a_repeated_key_is_named_by_its_path() {
        let raw = valid().to_string().replacen('{', r#"{"rationale":"x","#, 1);
        assert_eq!(
            parse_judgment(raw.as_bytes(), &labels(), CAP),
            Err(ParseError::DuplicateKey("$.rationale".into()))
        );
    }

    #[test]
    fn unknown_and_missing_fields_are_named_by_their_path() {
        let mut v = valid();
        v["extra"] = json!(1);
        assert_eq!(parse(&v), Err(ParseError::UnknownField("$.extra".into())));
        let mut v = valid();
        v["candidates"]["B"]["scores"]["style"] = json!(1);
        assert_eq!(
            parse(&v),
            Err(ParseError::UnknownField(
                "$.candidates.B.scores.style".into()
            ))
        );
        let mut v = valid();
        v.as_object_mut().unwrap().remove("rationale");
        assert_eq!(
            parse(&v),
            Err(ParseError::MissingField("$.rationale".into()))
        );
        // An unknown field is reported before a missing one.
        let mut v = valid();
        v["extra"] = json!(1);
        v.as_object_mut().unwrap().remove("rationale");
        assert_eq!(parse(&v), Err(ParseError::UnknownField("$.extra".into())));
        // `winner` may be left out of the object.
        let mut v = valid();
        v["verdict"] = json!("abstain");
        v.as_object_mut().unwrap().remove("winner");
        assert!(parse(&v).is_ok());
    }

    #[test]
    fn unknown_enum_values_are_named() {
        let mut v = valid();
        v["schema_version"] = json!("0.0.1");
        assert_eq!(
            parse(&v),
            Err(ParseError::UnknownEnum(
                "$.schema_version: \"0.0.1\"".into()
            ))
        );
        let mut v = valid();
        v["verdict"] = json!("draw");
        assert_eq!(
            parse(&v),
            Err(ParseError::UnknownEnum("$.verdict: \"draw\"".into()))
        );
    }

    #[test]
    fn a_score_or_confidence_out_of_range_is_named() {
        let mut v = valid();
        v["confidence"] = json!(1.5);
        assert_eq!(
            parse(&v),
            Err(ParseError::NonFinite("$.confidence: 1.5".into()))
        );
        let mut v = valid();
        v["candidates"]["B"]["scores"]["risk"] = json!(11.0);
        assert_eq!(
            parse(&v),
            Err(ParseError::NonFinite(
                "$.candidates.B.scores.risk: 11".into()
            ))
        );
        // The confidence is checked before the scores.
        v["confidence"] = json!(-1.0);
        assert_eq!(
            parse(&v),
            Err(ParseError::NonFinite("$.confidence: -1".into()))
        );
    }

    #[test]
    fn each_label_rule_has_its_own_message() {
        let mut v = valid();
        v["candidates"].as_object_mut().unwrap().remove("B");
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.candidates has {\"A\"}, the round has {\"A\", \"B\"}"
        );
        let mut v = valid();
        v["ranking"] = json!(["A", "C"]);
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.ranking: \"C\" is not a label"
        );
        let mut v = valid();
        v["ranking"] = json!(["A", "A"]);
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.ranking: \"A\" appears twice"
        );
        let mut v = valid();
        v["ranking"] = json!(["B"]);
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.ranking misses a label"
        );
        let mut v = valid();
        v["winner"] = json!("C");
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.winner: \"C\" is not a label"
        );
        let mut v = valid();
        v["winner"] = Value::Null;
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.winner is required when the verdict is winner"
        );
        let mut v = valid();
        v["verdict"] = json!("tie");
        assert_eq!(
            message(&v),
            "judgment has an impossible label: $.winner: \"A\" is set but the verdict is not winner"
        );
        let mut v = valid();
        v["verdict"] = json!("reject_all");
        v["winner"] = Value::Null;
        assert!(parse(&v).is_ok());
    }
}
