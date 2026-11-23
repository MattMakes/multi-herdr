use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use serde::Deserialize;

use super::HarnessKind;

pub const DEFAULT_DAYS: u32 = 7;

pub const NPM_REGISTRY: &str = "https://registry.npmjs.org";

pub fn npm_package(kind: HarnessKind) -> Option<&'static str> {
    match kind {
        HarnessKind::Claude => Some("@anthropic-ai/claude-code"),
        HarnessKind::Codex => Some("@openai/codex"),
        HarnessKind::OpenCode => Some("opencode-ai"),
        HarnessKind::Pi => Some("@earendil-works/pi-coding-agent"),
        HarnessKind::Antigravity | HarnessKind::Prime | HarnessKind::None => None,
    }
}

pub fn packument_url(registry: &str, package: &str) -> String {
    format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        package.replace('/', "%2f")
    )
}

#[derive(Debug, Default, Deserialize)]
pub struct Packument {
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: HashMap<String, String>,
    #[serde(default)]
    pub time: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clear,
    TooNew { version: String, age: TimeDelta },
    Unknown { reason: String },
}

pub fn judge(doc: &Packument, now: DateTime<Utc>, days: u32) -> Verdict {
    if let Some(version) = doc.dist_tags.get("latest") {
        if let Some(published) = publish_time(doc, version) {
            let age = now - published;
            if age >= TimeDelta::days(i64::from(days)) {
                Verdict::Clear
            } else {
                Verdict::TooNew {
                    version: version.clone(),
                    age,
                }
            }
        } else {
            Verdict::Unknown {
                reason: format!("the registry has no publish time for {version}"),
            }
        }
    } else {
        Verdict::Unknown {
            reason: "the registry names no latest release".into(),
        }
    }
}

