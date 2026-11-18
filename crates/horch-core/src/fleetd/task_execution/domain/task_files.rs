//! The content of `task.yaml` and `START.md` of an attempt (ticket 07), and
//! the SR-2 lint that refuses a secret in them.
//!
//! Facts in, text out. No I/O: the writer is `adapters/task_files.rs`.
//! SR-2 (`docs/specs/fleetd/design.md`): secrets reach agents only through
//! env vars named in the profile, never through task files. The lint
//! refuses a text that holds the value of an env var of the profile, the
//! value of a variable named in `FORBIDDEN_ENV`, or a line that assigns a
//! `*_KEY`, `*_TOKEN` or `*_SECRET`. A refusal names the rule and the
//! variable, never the value.

/// The facts of 1 attempt that its 2 task files show, from the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFacts {
    /// The short id, `T<seq>`.
    pub task: String,
    /// The UUID of the task.
    pub task_id: String,
    pub run: String,
    /// The attempt, `A<n>`.
    pub attempt: String,
    /// The persona of the attempt.
    pub profile: String,
    /// The absolute path of the worktree.
    pub worktree: String,
    pub branch: String,
    /// The ticket path.
    pub read_first: String,
    /// The ticket title.
    pub goal: String,
    pub check: Option<String>,
    /// `work_kind` of the task.
    pub kind: Option<String>,
    /// `ticket_snapshot` of the task: the head of `START.md`.
    pub ticket_snapshot: String,
    /// The section of a resolver or fix attempt (ticket 33), after the
    /// fixed part of `START.md`.
    pub extra: Option<String>,
    /// The real path of the handoff file of the last attempt, when it ended
    /// with a handoff and fleetd kept the path (ticket 29 of fleetd-p5, Q4).
    pub handoff: Option<String>,
    /// The idea of the task, `I-NNNN` (FR-12). `None` for a task with no
    /// idea.
    pub idea: Option<String>,
    /// The feature of the idea, `F-NNNN`, whose decisions file the worker
    /// reads (MAP.md, Q15).
    pub feature: Option<String>,
    /// The requirement ids of `task_covers` of the task.
    pub covers: Vec<String>,
    /// The verdict file of the judge, for a fix attempt of a judged task
    /// (ticket 41 of fleetd-p6).
    pub verdict: Option<String>,
}

/// The lease renewal period that `task.yaml` names.
pub const RENEW_EVERY_S: u64 = 10;

/// `task.yaml` of `f`, whose outbox is `report_to`. 1 key on each line.
/// `attempt` and `worktree` stay unquoted when they are plain. `read_first`
/// is the list of files that the worker reads first (RR-12).
pub fn render_task_yaml(f: &TaskFacts, report_to: &str, read_first: &[String]) -> String {
    let optional = |v: &Option<String>| v.as_deref().map_or("null".to_string(), scalar);
    [
        ("task", scalar(&f.task)),
        ("idea", optional(&f.idea)),
        ("task_id", scalar(&f.task_id)),
        ("run", scalar(&f.run)),
        ("attempt", scalar(&f.attempt)),
        ("profile", scalar(&f.profile)),
        ("worktree", scalar(&f.worktree)),
        ("branch", scalar(&f.branch)),
        ("read_first", list(read_first)),
        ("goal", scalar(&f.goal)),
        ("covers", list(&f.covers)),
        ("check", optional(&f.check)),
        ("kind", optional(&f.kind)),
        ("lease", format!("{{renew_every_s: {RENEW_EVERY_S}}}")),
        ("report_to", scalar(report_to)),
    ]
    .iter()
    .map(|(key, value)| format!("{key}: {value}\n"))
    .collect()
}

/// `START.md` of `f`, whose outbox is `report_to`: the ticket snapshot,
/// then the fixed part with the `read_first` files and the handoff file,
/// if any, then the extra section, if any.
pub fn render_start_md(f: &TaskFacts, report_to: &str, read_first: &[String]) -> String {
    let mut out = f.ticket_snapshot.clone();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "\n## Your attempt\n\n\
         - Worktree: `{}`\n\
         - Branch: `{}`\n\
         - Outbox: `{report_to}`\n\n\
         Work and commit in the worktree. \
         When the work is committed, run `horch result`. \
         To ask a question, send `QUESTION:`.\n",
        f.worktree, f.branch
    ));
    if !read_first.is_empty() {
        out.push_str("\nRead these files first:\n\n");
        for path in read_first {
            out.push_str(&format!("- `{path}`\n"));
        }
    }
    if let Some(handoff) = &f.handoff {
        out.push_str(&format!(
            "\nThe last attempt handed off. Read its handoff file first: `{handoff}`\n"
        ));
    }
    if let Some(extra) = &f.extra {
        out.push('\n');
        out.push_str(extra);
        if !extra.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// `value` as a YAML scalar: plain when it is safe, else double-quoted.
fn scalar(value: &str) -> String {
    if is_plain(value) {
        value.to_string()
    } else {
        quoted(value)
    }
}

/// `items` as a YAML flow list of scalars.
fn list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|i| scalar(i)).collect();
    format!("[{}]", items.join(", "))
}

/// Words that YAML reads as a bool or a null when plain.
const RESERVED: [&str; 11] = [
    "null", "true", "false", "yes", "no", "on", "off", "y", "n", "~", "",
];

/// A plain scalar that every YAML reader reads back as the same string:
/// it starts with a letter or `/`, and holds only letters, digits and
/// `_ . / - @ +`.
fn is_plain(value: &str) -> bool {
    let first_ok = value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '/');
    first_ok
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-@+".contains(c))
        && !RESERVED.contains(&value.to_ascii_lowercase().as_str())
}

