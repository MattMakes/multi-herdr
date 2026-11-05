//! The complexity-sweep step (#38): the `complexity-sweep` skill's analyser
//! on the TypeScript, JavaScript and Python files the branch changed, and
//! clippy's `cognitive_complexity` lint on its Rust files (TRF-07). It
//! passes only when no function the branch changed has a violation. horch
//! alone decides "not applicable": when no changed file is in a language
//! either reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Ctx, Outcome};

/// The extensions the analyser parses: TypeScript and JavaScript (its core
/// is `ts-morph`), and Python. It has no Rust or GDScript parser: it reads
/// those files as TypeScript and reports functions that do not exist, so
/// they are not sent to it (#204). Rust goes to clippy instead.
pub const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "py"];

#[derive(Debug, Deserialize)]
struct Analysis {
    #[serde(default)]
    files: Vec<File>,
    #[serde(default)]
    violations: Vec<Violation>,
}

#[derive(Debug, Deserialize)]
struct File {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(default)]
    functions: Vec<Function>,
}

#[derive(Debug, Deserialize)]
struct Function {
    #[serde(default)]
    line: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Violation {
    #[serde(rename = "functionName")]
    pub function_name: String,
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub line: u64,
    pub metric: String,
    pub value: f64,
    pub threshold: f64,
}

/// The new-side line ranges `git diff -U0` reports per file.
pub fn changed_lines(diff: &str) -> BTreeMap<String, Vec<(u64, u64)>> {
    let mut out: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
    let mut file: Option<String> = None;
    for line in diff.lines() {
        if let Some(f) = line.strip_prefix("+++ ") {
            file = f.strip_prefix("b/").map(|s| s.to_string());
        } else if let (Some(f), Some(range)) = (&file, hunk_range(line)) {
            out.entry(f.clone()).or_default().push(range);
        }
    }
    out
}

/// The new-side line range of 1 hunk header; `None` for any other line and
/// for a hunk that adds no line.
fn hunk_range(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = &rest.split_whitespace().find(|t| t.starts_with('+'))?[1..];
    let (start, count) = match plus.split_once(',') {
        Some((s, c)) => (s.parse::<u64>().unwrap_or(0), c.parse::<u64>().unwrap_or(1)),
        None => (plus.parse::<u64>().unwrap_or(0), 1),
    };
    (count > 0).then(|| (start, start + count - 1))
}

/// `p` relative to `root`, as `git diff` names it.
fn rel(p: &str, root: &Path) -> String {
    Path::new(p)
        .strip_prefix(root)
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.trim_start_matches("./").to_string())
}

/// The violations in functions whose span (its line to the line before the
/// next function in the file) holds a changed line. `starts` holds the
/// sorted first lines of the functions of each file.
fn keep_changed(
    found: Vec<Violation>,
    starts: &BTreeMap<String, Vec<u64>>,
    changed: &BTreeMap<String, Vec<(u64, u64)>>,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for v in found {
        let Some(ranges) = changed.get(&v.file_path) else {
            continue;
        };
        let end = starts
            .get(&v.file_path)
            .and_then(|s| s.iter().find(|l| **l > v.line).map(|l| l - 1))
            .unwrap_or(u64::MAX);
        if ranges.iter().any(|(a, b)| *a <= end && *b >= v.line) {
            out.push(v);
        }
    }
    out
}

/// The analyser's violations in functions the branch changed.
pub fn in_changed_functions(
    json: &str,
    changed: &BTreeMap<String, Vec<(u64, u64)>>,
    root: &Path,
) -> Result<Vec<Violation>> {
    let a: Analysis = serde_json::from_str(json).context("reading the analyser's JSON")?;
    let mut starts: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for f in &a.files {
        let mut lines: Vec<u64> = f.functions.iter().map(|x| x.line).collect();
        lines.sort();
        starts.insert(rel(&f.file_path, root), lines);
    }
    let found = a
        .violations
        .into_iter()
        .map(|mut v| {
            v.file_path = rel(&v.file_path, root);
            v
        })
        .collect();
    Ok(keep_changed(found, &starts, changed))
}

// ---- Rust: clippy's cognitive_complexity lint (TRF-07) ----

/// The lint that measures a Rust function.
const CLIPPY_LINT: &str = "clippy::cognitive_complexity";

#[derive(Debug, Deserialize)]
struct CargoLine {
    #[serde(default)]
    reason: String,
    message: Option<Diagnostic>,
}

