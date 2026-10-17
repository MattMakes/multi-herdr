//! `horch update`, and the update step of `horch fleet` and
//! `horch orchestration` (ARC-32 to ARC-35).

use anyhow::Result;
use horch_core::harness::update::{self, Outcome, UpdateResult};
use horch_core::harness::HarnessKind;
use horch_core::runtime::RuntimeContext;

/// The names `horch update` takes: the 6 agent CLIs, then herdr.
pub const NAMES: [&str; 7] = [
    "claude",
    "codex",
    "opencode",
    "antigravity",
    "pi",
    "prime",
    "herdr",
];

/// The warning `horch update herdr` prints before herdr runs.
const HERDR_WARNING: &str = "horch: NOTE: herdr update can stop the herdr server. \
                             That closes every pane, a running fleet included.";

/// Update `harnesses` and print 1 line per result. Returns whether 1
/// updater failed or timed out.
pub fn run_step(ctx: &RuntimeContext, harnesses: &[HarnessKind]) -> bool {
    println!("Updating agent CLIs (horch fleet --no-update skips this)...");
    let results = update::update_all(ctx, harnesses);
    // A NOTE goes to stderr, as every other `horch: NOTE:` line does.
    for result in &results {
        if result.failed() {
            eprintln!("{}", line(result));
        } else {
            println!("{}", line(result));
        }
    }
    results.iter().any(UpdateResult::failed)
}

/// The update step of `horch fleet` and `horch orchestration`: every agent
/// CLI, unless `--no-update` or `HORCH_NO_UPDATE=1`. A failed updater never
/// fails the command.
pub fn before_fleet(ctx: &RuntimeContext, no_update: bool) {
    if no_update || ctx.settings.no_update {
        return;
    }
    run_step(ctx, &update::ORDER);
}

/// `horch update [NAME...]`: exit 1 when 1 updater failed or timed out.
/// `herdr` runs last, attached to the terminal, and its exit code wins.
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
        failed = run_step(ctx, &agents);
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

/// The line that reports 1 result.
pub fn line(result: &UpdateResult) -> String {
    let name = result.harness.as_str();
    match &result.outcome {
        Outcome::NotInstalled => format!("update: {name}: not installed (skipped)"),
        Outcome::Failed { reason } => format!("horch: NOTE: update: {name}: {reason}"),
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
    }
}
