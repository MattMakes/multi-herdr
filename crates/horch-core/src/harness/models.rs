#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelName<'a> {
    pub family: String,
    pub version: Vec<u64>,
    pub id: &'a str,
}

pub fn parse(id: &str) -> Option<ModelName<'_>> {
    let name = id.rsplit('/').next().unwrap_or(id);
    let mut words = Vec::new();
    let mut version = Vec::new();
    for (index, token) in name.split('-').enumerate() {
        if is_date_stamp(token) {
            continue;
        }
        match numbers(token) {
            Some(parts) => version.extend(parts),
            None if index > 0 => words.push(token.to_ascii_lowercase()),
            None => {}
        }
    }
    (!words.is_empty() && !version.is_empty()).then(|| ModelName {
        family: words.join("-"),
        version,
        id,
    })
}

fn is_date_stamp(token: &str) -> bool {
    token.len() == 8 && token.bytes().all(|b| b.is_ascii_digit())
}

fn numbers(token: &str) -> Option<Vec<u64>> {
    if token.is_empty() || token.len() > 6 {
        return None;
    }
    token.split('.').map(|part| part.parse().ok()).collect()
}

pub fn is_family(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'_')
}

pub fn family_of(model: &str) -> String {
    match parse(model) {
        Some(name) => name.family,
        None => model.to_ascii_lowercase(),
    }
}

pub fn newest<'a>(ids: impl IntoIterator<Item = &'a str>, family: &str) -> Option<&'a str> {
    ids.into_iter()
        .filter_map(parse)
        .filter(|name| name.family.eq_ignore_ascii_case(family))
        .max_by(|a, b| a.version.cmp(&b.version))
        .map(|name| name.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_id_splits_into_a_family_and_a_version() {
        for (id, family, version) in [
            ("gpt-5.6-sol", "sol", vec![5, 6]),
            ("gpt-6-astra", "astra", vec![6]),
            ("claude-opus-5-5", "opus", vec![5, 5]),
            ("claude-haiku-4-5", "haiku", vec![4, 5]),
            ("gemini-4-argon-high", "argon-high", vec![4]),
            ("anthropic/claude-opus-5-5", "opus", vec![5, 5]),
            ("GPT-6.1-Sol", "sol", vec![6, 1]),
            ("claude-opus-5-5-20260101", "opus", vec![5, 5]),
        ] {
            let name = parse(id).unwrap_or_else(|| panic!("{id} parses"));
            assert_eq!(
                (name.family.as_str(), name.version),
                (family, version),
                "{id}"
            );
        }
        for id in [
            "opus",
            "sol",
            "codex-auto-review",
            "gpt-5.6",
            "",
            "claude-opus-20260101",
        ] {
            assert_eq!(parse(id), None, "{id}");
        }
    }

    #[test]
    fn a_family_is_a_name_with_no_version() {
        for name in ["sol", "astra", "opus", "codex-auto-review"] {
            assert!(is_family(name), "{name}");
        }
        for name in [
            "gpt-5.6-sol",
            "claude-opus-5-5",
            "ollama/qwen3.8",
            "",
            "opus[1m]",
        ] {
            assert!(!is_family(name), "{name}");
        }
    }

    #[test]
    fn the_newest_of_a_family_is_the_highest_version() {
        let listed = [
            "gpt-5.6-sol",
            "gpt-6.1-sol",
            "gpt-6-sol",
            "gpt-10-sol-mini",
            "gpt-6-astra",
            "gpt-5.10-terra",
            "gpt-5.9-terra",
            "codex-auto-review",
        ];
        assert_eq!(newest(listed, "sol"), Some("gpt-6.1-sol"));
        assert_eq!(newest(listed, "SOL"), Some("gpt-6.1-sol"));
        assert_eq!(newest(listed, "astra"), Some("gpt-6-astra"));
        assert_eq!(newest(listed, "terra"), Some("gpt-5.10-terra"));
        assert_eq!(newest(listed, "luna"), None);
    }

    #[test]
    fn the_family_of_a_plain_name_is_the_name() {
        assert_eq!(family_of("gpt-6.3-sol"), "sol");
        assert_eq!(family_of("claude-opus-5-6"), "opus");
        assert_eq!(family_of("Opus"), "opus");
        assert_eq!(family_of("sol"), "sol");
    }
}
