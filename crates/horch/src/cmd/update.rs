use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use anyhow::Result;
use flate2::read::GzDecoder;
use horch_core::harness::cooldown::{self, Packument};
use horch_core::harness::update::{self, Outcome, UpdateResult};
use horch_core::harness::HarnessKind;
use horch_core::runtime::RuntimeContext;

pub const NAMES: [&str; 7] = [
    "claude",
    "codex",
    "opencode",
    "antigravity",
    "pi",
    "prime",
    "herdr",
];

const REGISTRY_TIMEOUT: Duration = Duration::from_secs(60);

const MAX_PACKUMENT_BYTES: u64 = 256 * 1024 * 1024;

const HERDR_WARNING: &str = "horch: NOTE: herdr update can stop the herdr server. \
                             That closes every pane, a running fleet included.";

pub fn run_step(ctx: &RuntimeContext, harnesses: &[HarnessKind], skip_fresh: bool) -> bool {
    println!("Updating agent CLIs (horch fleet --no-update skips this)...");
    let results = update::update_all(ctx, harnesses, &npm_packument, skip_fresh);
    for result in &results {
        if result.is_note() {
            eprintln!("{}", line(result));
        } else {
            println!("{}", line(result));
        }
    }
    results.iter().any(UpdateResult::unfinished)
}

pub fn before_fleet(ctx: &RuntimeContext, no_update: bool) {
    super::selfupdate::update_before_fleet(ctx, no_update);
    if no_update || ctx.settings.no_update {
        return;
    }
    run_step(ctx, &update::ORDER, true);
}

pub fn update(ctx: &RuntimeContext, names: &[String]) -> Result<std::process::ExitCode> {
    let herdr = names.iter().any(|n| n == "herdr");
    let agents: Vec<HarnessKind> = if names.is_empty() {
        update::ORDER.to_vec()
    } else {
        update::ORDER
            .iter()
            .copied()
            .filter(|k| names.iter().any(|n| n == k.as_str()))
            .collect()
    };
    let mut failed = false;
    if !agents.is_empty() {
        failed = run_step(ctx, &agents, false);
    }
    if herdr {
        eprintln!("{HERDR_WARNING}");
        let status = update::run_herdr_update(ctx)?;
        if !status.success() {
            let code = status.code().unwrap_or(1).clamp(1, 255) as u8;
            return Ok(std::process::ExitCode::from(code));
        }
    }
    Ok(if failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    })
}

fn npm_registry() -> String {
    std::env::var("HORCH_NPM_REGISTRY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| cooldown::NPM_REGISTRY.to_string())
}

pub fn npm_packument(package: &str) -> Result<Packument, String> {
    let url = cooldown::packument_url(&npm_registry(), package);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REGISTRY_TIMEOUT))
        .build()
        .into();
    let response = agent
        .get(&url)
        .header(
            "User-Agent",
            &format!("horch/{}", super::selfupdate::CURRENT),
        )
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .call()
        .map_err(|e| format!("cannot reach {url}: {e}"))?;
    let raw = response.into_body().into_reader().take(MAX_PACKUMENT_BYTES);
    read_packument(raw).map_err(|e| format!("reading {url}: {e}"))
}

pub fn read_packument(raw: impl Read) -> std::io::Result<Packument> {
    let mut raw = BufReader::new(raw);
    let gzipped = raw.fill_buf()?.starts_with(&[0x1f, 0x8b]);
    let doc = if gzipped {
        let plain = GzDecoder::new(raw).take(MAX_PACKUMENT_BYTES);
        serde_json::from_reader(BufReader::new(plain))
    } else {
        serde_json::from_reader(raw)
    };
    doc.map_err(std::io::Error::other)
}

