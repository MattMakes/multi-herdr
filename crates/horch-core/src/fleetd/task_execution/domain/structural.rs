use crate::fleetd::task_execution::domain::judge_pair;
use crate::fleetd::task_execution::domain::result::parse_json;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::verification::{Decision, Verdict};

pub const PASSED: &str = "structural: passed";
pub const FAILED: &str = "structural: failed";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub path: String,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    pub files: Vec<String>,
    pub failures: Vec<String>,
    pub outcome: Option<Outcome>,
}

impl Checked {
    pub fn unreadable(why: &str) -> Checked {
        Checked {
            files: Vec::new(),
            failures: vec![why.to_string()],
            outcome: None,
        }
    }

    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn reason(&self) -> String {
        if self.passed() {
            format!("{PASSED}: {}", self.files.join(", "))
        } else {
            format!("{FAILED}: {}", self.failures.join("; "))
        }
    }
}

pub fn is_pass(v: &Verdict) -> bool {
    v.decision == Decision::Passed && v.job.is_none() && v.reason.starts_with(PASSED)
}

pub fn of_own(own: &[String], path: &str) -> bool {
    own.iter().any(|entry| {
        let folder = entry.trim_end_matches('/');
        path == entry
            || path
                .strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

pub fn outputs_of(own: &[String], changed: &[String]) -> Vec<String> {
    let mut files: Vec<String> = changed.iter().filter(|p| of_own(own, p)).cloned().collect();
    files.sort();
    files.dedup();
    files
}

pub fn check(outputs: &[Output]) -> Checked {
    check_with(outputs, |text| (markdown(text), None))
}

pub fn check_judge(outputs: &[Output]) -> Checked {
    let mut checked = check_with(outputs, |text| match judge_pair::parse_verdict(text) {
        Ok(v) => (Vec::new(), v.outcome()),
        Err(errors) => (errors.iter().map(ToString::to_string).collect(), None),
    });
    if !checked.passed() {
        checked.outcome = None;
    }
    checked
}

fn check_with(outputs: &[Output], md: impl Fn(&str) -> (Vec<String>, Option<Outcome>)) -> Checked {
    let mut files: Vec<String> = outputs.iter().map(|o| o.path.clone()).collect();
    files.sort();
    if files.is_empty() {
        return Checked::unreadable("the commits change no file of Own");
    }
    let mut failures = Vec::new();
    let mut outcome = None;
    for o in outputs {
        let Some(text) = &o.text else {
            failures.push(format!("{}: not at the tip", o.path));
            continue;
        };
        if o.path.ends_with(".md") {
            let (found, out) = md(text);
            failures.extend(found.into_iter().map(|f| format!("{}: {f}", o.path)));
            outcome = outcome.or(out);
        } else if o.path.ends_with(".json") {
            if let Err(e) = parse_json(text) {
                failures.push(format!("{}: invalid JSON: {e}", o.path));
            }
        }
    }
    Checked {
        files,
        failures,
        outcome,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Line<'a> {
    Blank,
    Text,
    Heading(usize, &'a str),
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut fenced = false;
    text.lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                fenced = !fenced;
                return Line::Text;
            }
            if fenced {
                return Line::Text;
            }
            if t.is_empty() {
                return Line::Blank;
            }
            let level = t.chars().take_while(|c| *c == '#').count();
            let rest = &t[level..];
            if (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' ')) {
                Line::Heading(level, line.trim())
            } else {
                Line::Text
            }
        })
        .collect()
}

fn markdown(text: &str) -> Vec<String> {
    let lines = lines(text);
    if !lines.iter().any(|l| matches!(l, Line::Heading(2, _))) {
        return vec!["no `## ` heading".into()];
    }
    let mut failures = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Line::Heading(level, heading) = *line else {
            continue;
        };
        if level < 2 {
            continue;
        }
        let content = lines[i + 1..]
            .iter()
            .take_while(|l| !matches!(l, Line::Heading(n, _) if *n <= level))
            .any(|l| *l != Line::Blank);
        if !content {
            failures.push(format!("the section `{heading}` is empty"));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(path: &str, text: Option<&str>) -> Output {
        Output {
            path: path.into(),
            text: text.map(Into::into),
        }
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn docs_task_with_its_file_and_sections_passes() {
        let c = check(&[
            out(
                "docs/a.md",
                Some("# A\n\n## Goal\ntext\n### Detail\nmore\n"),
            ),
            out("docs/b.json", Some("{\"a\": [1]}")),
            out("docs/c.txt", Some("")),
        ]);
        assert!(c.passed(), "{c:?}");
        assert_eq!(
            c.reason(),
            "structural: passed: docs/a.md, docs/b.json, docs/c.txt"
        );
    }

    #[test]
    fn missing_output_file_fails() {
        let c = check(&[out("docs/a.md", None)]);
        assert_eq!(c.failures, ["docs/a.md: not at the tip"]);
        assert!(c.reason().starts_with(FAILED));
    }

    #[test]
    fn markdown_with_no_section_fails() {
        let c = check(&[out("a.md", Some("# Title\ntext\n"))]);
        assert_eq!(c.failures, ["a.md: no `## ` heading"]);
        let c = check(&[out("a.md", Some("# T\n## Empty\n\n## Full\nx\n"))]);
        assert_eq!(c.failures, ["a.md: the section `## Empty` is empty"]);
        let c = check(&[out("a.md", Some("## Last\n\n"))]);
        assert_eq!(c.failures, ["a.md: the section `## Last` is empty"]);
    }

    #[test]
    fn a_subsection_is_content_and_a_heading_in_a_fence_is_not() {
        let c = check(&[out("a.md", Some("## Parent\n### Child\ntext\n"))]);
        assert!(c.passed(), "{c:?}");
        let c = check(&[out("a.md", Some("# T\n```\n## not a heading\n```\n"))]);
        assert_eq!(c.failures, ["a.md: no `## ` heading"]);
        let c = check(&[out("a.md", Some("## Child empty\n### Child\n"))]);
        assert_eq!(c.failures, ["a.md: the section `### Child` is empty"]);
    }

    #[test]
    fn invalid_json_output_fails() {
        let c = check(&[out("x.json", Some("{\"a\": "))]);
        assert_eq!(c.failures.len(), 1);
        assert!(c.failures[0].starts_with("x.json: invalid JSON"), "{c:?}");
    }

    #[test]
    fn judgment_task_that_changes_no_owned_file_fails() {
        let c = check(&[]);
        assert_eq!(c.failures, ["the commits change no file of Own"]);
    }

    #[test]
    fn the_outputs_are_the_changed_files_of_own() {
        let own = strings(&["docs/a.md", "data/", "out"]);
        let changed = strings(&[
            "docs/a.md",
            "docs/b.md",
            "data/x.json",
            "out/y.md",
            "outer.md",
            "data/x.json",
        ]);
        assert_eq!(
            outputs_of(&own, &changed),
            ["data/x.json", "docs/a.md", "out/y.md"]
        );
        assert!(!of_own(&[], "a.md"));
    }

    #[test]
    fn only_a_passed_check_verdict_with_no_job_is_a_pass() {
        let v = Verdict {
            task_id: "T-1".into(),
            attempt: "T-1-a1".into(),
            n: 2,
            decision: Decision::Passed,
            tip: "tip".into(),
            check_hash: "h".into(),
            job: None,
            reason: "structural: passed: a.md".into(),
        };
        assert!(is_pass(&v));
        assert!(!is_pass(&Verdict {
            job: Some(1),
            ..v.clone()
        }));
        assert!(!is_pass(&Verdict {
            reason: "exit 0".into(),
            ..v.clone()
        }));
        assert!(!is_pass(&Verdict {
            decision: Decision::Failed,
            reason: "structural: failed: x".into(),
            ..v
        }));
    }
}
