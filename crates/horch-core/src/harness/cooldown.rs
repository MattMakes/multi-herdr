//! The release cooldown of the agent CLI update step (ARC-36).
//!
//! A new release is the moment a hijacked package does its harm, and most
//! are caught and pulled within days. So the update step installs a release
//! only once it is [`DEFAULT_DAYS`] days old (`HORCH_UPDATE_COOLDOWN_DAYS`).
//!
//! The rule works at 2 levels:
//!
//! - The CLI itself. Before an updater runs, horch reads the publish time of
//!   the CLI's newest release from the npm registry. A release younger than
//!   the cooldown holds the updater back. So does a CLI whose release dates
//!   horch cannot read (Antigravity and Prime publish no npm package): an
//!   age that cannot be checked does not pass.
//! - What the updater installs with it. The updater gets the cooldown of
//!   each package manager that reads it from the environment ([`child_env`]),
//!   so npm, pnpm, yarn, uv and pip pick no dependency younger than the
//!   cutoff. bun reads its cooldown only from `bunfig.toml` or a flag, and
//!   Homebrew and the native installers have none.

use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use serde::Deserialize;

use super::HarnessKind;

/// The cooldown when `HORCH_UPDATE_COOLDOWN_DAYS` is unset or not a number.
pub const DEFAULT_DAYS: u32 = 7;

/// The registry the release dates come from.
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org";

/// The npm package each CLI publishes its releases as. The native and
/// Homebrew builds carry the same version numbers. `None`: horch knows no
/// public record of the CLI's release dates.
pub fn npm_package(kind: HarnessKind) -> Option<&'static str> {
    match kind {
        HarnessKind::Claude => Some("@anthropic-ai/claude-code"),
        HarnessKind::Codex => Some("@openai/codex"),
        HarnessKind::OpenCode => Some("opencode-ai"),
        HarnessKind::Pi => Some("@earendil-works/pi-coding-agent"),
        HarnessKind::Antigravity | HarnessKind::Prime | HarnessKind::None => None,
    }
}

/// The URL of `package`'s registry document under `registry`. A scoped
/// name keeps its `@` and has its `/` encoded, as the npm CLI sends it.
pub fn packument_url(registry: &str, package: &str) -> String {
    format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        package.replace('/', "%2f")
    )
}

/// The 2 fields of an npm registry document that the cooldown reads. Every
/// other field is skipped while the document is read.
#[derive(Debug, Default, Deserialize)]
pub struct Packument {
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: HashMap<String, String>,
    /// Version -> publish time (RFC 3339), plus `created` and `modified`.
    #[serde(default)]
    pub time: HashMap<String, serde_json::Value>,
}

/// Whether the cooldown lets a CLI's updater run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The newest release is at least the cooldown old.
    Clear,
    /// The newest release is younger than the cooldown.
    TooNew { version: String, age: TimeDelta },
    /// The registry document names no newest release or no publish time for
    /// it: why.
    Unknown { reason: String },
}

/// Judge the newest release (`dist-tags.latest`) of `doc` at `now`. A
/// publish time in the future is too new.
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

/// When `doc` says `version` was published: a time that reads as RFC 3339.
fn publish_time(doc: &Packument, version: &str) -> Option<DateTime<Utc>> {
    doc.time
        .get(version)
        .and_then(serde_json::Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
}

/// `1 day old`, `3 days old`, `less than 1 day old`.
pub fn age_text(age: TimeDelta) -> String {
    match age.num_days() {
        d if d < 1 => "less than 1 day old".into(),
        1 => "1 day old".into(),
        d => format!("{d} days old"),
    }
}

/// The variables that give each package manager an updater may run the
/// same cooldown, so a dependency it installs is as old as the CLI. Each one
/// replaces the operator's value for the updater only.
///
/// - npm `before`: every npm version. `min-release-age` exists only from npm
///   11.10, and a date from the same source wins over it anyway.
/// - pnpm `minimumReleaseAge`, in minutes.
/// - yarn `npmMinimalAgeGate`, a duration.
/// - uv `exclude-newer` and pip `--uploaded-prior-to`: the cutoff as a date,
///   which every uv version and every pip with the option read.
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
        // A publish time in the future is too new.
        assert!(matches!(
            judge(&doc, at("2026-10-01T00:00:00Z"), 7),
            Verdict::TooNew { .. }
        ));
        // 0 days lets any release through.
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

    /// The reason of every `Unknown` is shown to the operator as written.
    #[test]
    fn arc_36_an_undated_release_says_why() {
        let now = at("2030-01-01T00:00:00Z");
        let unknown = |reason: &str| Verdict::Unknown {
            reason: reason.into(),
        };
        let no_latest = unknown("the registry names no latest release");

        // No `latest` tag: another tag and its date do not stand in for it.
        let no_tag = doc(r#"{"dist-tags": {"next": "2.0.0-beta.1"},
                "time": {"1.0.0": "2026-01-01T00:00:00Z", "2.0.0-beta.1": "2026-01-02T00:00:00Z"}}"#);
        assert_eq!(judge(&no_tag, now, 7), no_latest);
        assert_eq!(judge(&Packument::default(), now, 7), no_latest);

        // `latest` names a version the time table does not list.
        let missing =
            doc(r#"{"dist-tags": {"latest": "2.0.0"}, "time": {"1.0.0": "2026-01-01T00:00:00Z"}}"#);
        assert_eq!(
            judge(&missing, now, 7),
            unknown("the registry has no publish time for 2.0.0")
        );

        // A time that is not a string, or not an RFC 3339 time, is no time.
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
