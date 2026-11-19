//! When fleetd queues a `docs_sync` or a `conformance` run for a CLI
//! (`docs/specs/fleetd/design.md`, "Docs sync and adapter conformance":
//! "Run it daily, on `horch self-update`, and whenever fleetd sees a CLI
//! binary whose version differs"; fleetd-p7 MAP.md Q8 and Q11).
//!
//! The rules, for 1 CLI:
//!
//! 1. `docs_sync` when no `docs_sync` run of the CLI started in the last
//!    [`DAILY_S`] seconds on the fleet clock.
//! 2. `docs_sync` when the horch binary id differs from the one of the last
//!    `docs_sync` run of the CLI (`horch self-update` restarts fleetd on the
//!    new binary, OR-6).
//! 3. `docs_sync` and `conformance` when the binary id of the CLI differs
//!    from the one of its last `conformance` run.
//! 4. `conformance` after each `docs_sync` run ends with a result.
//!
//! A CLI with no exec adapter (herdr) has no conformance suite (MAP.md
//! Q5): it gets no `conformance` run, and rule 3 reads the binary id of its
//! last `docs_sync` run.
//!
//! A kind with a queued or running run of the CLI gets no second run.
//!
//! The subject of a run is `<cli> <binary id>` (Q8). Its input key is the
//! subject, then `horch` and the horch binary id, so that rule 2 reads the
//! horch binary of the last run from its row.

/// The seconds between 2 daily `docs_sync` runs of 1 CLI (Q11).
pub const DAILY_S: i64 = 86_400;

/// The word in an input key before the horch binary id.
const HORCH_WORD: &str = "horch";

/// The kind of a docs job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsJob {
    DocsSync,
    Conformance,
}

/// Why a run is queued: the rule that asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Rule 1.
    Daily,
    /// Rule 2.
    NewHorch,
    /// Rule 3.
    NewCli,
    /// Rule 4.
    AfterDocsSync,
}

/// A run to queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub job: DocsJob,
    pub why: Why,
}

/// The last run of 1 kind of 1 CLI, as the rules read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRun {
    /// The row id. Ids grow, so a higher id is a later run.
    pub id: i64,
    pub subject: String,
    pub input_key: String,
    /// The start on the fleet clock, or `None` when it did not start.
    pub started: Option<i64>,
    /// Queued or running.
    pub active: bool,
    /// Ended `passed` or `failed`.
    pub has_result: bool,
}

/// The facts of 1 CLI at 1 pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliFacts<'a> {
    /// The binary id of the CLI now.
    pub binary_id: &'a str,
    /// Whether the CLI has a conformance suite: an exec adapter.
    pub conformance: bool,
    pub last_docs_sync: Option<&'a LastRun>,
    pub last_conformance: Option<&'a LastRun>,
}

/// The subject of a run of `cli` with the binary id `binary_id`.
pub fn subject(cli: &str, binary_id: &str) -> String {
    format!("{cli} {binary_id}")
}

/// The input key of a run of `subject` from the horch binary `horch_id`.
pub fn input_key(subject: &str, horch_id: &str) -> String {
    format!("{subject} {HORCH_WORD} {horch_id}")
}

/// The binary id in `subject`, or `None` when it has none.
pub fn binary_of(subject: &str) -> Option<&str> {
    subject.split_whitespace().nth(1)
}

/// The horch binary id in `input_key`, or `None` when it has none.
pub fn horch_of(input_key: &str) -> Option<&str> {
    let mut words = input_key.split_whitespace().skip(2);
    match (words.next(), words.next(), words.next()) {
        (Some(HORCH_WORD), Some(id), None) => Some(id),
        _ => None,
    }
}