#[derive(Debug, Deserialize)]
struct Diagnostic {
    message: String,
    code: Option<Code>,
    #[serde(default)]
    spans: Vec<Span>,
}

#[derive(Debug, Deserialize)]
struct Code {
    code: String,
}

#[derive(Debug, Deserialize)]
struct Span {
    file_name: String,
    line_start: u64,
    is_primary: bool,
    column_start: usize,
    column_end: usize,
    #[serde(default)]
    text: Vec<SpanText>,
}

#[derive(Debug, Deserialize)]
struct SpanText {
    text: String,
}

/// The `cognitive_complexity` findings in `cargo clippy --message-format=json`
/// output, once each (a file in 2 targets is reported twice). The value and
/// the threshold come from the message, "... of (26/25)", so the threshold
/// is the one clippy used: `clippy.toml` when present, else its default.
pub fn clippy_violations(json: &str, root: &Path) -> Vec<Violation> {
    let mut out: Vec<Violation> = Vec::new();
    for line in json.lines() {
        let Ok(CargoLine {
            reason,
            message: Some(d),
        }) = serde_json::from_str::<CargoLine>(line)
        else {
            continue;
        };
        if reason != "compiler-message"
            || d.code.as_ref().map(|c| c.code.as_str()) != Some(CLIPPY_LINT)
        {
            continue;
        }
        let Some(span) = d.spans.iter().find(|s| s.is_primary) else {
            continue;
        };
        let (value, threshold) = score(&d.message).unwrap_or((0.0, 0.0));
        let v = Violation {
            function_name: name(span),
            file_path: rel(&span.file_name, root),
            line: span.line_start,
            metric: "cognitiveComplexity".to_string(),
            value,
            threshold,
        };
        if !out
            .iter()
            .any(|o| o.file_path == v.file_path && o.line == v.line)
        {
            out.push(v);
        }
    }
    out
}

/// The value and the threshold of "the function has a cognitive complexity
/// of (26/25)".
fn score(message: &str) -> Option<(f64, f64)> {
    let inner = message.rsplit_once('(')?.1.split_once(')')?.0;
    let (value, threshold) = inner.split_once('/')?;
    Some((value.trim().parse().ok()?, threshold.trim().parse().ok()?))
}

/// The function name the primary span marks.
fn name(span: &Span) -> String {
    let Some(t) = span.text.first() else {
        return String::new();
    };
    t.text
        .chars()
        .skip(span.column_start.saturating_sub(1))
        .take(span.column_end.saturating_sub(span.column_start))
        .collect()
}

/// The 1-based lines of `source` that start a `fn` item.
fn fn_starts(source: &str) -> Vec<u64> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| starts_fn(l))
        .map(|(i, _)| i as u64 + 1)
        .collect()
}

/// Whether `line` starts a `fn` item: only qualifiers come before `fn`.
fn starts_fn(line: &str) -> bool {
    const QUALIFIERS: &[&str] = &["const", "async", "unsafe", "extern", "default", "\"C\""];
    for w in line.split_whitespace() {
        if w == "fn" {
            return true;
        }
        if !(w.starts_with("pub") || QUALIFIERS.contains(&w)) {
            return false;
        }
    }
    false
}

/// The changed `.rs` files, when `dir` is a cargo project.
fn rust_sources(names: &str, dir: &Path) -> Vec<String> {
    if !dir.join("Cargo.toml").is_file() {
        return Vec::new();
    }
    names
        .lines()
        .filter(|f| f.ends_with(".rs"))
        .map(|s| s.to_string())
        .collect()
}

/// `cargo clippy` with only the cognitive-complexity lint, capped at warn so
/// a `#![deny(...)]` in the code does not stop the build.
fn clippy(dir: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "clippy",
        "--quiet",
        "--workspace",
        "--all-targets",
        "--message-format=json",
        "--",
        "-A",
        "clippy::all",
        "-W",
        CLIPPY_LINT,
        "--cap-lints",
        "warn",
    ])
    .current_dir(dir);
    crate::runtime::process::scrub_child_env(&mut cmd);
    cmd
}

