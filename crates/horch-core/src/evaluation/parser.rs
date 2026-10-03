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
//! 1. A [`Node`] tree keeps every object member, including duplicates,
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
    if raw.len() > cap_bytes {
        return Err(ParseError::TooLarge(raw.len()));
    }
    let text =
        std::str::from_utf8(raw).map_err(|e| ParseError::NotJson(format!("not UTF-8: {e}")))?;
    // `from_str` rejects anything but whitespace after the value.
    let node: Node = serde_json::from_str(text).map_err(|e| ParseError::NotJson(e.to_string()))?;
    if let Some(path) = node.duplicate_key("$") {
        return Err(ParseError::DuplicateKey(path));
    }
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
    if let Some(Value::String(v)) = top.get("schema_version") {
        if v != JUDGMENT_SCHEMA_VERSION {
            return Err(ParseError::UnknownEnum(format!("$.schema_version: {v:?}")));
        }
    }
    if let Some(Value::String(v)) = top.get("verdict") {
        if !VERDICTS.contains(&v.as_str()) {
            return Err(ParseError::UnknownEnum(format!("$.verdict: {v:?}")));
        }
    }
    if let Some(Value::Object(candidates)) = top.get("candidates") {
        let components = components();
        for (label, assessment) in candidates {
            let path = format!("$.candidates.{label}");
            let Value::Object(assessment) = assessment else {
                continue;
            };
            check_fields(assessment, &path, ASSESSMENT_FIELDS, &[])?;
            if let Some(Value::Object(scores)) = assessment.get("scores") {
                check_fields(scores, &format!("{path}.scores"), &components, &[])?;
            }
        }
    }
    Ok(())
}

fn check_fields(
    object: &Map<String, Value>,
    path: &str,
    known: &[&str],
    optional: &[&str],
) -> Result<(), ParseError> {
    if let Some(key) = object.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(ParseError::UnknownField(format!("{path}.{key}")));
    }
    if let Some(field) = known
        .iter()
        .find(|f| !optional.contains(f) && !object.contains_key(**f))
    {
        return Err(ParseError::MissingField(format!("{path}.{field}")));
    }
    Ok(())
}

fn check_numbers(j: &Judgment) -> Result<(), ParseError> {
    if !(j.confidence.is_finite() && (0.0..=1.0).contains(&j.confidence)) {
        return Err(ParseError::NonFinite(format!(
            "$.confidence: {}",
            j.confidence
        )));
    }
    for (label, assessment) in &j.candidates {
        for (component, score) in &assessment.scores {
            if !(score.is_finite() && (SCORE_MIN..=SCORE_MAX).contains(score)) {
                return Err(ParseError::NonFinite(format!(
                    "$.candidates.{label}.scores.{component}: {score}"
                )));
            }
        }
    }
    Ok(())
}

fn check_labels(j: &Judgment, labels: &[String]) -> Result<(), ParseError> {
    let expected: BTreeSet<&str> = labels.iter().map(String::as_str).collect();
    let assessed: BTreeSet<&str> = j.candidates.keys().map(String::as_str).collect();
    if assessed != expected {
        return Err(ParseError::ImpossibleLabel(format!(
            "$.candidates has {assessed:?}, the round has {expected:?}"
        )));
    }
    let mut ranked = BTreeSet::new();
    for label in &j.ranking {
        if !expected.contains(label.as_str()) {
            return Err(ParseError::ImpossibleLabel(format!(
                "$.ranking: {label:?} is not a label"
            )));
        }
        if !ranked.insert(label.as_str()) {
            return Err(ParseError::ImpossibleLabel(format!(
                "$.ranking: {label:?} appears twice"
            )));
        }
    }
    if ranked.len() != expected.len() {
        return Err(ParseError::ImpossibleLabel(
            "$.ranking misses a label".into(),
        ));
    }
    match (j.verdict, &j.winner) {
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
