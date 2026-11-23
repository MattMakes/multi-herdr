#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFacts {
    pub task: String,
    pub task_id: String,
    pub run: String,
    pub attempt: String,
    pub profile: String,
    pub worktree: String,
    pub branch: String,
    pub read_first: String,
    pub goal: String,
    pub check: Option<String>,
    pub kind: Option<String>,
    pub ticket_snapshot: String,
    pub extra: Option<String>,
    pub handoff: Option<String>,
    pub idea: Option<String>,
    pub feature: Option<String>,
    pub covers: Vec<String>,
    pub verdict: Option<String>,
}

pub const RENEW_EVERY_S: u64 = 10;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnsweredQuestion {
    pub id: String,
    pub question: Option<String>,
    pub answer: Option<String>,
}

pub fn answers_section(answered: &[AnsweredQuestion]) -> Option<String> {
    if answered.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Answers to the questions of this task\n\n\
         An earlier attempt asked these questions. Use each answer.\n",
    );
    for q in answered {
        out.push_str(&format!(
            "\n### Question {}\n\n{}\n\nAnswer: {}\n",
            q.id,
            q.question
                .as_deref()
                .unwrap_or("(the question has no text)"),
            q.answer.as_deref().unwrap_or("(the answer has no text)"),
        ));
    }
    Some(out)
}

fn scalar(value: &str) -> String {
    if is_plain(value) {
        value.to_string()
    } else {
        quoted(value)
    }
}

fn list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|i| scalar(i)).collect();
    format!("[{}]", items.join(", "))
}

const RESERVED: [&str; 11] = [
    "null", "true", "false", "yes", "no", "on", "off", "y", "n", "~", "",
];

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Profile,
    Forbidden,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub name: String,
    value: String,
    pub source: Source,
}

impl Secret {
    pub fn profile(name: &str, value: &str) -> Secret {
        Secret {
            name: name.to_string(),
            value: value.to_string(),
            source: Source::Profile,
        }
    }

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

pub const MIN_SECRET_LEN: usize = 8;

const SECRET_SUFFIXES: [&str; 3] = ["_KEY=", "_TOKEN=", "_SECRET="];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sr2Rule {
    ProfileEnvValue,
    ForbiddenEnvValue,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sr2Refusal {
    pub file: String,
    pub rule: Sr2Rule,
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
    fn answers_section_holds_each_question_and_its_answer() {
        assert_eq!(answers_section(&[]), None);
        let q = |id: &str, question: &str, answer: Option<&str>| AnsweredQuestion {
            id: id.into(),
            question: Some(question.into()),
            answer: answer.map(str::to_string),
        };
        let section = answers_section(&[
            q("t-1/A1/Q1", "Which port?", Some("Port 8080.")),
            q("t-1/A1/Q2", "Which host?", None),
        ])
        .unwrap();
        let first = section.find("Which port?\n\nAnswer: Port 8080.").unwrap();
        let second = section
            .find("Which host?\n\nAnswer: (the answer has no text)")
            .unwrap();
        assert!(first < second, "oldest first:\n{section}");
        assert!(section.contains("### Question t-1/A1/Q1"), "{section}");
    }

    #[test]
    fn debug_of_a_secret_hides_its_value() {
        let shown = format!("{:?}", Secret::profile("GH_TOKEN", "ghp_value12345"));
        assert!(shown.contains("GH_TOKEN") && !shown.contains("ghp_value"));
    }
}