/// Run clippy in `dir` and return its findings in the changed Rust files,
/// before the changed-function filter. Its JSON goes to `evidence`.
fn analyse_rust(
    dir: &Path,
    evidence: &Path,
) -> Result<std::result::Result<Vec<Violation>, Outcome>> {
    let mut cmd = clippy(dir);
    cmd.stdout(std::fs::File::create(evidence)?);
    let out = cmd.output().context("running cargo clippy")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.trim().lines().rev().take(20).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Ok(Err(Outcome::fail(
            "clippy failed",
            Some(evidence.to_path_buf()),
            &format!(
                "cargo clippy failed; fix the build, commit, then run finish again: {}. Full output: {}",
                tail.join("\n"),
                evidence.display()
            ),
        )));
    }
    let json = std::fs::read_to_string(evidence)?;
    Ok(Ok(clippy_violations(&json, dir)))
}

/// The first lines of the functions in each of `files`.
fn rust_starts(dir: &Path, files: &[String]) -> BTreeMap<String, Vec<u64>> {
    files
        .iter()
        .map(|f| {
            let text = std::fs::read_to_string(dir.join(f)).unwrap_or_default();
            (f.clone(), fn_starts(&text))
        })
        .collect()
}

/// The files of `git diff --name-only` in a language the analyser reads.
fn sources(names: &str) -> Vec<String> {
    names
        .lines()
        .filter(|f| {
            Path::new(f)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e))
        })
        .map(|s| s.to_string())
        .collect()
}

/// The command that runs the analyser on `files`.
fn analyser(cx: &Ctx<'_>, cli: &Path, files: &[String]) -> Command {
    let mut cmd = if cli.extension().is_some_and(|e| e == "js") {
        let mut c = Command::new(&cx.tools.node);
        c.arg(cli);
        c
    } else {
        Command::new(cli)
    };
    cmd.args(["--format", "json", "--threshold-action", "none"])
        .args(files)
        .current_dir(&cx.dir);
    crate::runtime::process::scrub_child_env(&mut cmd);
    cmd
}

/// The failed outcome that names the first 10 violations.
fn violations(found: &[Violation], evidence: &[PathBuf]) -> Outcome {
    let list: Vec<String> = found
        .iter()
        .take(10)
        .map(|v| {
            format!(
                "{}:{} {} {}={} (max {})",
                v.file_path, v.line, v.function_name, v.metric, v.value, v.threshold
            )
        })
        .collect();
    let paths: Vec<String> = evidence.iter().map(|p| p.display().to_string()).collect();
    Outcome::fail(
        &format!("{} violations", found.len()),
        evidence.first().cloned(),
        &format!(
            "reduce the complexity of the functions you changed, commit, then run finish again: {}. Full analysis: {}",
            list.join("; "),
            paths.join(", ")
        ),
    )
}

