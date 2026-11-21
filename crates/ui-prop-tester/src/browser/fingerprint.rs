use serde_json::Value;

use crate::driver::{default_match_recorded, Action};

pub const FINGERPRINT_KEYS: [&str; 10] = [
    "test_id",
    "id",
    "role",
    "name",
    "href",
    "form_name",
    "placeholder",
    "input_type",
    "text",
    "path",
];

pub fn match_recorded_click(recorded: &Action, offered: &[Action]) -> Option<Action> {
    let kind = recorded.get("kind").and_then(Value::as_str)?;
    if kind != "click" && kind != "double_click" {
        return default_match_recorded(recorded, offered);
    }
    let target = recorded.get("target")?;
    let candidates: Vec<&Action> = offered
        .iter()
        .filter(|candidate| candidate.get("kind").and_then(Value::as_str) == Some(kind))
        .filter(|candidate| candidate.get("target").is_some_and(Value::is_object))
        .collect();
    for key in FINGERPRINT_KEYS {
        let Some(wanted) = target.get(key) else {
            continue;
        };
        let mut best: Option<(&Action, usize)> = None;
        for candidate in &candidates {
            let offered_target = &candidate["target"];
            if offered_target.get(key) != Some(wanted) {
                continue;
            }
            let agreement = FINGERPRINT_KEYS
                .iter()
                .filter(|other| {
                    target
                        .get(**other)
                        .is_some_and(|value| offered_target.get(**other) == Some(value))
                })
                .count();
            if best.is_none_or(|(_, score)| agreement > score) {
                best = Some((candidate, agreement));
            }
        }
        if let Some((candidate, _)) = best {
            let mut matched = recorded.clone();
            matched["point"] = candidate.get("point").cloned().unwrap_or(Value::Null);
            return Some(matched);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn earliest_agreeing_key_wins_and_takes_the_current_point() {
        let recorded = json!({"kind": "click", "target": {"id": "save", "role": "button", "text": "Save"}, "point": {"x": 1, "y": 1}});
        let offered = vec![
            json!({"kind": "click", "target": {"role": "button", "text": "Save"}, "point": {"x": 5, "y": 5}}),
            json!({"kind": "click", "target": {"id": "save", "role": "button", "text": "Store"}, "point": {"x": 9, "y": 40}}),
        ];
        let matched = match_recorded_click(&recorded, &offered).unwrap();
        assert_eq!(matched["point"], json!({"x": 9, "y": 40}));
        assert_eq!(matched["target"], recorded["target"]);
        let gone = json!({"kind": "click", "target": {"id": "missing"}});
        assert!(match_recorded_click(&gone, &offered).is_none());
        let typing = json!({"kind": "type", "text": "a"});
        assert!(match_recorded_click(&typing, &[json!({"kind": "type", "text": "b"})]).is_some());
    }
}
