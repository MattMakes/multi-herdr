use serde_json::Value;
use std::path::Path;

const FALLBACKS: [(&str, &str); 4] = [
    ("sol", "gpt-6.2-sol"),
    ("astra", "gpt-6.2-astra"),
    ("terra", "gpt-6.1-terra"),
    ("luna", "gpt-6-luna"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub slug: String,
    pub alias: Option<String>,
    pub used_fallback: bool,
}

pub fn resolve_codex_model_from_bin(model: &str, codex_bin: &Path) -> ResolvedModel {
    let catalog = crate::harness::codex::model_catalog(codex_bin);
    match catalog.as_ref() {
        Ok(catalog) => resolve(model, Ok(catalog)),
        Err(_) => resolve(model, Err("catalog unavailable")),
    }
}

pub fn resolve_codex_model(model: &str, catalog: Result<&Value, &str>) -> String {
    resolve(model, catalog).slug
}

fn resolve(model: &str, catalog: Result<&Value, &str>) -> ResolvedModel {
    let Some((family, fallback)) = FALLBACKS.iter().find(|(family, _)| *family == model) else {
        return ResolvedModel {
            slug: model.to_string(),
            alias: None,
            used_fallback: false,
        };
    };
    let listed = catalog
        .ok()
        .and_then(models)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|entry| entry.get("slug").and_then(Value::as_str))
        .filter_map(|slug| version(slug, family).map(|version| (version, slug)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, slug)| slug.to_string());
    ResolvedModel {
        slug: listed.clone().unwrap_or_else(|| (*fallback).to_string()),
        alias: Some((*family).to_string()),
        used_fallback: listed.is_none(),
    }
}

fn models(catalog: &Value) -> Option<&Vec<Value>> {
    match catalog {
        Value::Object(root) => root.get("models")?.as_array(),
        Value::Array(models) => Some(models),
        _ => None,
    }
}

fn version(slug: &str, family: &str) -> Option<Vec<u64>> {
    let version = slug
        .strip_prefix("gpt-")?
        .strip_suffix(&format!("-{family}"))?;
    let numbers = version
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<u64>, _>>()
        .ok()?;
    (!numbers.is_empty()).then_some(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> Value {
        json!({"models": [
            {"slug": "gpt-6.1-sol", "visibility": "list"},
            {"slug": "gpt-6.2-sol", "visibility": "list"},
            {"slug": "gpt-6.10-sol", "visibility": "list"},
            {"slug": "gpt-20-sol", "visibility": "hide"},
            {"slug": "gpt-6.2-terra", "visibility": "hide"},
            {"slug": "gpt-6.1-terra", "visibility": "list"}
        ]})
    }

    #[test]
    fn alias_uses_the_highest_listed_numeric_version() {
        assert_eq!(resolve_codex_model("sol", Ok(&catalog())), "gpt-6.10-sol");
    }

    #[test]
    fn hidden_models_are_skipped() {
        assert_eq!(
            resolve_codex_model("terra", Ok(&catalog())),
            "gpt-6.1-terra"
        );
    }

    #[test]
    fn a_full_slug_passes_through() {
        assert_eq!(
            resolve_codex_model("gpt-6.1-sol", Err("unavailable")),
            "gpt-6.1-sol"
        );
    }

    #[test]
    fn catalog_failure_uses_the_family_fallback() {
        assert_eq!(
            resolve_codex_model("sol", Err("unavailable")),
            "gpt-6.2-sol"
        );
        assert_eq!(
            resolve_codex_model("astra", Err("unavailable")),
            "gpt-6.2-astra"
        );
        assert_eq!(
            resolve_codex_model("terra", Err("unavailable")),
            "gpt-6.1-terra"
        );
        assert_eq!(
            resolve_codex_model("luna", Err("unavailable")),
            "gpt-6-luna"
        );
    }

    #[test]
    fn catalog_without_a_listed_family_uses_the_fallback() {
        assert_eq!(resolve_codex_model("luna", Ok(&catalog())), "gpt-6-luna");
    }
}