/// Run the analyser on `files` and return the analysis, or the failed
/// outcome when it printed nothing. Its JSON goes to `evidence`.
fn analyse(
    cx: &Ctx<'_>,
    cli: &Path,
    files: &[String],
    evidence: &Path,
) -> Result<std::result::Result<String, Outcome>> {
    let mut cmd = analyser(cx, cli, files);
    // The report goes to a file, not a pipe: node exits before a piped
    // stdout drains, and a pipe keeps only the first 64 KiB.
    cmd.stdout(std::fs::File::create(evidence)?);
    let out = cmd
        .output()
        .with_context(|| format!("running {}", cli.display()))?;
    let json = std::fs::read_to_string(evidence)?;
    if json.trim().is_empty() {
        return Ok(Err(Outcome::fail(
            "no output",
            Some(evidence.to_path_buf()),
            &format!(
                "the analyser printed nothing: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        )));
    }
    Ok(Ok(json))
}

/// The violations in changed functions of the Rust `files`, from clippy
/// in `dir`: the found list, or the failed outcome when clippy failed.
pub fn rust_violations(
    dir: &Path,
    files: &[String],
    changed: &BTreeMap<String, Vec<(u64, u64)>>,
    evidence: &Path,
) -> Result<std::result::Result<Vec<Violation>, Outcome>> {
    Ok(analyse_rust(dir, evidence)?
        .map(|found| keep_changed(found, &rust_starts(dir, files), changed)))
}

/// The step.
pub fn step(cx: &Ctx<'_>) -> Result<Outcome> {
    let git = &cx.tools.git;
    let base = cx
        .run
        .run_branch
        .clone()
        .or(cx.run.base_branch.clone())
        .context("the run has no run branch")?;
    let names = git.run(
        &cx.dir,
        &[
            "diff",
            "--name-only",
            "--diff-filter=AMR",
            &format!("{base}...HEAD"),
        ],
    )?;
    let files = sources(&names);
    let rust = rust_sources(&names, &cx.dir);
    let tree = git.tree(&cx.dir).ok();
    if files.is_empty() && rust.is_empty() {
        return Ok(
            Outcome::not_applicable("no changed file is in a language the analyser reads")
                .judged(tree),
        );
    }
    let mut found = Vec::new();
    let mut evidence = Vec::new();
    let mut ts_json = None;
    if !files.is_empty() {
        let Some(cli) = &cx.tools.complexity_cli else {
            return Ok(Outcome::fail(
                "no analyser",
                None,
                "the complexity-sweep skill's analyser was not found. Install the code plugin's complexity-sweep skill (and run npm install in its code/core and code/cli), or set HORCH_COMPLEXITY_CLI to its code/cli/dist/index.js",
            ));
        };
        let path = cx.evidence_dir.join(format!("{}-complexity.json", cx.seq));
        match analyse(cx, cli, &files, &path)? {
            Ok(json) => ts_json = Some(json),
            Err(failed) => return Ok(failed),
        }
        evidence.push(path);
    }
    let diff = git.run(&cx.dir, &["diff", "-U0", &format!("{base}...HEAD"), "--"])?;
    let changed = changed_lines(&diff);
    if let Some(json) = ts_json {
        found.extend(in_changed_functions(&json, &changed, &cx.dir)?);
    }
    if !rust.is_empty() {
        let path = cx.evidence_dir.join(format!("{}-clippy.json", cx.seq));
        match rust_violations(&cx.dir, &rust, &changed, &path)? {
            Ok(v) => found.extend(v),
            Err(failed) => return Ok(failed),
        }
        evidence.push(path);
    }
    let outcome = if found.is_empty() {
        Outcome::pass(
            "0 violations",
            evidence.first().cloned(),
            &format!("{} files analysed", files.len() + rust.len()),
        )
    } else {
        violations(&found, &evidence)
    };
    Ok(outcome.judged(tree))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::git::Git;

    #[test]
    fn only_violations_in_changed_functions_count() {
        let diff = "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -10,0 +11,3 @@ fn x\n+a\n+b\n+c\n";
        let changed = changed_lines(diff);
        assert_eq!(changed["src/a.ts"], vec![(11, 13)]);
        let json = r#"{"files":[{"filePath":"src/a.ts","functions":[{"name":"f","line":1},{"name":"g","line":9},{"name":"h","line":30}]}],
          "violations":[
            {"functionName":"g","filePath":"src/a.ts","line":9,"metric":"cyclomaticComplexity","value":12,"threshold":10,"severity":"warn"},
            {"functionName":"h","filePath":"src/a.ts","line":30,"metric":"abcScore","value":40,"threshold":30,"severity":"warn"}]}"#;
        let v = in_changed_functions(json, &changed, Path::new("/repo")).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].function_name, "g");
    }

    #[test]
    fn changed_lines_reads_every_hunk_kind() {
        let diff = "@@ -1 +1 @@ before any file\n\
                    +++ b/a.ts\n@@ -1,0 +2 @@\n@@ -5,2 +7,0 @@ deleted only\n@@ -9 +10,2 @@\n\
                    +++ /dev/null\n@@ -1,3 +0,0 @@\n\
                    +++ b/b.py\n@@ -0,0 +1,4 @@\n";
        let changed = changed_lines(diff);
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert_eq!(changed["a.ts"], vec![(2, 2), (10, 11)]);
        assert_eq!(changed["b.py"], vec![(1, 4)]);
    }

    #[test]
    fn trf_07_reads_clippy_findings_once_with_name_value_and_threshold() {
        let line = r#"{"reason":"compiler-message","message":{"message":"the function has a cognitive complexity of (5/2)","code":{"code":"clippy::cognitive_complexity","explanation":null},"spans":[{"file_name":"src/lib.rs","line_start":5,"is_primary":true,"column_start":8,"column_end":15,"text":[{"text":"pub fn tangled(v: &[u32]) -> u32 {"}]}]}}"#;
        let other = r#"{"reason":"compiler-message","message":{"message":"unused variable","code":{"code":"unused_variables"},"spans":[]}}"#;
        let json = format!("{line}\n{line}\n{other}\n{{\"reason\":\"build-finished\",\"success\":true}}\nnot json\n");
        let v = clippy_violations(&json, Path::new("/repo"));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].function_name, "tangled");
        assert_eq!(v[0].file_path, "src/lib.rs");
        assert_eq!(v[0].line, 5);
        assert_eq!(v[0].metric, "cognitiveComplexity");
        assert_eq!((v[0].value, v[0].threshold), (5.0, 2.0));
    }

    #[test]
    fn trf_07_finds_the_lines_that_start_a_rust_function() {
        let src = "use x;\nfn a() {}\n    pub(crate) async fn b() {\n        let f = |x| x;\n    }\npub extern \"C\" fn c() {}\n// fn not\nlet fn_ptr = 1;\n";
        assert_eq!(fn_starts(src), vec![2, 3, 6]);
    }

    /// A git repository with a cargo crate whose `tangled` function is over
    /// the threshold `clippy.toml` sets (2); `simple` is under it.
    fn crate_repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir(d.join("src")).unwrap();
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"fx\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            d.join("clippy.toml"),
            "cognitive-complexity-threshold = 2\n",
        )
        .unwrap();
        std::fs::write(d.join(".gitignore"), "target\nCargo.lock\n").unwrap();
        std::fs::write(d.join("src/lib.rs"), LIB).unwrap();
        let git = Git::new("git");
        git.run(d, &["init", "-q", "-b", "base"]).unwrap();
        commit(&git, d);
        git.run(d, &["checkout", "-q", "-b", "work"]).unwrap();
        (dir, git)
    }

    const LIB: &str = "pub fn simple(x: u32) -> u32 {
    x + 1
}

