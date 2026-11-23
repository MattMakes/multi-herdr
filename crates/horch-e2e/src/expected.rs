use std::any::Any;
use std::collections::HashSet;
use std::panic::{catch_unwind, UnwindSafe};
use std::path::{Path, PathBuf};

const PHASES: [&str; 8] = ["P", "1", "2", "3", "4", "5", "6", "7"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub test: String,
    pub phase: String,
    pub reason: String,
}

pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '|').map(str::trim).collect();
        let [test, phase, reason] = parts[..] else {
            return Err(format!(
                "line {}: `{line}` is not `<test name> | <phase> | <reason>`",
                n + 1
            ));
        };
        if test.is_empty() || phase.is_empty() || reason.is_empty() {
            return Err(format!(
                "line {}: `{line}` needs a test name, a phase and a reason",
                n + 1
            ));
        }
        if !PHASES.contains(&phase) {
            return Err(format!(
                "line {}: phase `{phase}` is not 1 of {}",
                n + 1,
                PHASES.join(", ")
            ));
        }
        if !seen.insert(test.to_string()) {
            return Err(format!("line {}: `{test}` is listed 2 times", n + 1));
        }
        entries.push(Entry {
            test: test.to_string(),
            phase: phase.to_string(),
            reason: reason.to_string(),
        });
    }
    Ok(entries)
}

fn lists_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("expected-failures")
}

pub fn check(test_file: &str, test: &str, body: impl FnOnce() + UnwindSafe) {
    check_in(&lists_dir(), test_file, test, body);
}

fn check_in(lists: &Path, test_file: &str, test: &str, body: impl FnOnce() + UnwindSafe) {
    let stem = Path::new(test_file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let list = lists.join(format!("{stem}.txt"));
    let text = match std::fs::read_to_string(&list) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => panic!("cannot read {}: {e}", list.display()),
    };
    let entries = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", list.display()));
    let Some(entry) = entries.into_iter().find(|e| e.test == test) else {
        body();
        return;
    };
    match catch_unwind(body) {
        Ok(()) => panic!(
            "`{test}` passed but is on the list of expected failures. Phase `{}` makes it green: remove its line from {}.",
            entry.phase,
            list.display()
        ),
        Err(payload) => {
            let got = message(payload.as_ref());
            assert!(
                got.contains(&entry.reason),
                "`{test}` is on the list of expected failures with the reason `{}`, but it failed with another message: `{got}`",
                entry.reason
            );
        }
    }
}

fn message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<a panic with no text>".to_string()
    }
}

pub fn audit(lists: &Path, tests: &Path) -> Vec<String> {
    let mut faults = Vec::new();
    let Ok(dir) = std::fs::read_dir(lists) else {
        return faults;
    };
    let mut files: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    for list in files {
        let stem = list
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let test_file = tests.join(format!("{stem}.rs"));
        let Ok(source) = std::fs::read_to_string(&test_file) else {
            faults.push(format!(
                "{}: no test file {}",
                list.display(),
                test_file.display()
            ));
            continue;
        };
        let entries = match std::fs::read_to_string(&list)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&text))
        {
            Ok(entries) => entries,
            Err(e) => {
                faults.push(format!("{}: {e}", list.display()));
                continue;
            }
        };
        let code = strip_comments(&source);
        for entry in entries {
            let test = &entry.test;
            let Some(body) = test_body(&code, test) else {
                faults.push(format!(
                    "{}: `{test}` has no `#[test] fn {test}(` in {}",
                    list.display(),
                    test_file.display()
                ));
                continue;
            };
            let body: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            if !body.contains(&format!("check(file!(),\"{test}\"")) {
                faults.push(format!(
                    "{}: `{test}` does not call `check(file!(), \"{test}\", ...)` in {}",
                    list.display(),
                    test_file.display()
                ));
            }
        }
    }
    faults
}

fn strip_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let next = chars.get(i + 1).copied();
        if chars[i] == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if chars[i] == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
            }
        } else {
            let end = literal_end(&chars, i).unwrap_or(i + 1);
            out.extend(&chars[i..end]);
            i = end;
        }
    }
    out
}

