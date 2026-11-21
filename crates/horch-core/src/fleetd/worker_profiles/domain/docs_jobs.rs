pub const DAILY_S: i64 = 86_400;

const HORCH_WORD: &str = "horch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsJob {
    DocsSync,
    Conformance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Daily,
    NewHorch,
    NewCli,
    AfterDocsSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub job: DocsJob,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRun {
    pub id: i64,
    pub subject: String,
    pub input_key: String,
    pub started: Option<i64>,
    pub active: bool,
    pub has_result: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliFacts<'a> {
    pub binary_id: &'a str,
    pub conformance: bool,
    pub last_docs_sync: Option<&'a LastRun>,
    pub last_conformance: Option<&'a LastRun>,
}

pub fn subject(cli: &str, binary_id: &str) -> String {
    format!("{cli} {binary_id}")
}

pub fn input_key(subject: &str, horch_id: &str) -> String {
    format!("{subject} {HORCH_WORD} {horch_id}")
}

pub fn binary_of(subject: &str) -> Option<&str> {
    subject.split_whitespace().nth(1)
}

pub fn horch_of(input_key: &str) -> Option<&str> {
    let mut words = input_key.split_whitespace().skip(2);
    match (words.next(), words.next(), words.next()) {
        (Some(HORCH_WORD), Some(id), None) => Some(id),
        _ => None,
    }
}

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