pub fn tangled(v: &[u32]) -> u32 {
    let mut n = 0;
    for a in v {
        if *a > 1 {
            for b in v {
                if *b > *a && *a % 2 == 0 || *b == 3 {
                    n += 1;
                }
            }
        }
    }
    n
}
";

    fn commit(git: &Git, d: &Path) {
        git.run(d, &["add", "-A"]).unwrap();
        git.run(
            d,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "c",
            ],
        )
        .unwrap();
    }

    /// The violations in the changed functions of `base...HEAD`.
    fn judge(git: &Git, d: &Path) -> Vec<Violation> {
        let names = git
            .run(
                d,
                &["diff", "--name-only", "--diff-filter=AMR", "base...HEAD"],
            )
            .unwrap();
        let files = rust_sources(&names, d);
        assert_eq!(files, vec!["src/lib.rs".to_string()]);
        let diff = git.run(d, &["diff", "-U0", "base...HEAD", "--"]).unwrap();
        let evidence = d.join("clippy.json");
        rust_violations(d, &files, &changed_lines(&diff), &evidence)
            .unwrap()
            .expect("clippy runs")
    }

    #[test]
    fn trf_07_clippy_fails_an_over_complex_changed_function_and_passes_a_clean_change() {
        let (dir, git) = crate_repo();
        let d = dir.path();

        // A change in `simple` only: `tangled` is over the threshold but
        // unchanged, so the step passes.
        std::fs::write(d.join("src/lib.rs"), LIB.replace("x + 1", "x + 2")).unwrap();
        commit(&git, d);
        assert_eq!(judge(&git, d), Vec::<Violation>::new());

        // A change inside `tangled`: 1 violation that names it, with the
        // threshold from clippy.toml.
        std::fs::write(
            d.join("src/lib.rs"),
            LIB.replace("x + 1", "x + 2").replace("n += 1", "n += 2"),
        )
        .unwrap();
        commit(&git, d);
        let v = judge(&git, d);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].function_name, "tangled");
        assert_eq!(v[0].file_path, "src/lib.rs");
        assert_eq!(v[0].line, 5);
        assert_eq!(v[0].threshold, 2.0);
        let out = violations(&v, &[d.join("clippy.json")]);
        assert!(
            out.note
                .contains("src/lib.rs:5 tangled cognitiveComplexity=5 (max 2)"),
            "{out:?}"
        );
    }

    #[test]
    fn trf_07_rust_files_outside_a_cargo_project_are_not_sent_to_clippy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rust_sources("a.rs\nb.ts\n", dir.path()).is_empty());
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(
            rust_sources("a.rs\nb.ts\n", dir.path()),
            vec!["a.rs".to_string()]
        );
    }
}