fn literal_end(chars: &[char], i: usize) -> Option<usize> {
    match chars[i] {
        '"' => {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            Some((j + 1).min(chars.len()))
        }
        '\'' if chars.get(i + 1) == Some(&'\\') => {
            let close = chars[i + 2..].iter().position(|&c| c == '\'')?;
            Some(i + 2 + close + 1)
        }
        '\'' if chars.get(i + 2) == Some(&'\'') => Some(i + 3),
        _ => None,
    }
}

fn test_body<'a>(code: &'a str, test: &str) -> Option<&'a str> {
    let needle = format!("fn {test}(");
    let lines: Vec<&str> = code.lines().collect();
    let mut offset = 0;
    for (i, line) in lines.iter().enumerate() {
        let is_test = lines[..i]
            .iter()
            .rev()
            .map(|l| l.trim())
            .take_while(|l| l.is_empty() || l.starts_with("#["))
            .any(|l| l == "#[test]");
        if let (true, Some(at)) = (is_test, line.find(&needle)) {
            return braced(code, offset + at);
        }
        offset += line.len() + 1;
    }
    None
}

fn braced(code: &str, from: usize) -> Option<&str> {
    let start = from + code[from..].find('{')?;
    let chars: Vec<(usize, char)> = code[start..].char_indices().collect();
    let plain: Vec<char> = chars.iter().map(|&(_, c)| c).collect();
    let mut depth = 0;
    let mut i = 0;
    while i < plain.len() {
        if let Some(end) = literal_end(&plain, i) {
            i = end;
            continue;
        }
        match plain[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&code[start..=start + chars[i].0]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn panic_text(f: impl FnOnce()) -> Option<String> {
        catch_unwind(AssertUnwindSafe(f))
            .err()
            .map(|p| message(p.as_ref()))
    }

    fn list_dir(file: &str, text: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(file), text).unwrap();
        dir
    }

    #[test]
    fn expected_parse_reads_test_phase_and_reason() {
        let text = "# comment\n\n  my_test | 2 | no fleetd yet | really \n";
        let entries = parse(text).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].test, "my_test");
        assert_eq!(entries[0].phase, "2");
        assert_eq!(entries[0].reason, "no fleetd yet | really");
    }

    #[test]
    fn expected_parse_refuses_a_line_without_a_phase_or_a_reason() {
        assert!(parse("my_test").is_err());
        assert!(parse("my_test | 2").is_err());
        assert!(parse("my_test | 2 |  ").is_err());
        assert!(parse("my_test |  | why").is_err());
        assert!(parse(" | 2 | why").is_err());
    }

    #[test]
    fn expected_parse_refuses_an_unknown_phase() {
        let err = parse("my_test | 8 | why").unwrap_err();
        assert!(err.contains('8'), "{err}");
        assert!(parse("my_test | p | why").is_err());
        for phase in ["P", "1", "2", "3", "4", "5", "6", "7"] {
            assert!(parse(&format!("t | {phase} | why")).is_ok(), "{phase}");
        }
    }

    #[test]
    fn expected_parse_refuses_a_test_listed_twice() {
        let err = parse("my_test | 1 | a\nmy_test | 2 | b\n").unwrap_err();
        assert!(err.contains("my_test"), "{err}");
    }

    #[test]
    fn expected_listed_body_that_panics_with_the_reason_passes() {
        let dir = list_dir("demo.txt", "my_test | 3 | no fleetd yet\n");
        check_in(dir.path(), "tests/demo.rs", "my_test", || {
            panic!("boom: no fleetd yet, sorry")
        });
    }

    #[test]
    fn expected_listed_body_that_panics_for_another_reason_fails() {
        let dir = list_dir("demo.txt", "my_test | 3 | no fleetd yet\n");
        let text = panic_text(|| {
            check_in(dir.path(), "tests/demo.rs", "my_test", || {
                panic!("disk is full")
            });
        })
        .expect("check must panic");
        assert!(text.contains("no fleetd yet"), "{text}");
        assert!(text.contains("disk is full"), "{text}");
    }

    #[test]
    fn expected_listed_body_that_passes_fails_and_names_the_phase() {
        let dir = list_dir("demo.txt", "my_test | 3 | no fleetd yet\n");
        let text = panic_text(|| check_in(dir.path(), "tests/demo.rs", "my_test", || {}))
            .expect("check must panic");
        assert!(
            text.contains("`my_test` passed but is on the list of expected failures"),
            "{text}"
        );
        assert!(text.contains("Phase `3` makes it green"), "{text}");
        assert!(
            text.contains(&dir.path().join("demo.txt").display().to_string()),
            "{text}"
        );
    }

    #[test]
    fn expected_unlisted_body_runs_as_a_normal_test() {
        let dir = list_dir("demo.txt", "other | 3 | no fleetd yet\n");
        let ran = std::sync::atomic::AtomicBool::new(false);
        check_in(dir.path(), "tests/demo.rs", "my_test", || {
            ran.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
        let text = panic_text(|| {
            check_in(dir.path(), "tests/demo.rs", "my_test", || {
                panic!("real bug")
            })
        });
        assert_eq!(text.as_deref(), Some("real bug"));
        let text = panic_text(|| {
            check_in(dir.path(), "tests/none.rs", "my_test", || {
                panic!("real bug")
            })
        });
        assert_eq!(text.as_deref(), Some("real bug"));
    }

    fn tree(list: &str, test_file: Option<&str>) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let lists = root.path().join("expected-failures");
        let tests = root.path().join("tests");
        std::fs::create_dir_all(&lists).unwrap();
        std::fs::create_dir_all(&tests).unwrap();
        std::fs::write(lists.join("demo.txt"), list).unwrap();
        std::fs::write(lists.join("README.md"), "not a list\n").unwrap();
        if let Some(text) = test_file {
            std::fs::write(tests.join("demo.rs"), text).unwrap();
        }
        (root, lists, tests)
    }

    const GOOD: &str = r#"
#[test]
fn my_test() {
    horch_e2e::expected::check(file!(), "my_test", || {
        panic!("no fleetd yet");
    });
}
"#;

    #[test]
    fn expected_audit_reports_an_entry_with_no_test() {
        let (_root, lists, tests) = tree("my_test | 1 | no fleetd yet\n", Some(GOOD));
        assert_eq!(audit(&lists, &tests), Vec::<String>::new());

        let list = "my_test | 1 | no fleetd yet\ngone_test | 1 | x\n";
        let (_root, lists, tests) = tree(list, Some(GOOD));
        let faults = audit(&lists, &tests);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("gone_test"), "{faults:?}");

        let text = format!("{GOOD}\nfn gone_test() {{}}\n");
        let (_root, lists, tests) = tree(list, Some(&text));
        let faults = audit(&lists, &tests);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("gone_test"), "{faults:?}");
    }

    #[test]
    fn expected_audit_reports_a_list_file_with_no_test_file() {
        let (_root, lists, tests) = tree("my_test | 1 | no fleetd yet\n", None);
        let faults = audit(&lists, &tests);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("demo.txt"), "{faults:?}");
        assert!(faults[0].contains("demo.rs"), "{faults:?}");
    }

    #[test]
    fn expected_audit_reports_a_listed_test_that_does_not_call_the_wrapper() {
        let text = "#[test]\nfn my_test() {\n    panic!(\"no fleetd yet\");\n}\n";
        let (_root, lists, tests) = tree("my_test | 1 | no fleetd yet\n", Some(text));
        let faults = audit(&lists, &tests);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("my_test"), "{faults:?}");
        assert!(faults[0].contains("check(file!()"), "{faults:?}");

        let commented = "#[test]\nfn my_test() {\n    // check(file!(), \"my_test\", || {});\n    /* check(file!(), \"my_test\", || {}); */\n    let _ = \"}\";\n}\n";
        let other = "#[test]\nfn my_test() {\n    let _ = '}';\n}\n\n#[test]\nfn other() {\n    horch_e2e::expected::check(file!(), \"my_test\", || {});\n}\n";
        for text in [commented, other] {
            let (_root, lists, tests) = tree("my_test | 1 | no fleetd yet\n", Some(text));
            let faults = audit(&lists, &tests);
            assert_eq!(faults.len(), 1, "{text}\n{faults:?}");
            assert!(faults[0].contains("check(file!()"), "{faults:?}");
        }
    }

    #[test]
    fn expected_audit_reports_a_list_file_that_does_not_parse() {
        let (_root, lists, tests) = tree("my_test | 9 | x\n", Some(GOOD));
        let faults = audit(&lists, &tests);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("demo.txt"), "{faults:?}");
    }
}