fn publish_time(doc: &Packument, version: &str) -> Option<DateTime<Utc>> {
    doc.time
        .get(version)
        .and_then(serde_json::Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
}

pub fn age_text(age: TimeDelta) -> String {
    match age.num_days() {
        d if d < 1 => "less than 1 day old".into(),
        1 => "1 day old".into(),
        d => format!("{d} days old"),
    }
}

pub fn child_env(now: DateTime<Utc>, days: u32) -> Vec<(&'static str, String)> {
    let cutoff =
        (now - TimeDelta::days(i64::from(days))).to_rfc3339_opts(SecondsFormat::Secs, true);
    vec![
        ("npm_config_before", cutoff.clone()),
        (
            "PNPM_CONFIG_MINIMUM_RELEASE_AGE",
            (u64::from(days) * 24 * 60).to_string(),
        ),
        ("YARN_NPM_MINIMAL_AGE_GATE", format!("{days}d")),
        ("UV_EXCLUDE_NEWER", cutoff.clone()),
        ("PIP_UPLOADED_PRIOR_TO", cutoff),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn doc(json: &str) -> Packument {
        serde_json::from_str(json).unwrap()
    }

    const DOC: &str = r#"{
        "name": "x",
        "dist-tags": {"latest": "2.0.0", "next": "3.0.0-beta.1"},
        "versions": {"1.0.0": {"dist": {}}, "2.0.0": {"dist": {}}},
        "time": {
            "created": "2026-01-01T00:00:00.000Z",
            "modified": "2026-10-20T00:00:00.000Z",
            "1.0.0": "2026-01-01T00:00:00.000Z",
            "2.0.0": "2026-10-10T12:00:00.000Z",
            "3.0.0-beta.1": "2026-10-20T00:00:00.000Z"
        }
    }"#;

    #[test]
    fn arc_36_a_release_passes_only_at_the_cooldown_age() {
        let doc = doc(DOC);
        assert_eq!(judge(&doc, at("2026-10-17T12:00:00Z"), 7), Verdict::Clear);
        assert_eq!(judge(&doc, at("2026-10-30T00:00:00Z"), 7), Verdict::Clear);
        assert_eq!(
            judge(&doc, at("2026-10-17T11:59:59Z"), 7),
            Verdict::TooNew {
                version: "2.0.0".into(),
                age: TimeDelta::days(7) - TimeDelta::seconds(1),
            }
        );
        assert!(matches!(
            judge(&doc, at("2026-10-01T00:00:00Z"), 7),
            Verdict::TooNew { .. }
        ));
        assert_eq!(judge(&doc, at("2026-10-10T12:00:00Z"), 0), Verdict::Clear);
    }

    #[test]
    fn arc_36_a_release_without_a_date_does_not_pass() {
        let now = at("2030-01-01T00:00:00Z");
        let no_tag = doc(r#"{"time": {"1.0.0": "2026-01-01T00:00:00Z"}}"#);
        assert!(matches!(judge(&no_tag, now, 7), Verdict::Unknown { .. }));
        let no_time = doc(r#"{"dist-tags": {"latest": "1.0.0"}}"#);
        assert_eq!(
            judge(&no_time, now, 7),
            Verdict::Unknown {
                reason: "the registry has no publish time for 1.0.0".into()
            }
        );
        let bad_time = doc(r#"{"dist-tags": {"latest": "1.0.0"}, "time": {"1.0.0": 5}}"#);
        assert!(matches!(judge(&bad_time, now, 7), Verdict::Unknown { .. }));
    }

    #[test]
    fn arc_36_an_undated_release_says_why() {
        let now = at("2030-01-01T00:00:00Z");
        let unknown = |reason: &str| Verdict::Unknown {
            reason: reason.into(),
        };
        let no_latest = unknown("the registry names no latest release");

        let no_tag = doc(r#"{"dist-tags": {"next": "2.0.0-beta.1"},
                "time": {"1.0.0": "2026-01-01T00:00:00Z", "2.0.0-beta.1": "2026-01-02T00:00:00Z"}}"#);
        assert_eq!(judge(&no_tag, now, 7), no_latest);
        assert_eq!(judge(&Packument::default(), now, 7), no_latest);

        let missing =
            doc(r#"{"dist-tags": {"latest": "2.0.0"}, "time": {"1.0.0": "2026-01-01T00:00:00Z"}}"#);
        assert_eq!(
            judge(&missing, now, 7),
            unknown("the registry has no publish time for 2.0.0")
        );

        for bad in [
            serde_json::json!(5),
            serde_json::json!(null),
            serde_json::json!("yesterday"),
            serde_json::json!("2026-10-10"),
        ] {
            let json =
                serde_json::json!({"dist-tags": {"latest": "1.0.0"}, "time": {"1.0.0": bad}});
            assert_eq!(
                judge(&doc(&json.to_string()), now, 7),
                unknown("the registry has no publish time for 1.0.0"),
                "{bad}"
            );
        }
    }

    #[test]
    fn arc_36_the_age_reads_in_whole_days() {
        assert_eq!(age_text(TimeDelta::hours(5)), "less than 1 day old");
        assert_eq!(age_text(TimeDelta::hours(-5)), "less than 1 day old");
        assert_eq!(age_text(TimeDelta::hours(30)), "1 day old");
        assert_eq!(
            age_text(TimeDelta::days(6) + TimeDelta::hours(23)),
            "6 days old"
        );
    }

    #[test]
    fn arc_36_only_the_npm_published_clis_have_a_package() {
        assert_eq!(
            npm_package(HarnessKind::Claude),
            Some("@anthropic-ai/claude-code")
        );
        assert_eq!(npm_package(HarnessKind::Codex), Some("@openai/codex"));
        assert_eq!(npm_package(HarnessKind::OpenCode), Some("opencode-ai"));
        assert_eq!(
            npm_package(HarnessKind::Pi),
            Some("@earendil-works/pi-coding-agent")
        );
        assert_eq!(npm_package(HarnessKind::Antigravity), None);
        assert_eq!(npm_package(HarnessKind::Prime), None);
        assert_eq!(
            packument_url("https://registry.npmjs.org/", "@openai/codex"),
            "https://registry.npmjs.org/@openai%2fcodex"
        );
        assert_eq!(
            packument_url(NPM_REGISTRY, "opencode-ai"),
            "https://registry.npmjs.org/opencode-ai"
        );
    }

    #[test]
    fn arc_36_every_package_manager_gets_the_same_cutoff() {
        let env = child_env(at("2026-10-27T18:30:15.250Z"), 7);
        assert_eq!(
            env,
            [
                ("npm_config_before", "2026-10-20T18:30:15Z".to_string()),
                ("PNPM_CONFIG_MINIMUM_RELEASE_AGE", "10080".to_string()),
                ("YARN_NPM_MINIMAL_AGE_GATE", "7d".to_string()),
                ("UV_EXCLUDE_NEWER", "2026-10-20T18:30:15Z".to_string()),
                ("PIP_UPLOADED_PRIOR_TO", "2026-10-20T18:30:15Z".to_string()),
            ]
        );
    }
}