/// The runs to queue for 1 CLI at `clock`, from the horch binary
/// `horch_id`. `docs_sync` comes first.
pub fn due(clock: i64, horch_id: &str, cli: &CliFacts<'_>) -> Vec<Due> {
    let docs = cli.last_docs_sync;
    let conf = cli.last_conformance;
    let last = if cli.conformance { conf } else { docs };
    let new_cli = last.is_none_or(|r| binary_of(&r.subject) != Some(cli.binary_id));
    let mut out = Vec::new();
    if !docs.is_some_and(|d| d.active) {
        let why = if new_cli {
            Some(Why::NewCli)
        } else if docs.is_none_or(|d| horch_of(&d.input_key) != Some(horch_id)) {
            Some(Why::NewHorch)
        } else if docs.is_none_or(|d| d.started.is_none_or(|s| clock - s >= DAILY_S)) {
            Some(Why::Daily)
        } else {
            None
        };
        if let Some(why) = why {
            out.push(Due {
                job: DocsJob::DocsSync,
                why,
            });
        }
    }
    if cli.conformance && !conf.is_some_and(|c| c.active) {
        let after_docs = docs.is_some_and(|d| d.has_result && conf.is_none_or(|c| c.id < d.id));
        let why = if new_cli {
            Some(Why::NewCli)
        } else if after_docs {
            Some(Why::AfterDocsSync)
        } else {
            None
        };
        if let Some(why) = why {
            out.push(Due {
                job: DocsJob::Conformance,
                why,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: i64, binary: &str, horch: &str) -> LastRun {
        let s = subject("claude", binary);
        LastRun {
            id,
            input_key: input_key(&s, horch),
            subject: s,
            started: Some(0),
            active: false,
            has_result: true,
        }
    }

    fn jobs(d: &[Due]) -> Vec<(DocsJob, Why)> {
        d.iter().map(|d| (d.job, d.why)).collect()
    }

    #[test]
    fn a_new_cli_gets_docs_sync_and_conformance() {
        let facts = CliFacts {
            binary_id: "b1",
            conformance: true,
            last_docs_sync: None,
            last_conformance: None,
        };
        assert_eq!(
            jobs(&due(0, "h1", &facts)),
            [
                (DocsJob::DocsSync, Why::NewCli),
                (DocsJob::Conformance, Why::NewCli)
            ]
        );
    }

    #[test]
    fn the_daily_rule_counts_from_the_start() {
        let docs = run(1, "b1", "h1");
        let conf = run(2, "b1", "h1");
        let facts = CliFacts {
            binary_id: "b1",
            conformance: true,
            last_docs_sync: Some(&docs),
            last_conformance: Some(&conf),
        };
        assert!(due(DAILY_S - 1, "h1", &facts).is_empty());
        assert_eq!(
            jobs(&due(DAILY_S, "h1", &facts)),
            [(DocsJob::DocsSync, Why::Daily)]
        );
    }

    #[test]
    fn an_old_input_key_with_no_horch_id_is_a_new_horch() {
        let mut docs = run(1, "b1", "h1");
        docs.input_key = "claude b1".into();
        let conf = run(2, "b1", "h1");
        let facts = CliFacts {
            binary_id: "b1",
            conformance: true,
            last_docs_sync: Some(&docs),
            last_conformance: Some(&conf),
        };
        assert_eq!(
            jobs(&due(0, "h1", &facts)),
            [(DocsJob::DocsSync, Why::NewHorch)]
        );
    }

    #[test]
    fn the_key_words_read_back() {
        let s = subject("codex", "abc");
        assert_eq!(binary_of(&s), Some("abc"));
        assert_eq!(horch_of(&input_key(&s, "h9")), Some("h9"));
        assert_eq!(horch_of("codex abc"), None);
    }

    #[test]
    fn a_cli_with_no_conformance_suite_gets_docs_sync_only() {
        let mut facts = CliFacts {
            binary_id: "b1",
            conformance: false,
            last_docs_sync: None,
            last_conformance: None,
        };
        assert_eq!(
            jobs(&due(0, "h1", &facts)),
            [(DocsJob::DocsSync, Why::NewCli)]
        );
        let docs = run(1, "b1", "h1");
        facts.last_docs_sync = Some(&docs);
        assert!(due(10, "h1", &facts).is_empty(), "no loop, no conformance");
        facts.binary_id = "b2";
        assert_eq!(
            jobs(&due(10, "h1", &facts)),
            [(DocsJob::DocsSync, Why::NewCli)]
        );
    }
}