/// `value` as a YAML double-quoted scalar.
fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Where the lint got a secret from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An env var of the profile.
    Profile,
    /// A variable named in `FORBIDDEN_ENV`.
    Forbidden,
}

/// 1 secret value that no task file may hold, with the name of its
/// variable. Its `Debug` hides the value.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub name: String,
    value: String,
    pub source: Source,
}

impl Secret {
    /// The value of the env var `name` of the profile.
    pub fn profile(name: &str, value: &str) -> Secret {
        Secret {
            name: name.to_string(),
            value: value.to_string(),
            source: Source::Profile,
        }
    }

    /// The value of the variable `name` of `FORBIDDEN_ENV`.
    pub fn forbidden(name: &str, value: &str) -> Secret {
        Secret {
            name: name.to_string(),
            value: value.to_string(),
            source: Source::Forbidden,
        }
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("name", &self.name)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// A value shorter than this is not looked for: an env var such as
/// `X=1` is a setting, and its value is in most texts.
pub const MIN_SECRET_LEN: usize = 8;

/// The suffixes of a variable whose assignment is refused.
const SECRET_SUFFIXES: [&str; 3] = ["_KEY=", "_TOKEN=", "_SECRET="];

/// The rule of SR-2 that refused a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sr2Rule {
    /// The text holds the value of an env var of the profile.
    ProfileEnvValue,
    /// The text holds the value of a variable named in `FORBIDDEN_ENV`.
    ForbiddenEnvValue,
    /// A line assigns a `*_KEY`, `*_TOKEN` or `*_SECRET`.
    SecretAssignment,
}

impl Sr2Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Sr2Rule::ProfileEnvValue => "the value of a profile env var",
            Sr2Rule::ForbiddenEnvValue => "the value of a FORBIDDEN_ENV variable",
            Sr2Rule::SecretAssignment => "a *_KEY=, *_TOKEN= or *_SECRET= line",
        }
    }
}

/// Why the lint refused a task file. It holds no secret value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sr2Refusal {
    /// The file, relative to the attempt folder.
    pub file: String,
    pub rule: Sr2Rule,
    /// The variable name of a value rule, or the line number of an
    /// assignment, as text.
    pub what: String,
}

impl std::fmt::Display for Sr2Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SR-2: {} holds {} ({})",
            self.file,
            self.rule.as_str(),
            self.what
        )
    }
}

/// Refuse `text` of the file `file` when it holds a secret of `secrets` or
/// a secret assignment line.
pub fn lint(file: &str, text: &str, secrets: &[Secret]) -> Result<(), Sr2Refusal> {
    let refusal = |rule, what: String| Sr2Refusal {
        file: file.to_string(),
        rule,
        what,
    };
    for s in secrets {
        if s.value.len() >= MIN_SECRET_LEN && text.contains(&s.value) {
            let rule = match s.source {
                Source::Profile => Sr2Rule::ProfileEnvValue,
                Source::Forbidden => Sr2Rule::ForbiddenEnvValue,
            };
            return Err(refusal(rule, s.name.clone()));
        }
    }
    for (n, line) in text.lines().enumerate() {
        if assigns_secret(line) {
            return Err(refusal(
                Sr2Rule::SecretAssignment,
                format!("line {}", n + 1),
            ));
        }
    }
    Ok(())
}

/// `line` holds `<NAME>_KEY=`, `<NAME>_TOKEN=` or `<NAME>_SECRET=`, with a
/// non-empty variable name before the suffix.
fn assigns_secret(line: &str) -> bool {
    SECRET_SUFFIXES.iter().any(|suffix| {
        line.match_indices(suffix).any(|(at, _)| {
            line[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_quoted_scalars() {
        assert_eq!(scalar("A2"), "A2");
        assert_eq!(scalar("/tmp/x/work-1"), "/tmp/x/work-1");
        assert_eq!(scalar("a: b"), "\"a: b\"");
        assert_eq!(scalar("yes"), "\"yes\"");
        assert_eq!(scalar("12"), "\"12\"");
        assert_eq!(scalar("say \"hi\"\n"), "\"say \\\"hi\\\"\\n\"");
    }

    #[test]
    fn assignment_needs_a_name_before_the_suffix() {
        assert!(assigns_secret("export A_KEY=1"));
        assert!(assigns_secret("x MY_TOKEN=abc"));
        assert!(!assigns_secret("_KEY=1"));
        assert!(!assigns_secret("set the API_KEY variable"));
        assert!(!assigns_secret("api_key=1"));
    }

    #[test]
    fn start_md_names_the_handoff_file_of_the_last_attempt() {
        let mut f = TaskFacts {
            task: "T1".into(),
            task_id: "t-1".into(),
            run: "r1".into(),
            attempt: "A2".into(),
            profile: "dev".into(),
            worktree: "/w".into(),
            branch: "b".into(),
            read_first: "tickets/t.md".into(),
            goal: "g".into(),
            check: None,
            kind: None,
            ticket_snapshot: "# t\n".into(),
            extra: None,
            handoff: None,
            idea: None,
            feature: None,
            covers: Vec::new(),
            verdict: None,
        };
        assert!(!render_start_md(&f, "/o/", &[]).contains("handoff"));
        f.handoff = Some("/fleet/tasks/t-1/handoff.md".into());
        assert!(render_start_md(&f, "/o/", &[])
            .contains("Read its handoff file first: `/fleet/tasks/t-1/handoff.md`"));
    }

    #[test]
    fn debug_of_a_secret_hides_its_value() {
        let shown = format!("{:?}", Secret::profile("GH_TOKEN", "ghp_value12345"));
        assert!(shown.contains("GH_TOKEN") && !shown.contains("ghp_value"));
    }
}
