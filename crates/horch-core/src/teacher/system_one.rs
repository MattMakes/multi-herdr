//! Serde types for the System One API (`POST /v1/systemone`). There is no HTTP
//! client in this crate; these types only fix the wire shape. They belong to
//! the OD4 seam (`docs/specs/dataset-competition.md` §1.3) and have no caller outside `teacher` yet.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const API: &str = "systemone/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub api: String,
    pub model: String,
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, Question>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    #[serde(rename = "type")]
    pub kind: QuestionKind,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Opaque, passed through unchanged; horch never reads it and writes
    /// `None` in every exported question. Dataset design §4.9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionKind {
    Choice,
    Score,
    Noul,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub choice: Option<String>,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teacher::inert::Inert;
    use crate::teacher::{DecisionModel, TeacherRef};
    use serde_json::json;

    fn request() -> DecisionRequest {
        let mut questions = BTreeMap::new();
        questions.insert(
            "pick".to_string(),
            Question {
                kind: QuestionKind::Choice,
                instructions: "Pick a harness.".to_string(),
                options: vec!["claude".to_string(), "codex".to_string()],
                criteria: None,
            },
        );
        questions.insert(
            "rate".to_string(),
            Question {
                kind: QuestionKind::Score,
                instructions: "Rate it.".to_string(),
                options: vec![],
                criteria: Some(json!({"scale": 5})),
            },
        );
        questions.insert(
            "other".to_string(),
            Question {
                kind: QuestionKind::Noul,
                instructions: "Anything else?".to_string(),
                options: vec![],
                criteria: None,
            },
        );
        DecisionRequest {
            api: API.to_string(),
            model: "laya".to_string(),
            state: json!({"task": "x"}),
            questions,
        }
    }

    #[test]
    fn exp_01_inert_returns_none() {
        let inert = Inert;
        assert_eq!(inert.id(), "none");
        assert!(inert.decide(&request()).is_none());
    }

    #[test]
    fn exp_01_system_one_serde_shape() {
        let req = request();
        let value = serde_json::to_value(&req).unwrap();
        assert_eq!(value["api"], "systemone/v1");
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["api", "model", "questions", "state"]);
        assert_eq!(value["questions"]["pick"]["type"], "choice");
        assert_eq!(value["questions"]["rate"]["type"], "score");
        assert_eq!(value["questions"]["other"]["type"], "noul");
        assert!(value["questions"]["other"].get("options").is_none());
        assert!(value["questions"]["other"].get("criteria").is_none());
        assert_eq!(value["questions"]["rate"]["criteria"], json!({"scale": 5}));
        let back: DecisionRequest = serde_json::from_value(value).unwrap();
        assert_eq!(back, req);

        let mut probabilities = BTreeMap::new();
        probabilities.insert("claude".to_string(), 0.75);
        probabilities.insert("codex".to_string(), 0.25);
        let mut answers = BTreeMap::new();
        answers.insert(
            "pick".to_string(),
            Answer {
                choice: Some("claude".to_string()),
                probabilities,
                confidence: Some(0.75),
            },
        );
        let resp = DecisionResponse {
            answers,
            usage: None,
        };
        let value = serde_json::to_value(&resp).unwrap();
        let answer = value["answers"]["pick"].as_object().unwrap();
        let keys: Vec<&str> = answer.keys().map(String::as_str).collect();
        assert_eq!(keys, ["choice", "confidence", "probabilities"]);
        let back: DecisionResponse = serde_json::from_value(value).unwrap();
        assert_eq!(back, resp);
        let no_usage: DecisionResponse = serde_json::from_value(json!({"answers": {}})).unwrap();
        assert!(no_usage.usage.is_none());

        assert_eq!(
            serde_json::to_string(&TeacherRef::none()).unwrap(),
            r#"{"id":"none","probabilities":null}"#
        );
    }

    #[test]
    fn exp_01_teacher_has_no_http() {
        let sources = [
            include_str!("mod.rs"),
            include_str!("inert.rs"),
            include_str!("system_one.rs"),
        ];
        // Split the words so this test does not match itself.
        let banned = [
            ["ht", "tp"].concat(),
            ["req", "west"].concat(),
            ["u", "reg"].concat(),
            ["Tcp", "Stream"].concat(),
            ["cu", "rl"].concat(),
        ];
        for source in sources {
            let code = source.split("#[cfg(test)]").next().unwrap();
            for word in &banned {
                assert!(
                    !code.contains(word.as_str()),
                    "found {word} in teacher source"
                );
            }
        }
    }
}