pub fn line(result: &UpdateResult) -> String {
    let name = result.harness.as_str();
    match &result.outcome {
        Outcome::NotInstalled => format!("update: {name}: not installed (skipped)"),
        Outcome::Failed { reason } => format!("horch: NOTE: update: {name}: {reason}"),
        Outcome::Busy => {
            format!("update: {name}: another horch is updating it (skipped)")
        }
        Outcome::Fresh { checked_at } => format!(
            "update: {name}: updated at {}, less than {} h ago (skipped)",
            horch_core::clock::stamp(*checked_at),
            update::FRESH_FOR.num_hours()
        ),
        Outcome::Detached { after } => format!(
            "horch: NOTE: update: {name}: still running after {} s, not waited for",
            after.as_secs()
        ),
        Outcome::Cooldown { version, age, days } => format!(
            "update: {name}: {version} is {}, under the {days}-day cooldown (skipped)",
            cooldown::age_text(*age)
        ),
        Outcome::NoReleaseDates { days } => format!(
            "update: {name}: no release dates to check against the {days}-day cooldown (skipped)"
        ),
        Outcome::Updated { before, after } => {
            let shown = |v: &Option<String>| v.clone().unwrap_or_else(|| "(no version)".into());
            if before == after {
                format!("update: {name}: {} (no change)", shown(after))
            } else {
                format!("update: {name}: {} -> {}", shown(before), shown(after))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(harness: HarnessKind, outcome: Outcome) -> UpdateResult {
        UpdateResult {
            harness,
            bin: None,
            outcome,
        }
    }

    #[test]
    fn arc_32_each_result_has_its_line() {
        let updated = |before: &str, after: &str| Outcome::Updated {
            before: Some(before.into()),
            after: Some(after.into()),
        };
        assert_eq!(
            line(&result(HarnessKind::Claude, updated("2.1.0", "2.2.0"))),
            "update: claude: 2.1.0 -> 2.2.0"
        );
        assert_eq!(
            line(&result(HarnessKind::Codex, updated("0.9", "0.9"))),
            "update: codex: 0.9 (no change)"
        );
        assert_eq!(
            line(&result(HarnessKind::Pi, Outcome::NotInstalled)),
            "update: pi: not installed (skipped)"
        );
        assert_eq!(
            line(&result(
                HarnessKind::OpenCode,
                Outcome::Failed {
                    reason: "timed out after 5 s".into()
                }
            )),
            "horch: NOTE: update: opencode: timed out after 5 s"
        );
        assert_eq!(
            line(&result(
                HarnessKind::Claude,
                Outcome::Cooldown {
                    version: "2.1.311".into(),
                    age: chrono::TimeDelta::hours(30),
                    days: 7,
                }
            )),
            "update: claude: 2.1.311 is 1 day old, under the 7-day cooldown (skipped)"
        );
        assert_eq!(
            line(&result(
                HarnessKind::Prime,
                Outcome::NoReleaseDates { days: 7 }
            )),
            "update: prime: no release dates to check against the 7-day cooldown (skipped)"
        );
        assert_eq!(
            line(&result(HarnessKind::Codex, Outcome::Busy)),
            "update: codex: another horch is updating it (skipped)"
        );
        let checked_at = chrono::DateTime::parse_from_rfc3339("2026-10-29T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(
            line(&result(HarnessKind::Claude, Outcome::Fresh { checked_at })),
            "update: claude: updated at 2026-10-29T08:00:00Z, less than 24 h ago (skipped)"
        );
        assert_eq!(
            line(&result(
                HarnessKind::OpenCode,
                Outcome::Detached {
                    after: Duration::from_secs(180)
                }
            )),
            "horch: NOTE: update: opencode: still running after 180 s, not waited for"
        );
    }

    #[test]
    fn arc_36_a_registry_document_reads_plain_or_gzipped() {
        use std::io::Write;
        let json = br#"{"name":"x","dist-tags":{"latest":"1.0.0"},
            "versions":{"1.0.0":{"big":"skipped"}},
            "time":{"1.0.0":"2026-10-01T00:00:00.000Z"}}"#;
        let plain = read_packument(&json[..]).unwrap();
        assert_eq!(plain.dist_tags["latest"], "1.0.0");

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(json).unwrap();
        let zipped = gz.finish().unwrap();
        let doc = read_packument(&zipped[..]).unwrap();
        assert_eq!(doc.dist_tags, plain.dist_tags);
        assert_eq!(doc.time, plain.time);

        assert!(read_packument(&b"<html>busy</html>"[..]).is_err());
        assert!(read_packument(&b""[..]).is_err());
    }
}
