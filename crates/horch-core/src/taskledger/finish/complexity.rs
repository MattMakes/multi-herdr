use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Ctx, Outcome};

mod config;

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

fn hunk_range(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = &rest.split_whitespace().find(|t| t.starts_with('+'))?[1..];
    let (start, count) = match plus.split_once(',') {
        Some((s, c)) => (s.parse::<u64>().unwrap_or(0), c.parse::<u64>().unwrap_or(1)),
        None => (plus.parse::<u64>().unwrap_or(0), 1),
    };
    (count > 0).then(|| (start, start + count - 1))
}

fn rel(p: &str, root: &Path) -> String {
    Path::new(p)
        .strip_prefix(root)
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.trim_start_matches("./").to_string())
}

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

const CLIPPY_LINTS: &[(&str, &str)] = &[
    ("clippy::cognitive_complexity", "cognitiveComplexity"),
    ("clippy::too_many_lines", "functionLength"),
    ("clippy::too_many_arguments", "parameterCount"),
];

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

fn metric(d: &Diagnostic) -> Option<&'static str> {
    let code = d.code.as_ref()?.code.as_str();
    CLIPPY_LINTS
        .iter()
        .find(|(lint, _)| *lint == code)
        .map(|(_, m)| *m)
}

fn work_rel(file: &str, root: &Path, dir: &Path) -> String {
    let joined = root.join(file);
    let full = joined.canonicalize().unwrap_or_else(|_| joined.clone());
    let base = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    full.strip_prefix(&base)
        .or_else(|_| joined.strip_prefix(dir))
        .map(|r| r.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file.trim_start_matches("./").to_string())
}

#[derive(Debug, Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
}

fn workspace_root(krate: &Path) -> Result<std::result::Result<PathBuf, String>> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(krate);
    crate::runtime::process::scrub_child_env(&mut cmd);
    let out = cmd.output().context("running cargo metadata")?;
    if !out.status.success() {
        return Ok(Err(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    let m: Metadata =
        serde_json::from_slice(&out.stdout).context("reading the cargo metadata JSON")?;
    Ok(Ok(m.workspace_root))
}

fn push_new(out: &mut Vec<Violation>, v: Violation) {
    if !out
        .iter()
        .any(|o| o.file_path == v.file_path && o.line == v.line && o.metric == v.metric)
    {
        out.push(v);
    }
}

fn clippy_violation(line: &str, root: &Path, dir: &Path) -> Option<Violation> {
    let CargoLine { reason, message } = serde_json::from_str::<CargoLine>(line).ok()?;
    let d = message.filter(|_| reason == "compiler-message")?;
    let metric = metric(&d)?;
    let span = d.spans.iter().find(|s| s.is_primary)?;
    let (value, threshold) = score(&d.message).unwrap_or((0.0, 0.0));
    Some(Violation {
        function_name: name(span),
        file_path: work_rel(&span.file_name, root, dir),
        line: span.line_start,
        metric: metric.to_string(),
        value,
        threshold,
    })
}

pub fn clippy_violations(json: &str, root: &Path, dir: &Path) -> Vec<Violation> {
    let mut out: Vec<Violation> = Vec::new();
    for v in json.lines().filter_map(|l| clippy_violation(l, root, dir)) {
        push_new(&mut out, v);
    }
    out
}

fn score(message: &str) -> Option<(f64, f64)> {
    let inner = message.rsplit_once('(')?.1.split_once(')')?.0;
    let (value, threshold) = inner.split_once('/')?;
    Some((value.trim().parse().ok()?, threshold.trim().parse().ok()?))
}

fn name(span: &Span) -> String {
    let Some(t) = span.text.first() else {
        return String::new();
    };
    let tail = t.text.chars().skip(span.column_start.saturating_sub(1));
    let slice: String = if span.text.len() == 1 {
        tail.take(span.column_end.saturating_sub(span.column_start))
            .collect()
    } else {
        tail.collect()
    };
    signature_name(&slice).unwrap_or(slice)
}

fn signature_name(slice: &str) -> Option<String> {
    let mut words = slice.split_whitespace();
    words.find(|w| *w == "fn")?;
    let ident = words.next()?.split(['<', '(']).next()?;
    Some(ident.to_string())
}

fn fn_starts(source: &str) -> Vec<u64> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| starts_fn(l))
        .map(|(i, _)| i as u64 + 1)
        .collect()
}

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

fn nearest_crate(dir: &Path, file: &str) -> Option<PathBuf> {
    let mut folder = dir.join(file).parent()?.to_path_buf();
    loop {
        if folder.join("Cargo.toml").is_file() {
            return Some(folder);
        }
        if folder == dir || !folder.pop() || !folder.starts_with(dir) {
            return None;
        }
    }
}

fn rust_crates(names: &str, dir: &Path) -> BTreeMap<PathBuf, Vec<String>> {
    let mut by: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for f in names.lines().filter(|f| f.ends_with(".rs")) {
        if let Some(krate) = nearest_crate(dir, f) {
            by.entry(krate).or_default().push(f.to_string());
        }
    }
    by
}

fn clippy_evidence(evidence_dir: &Path, seq: i64, i: usize) -> PathBuf {
    evidence_dir.join(match i {
        0 => format!("{seq}-clippy.json"),
        n => format!("{seq}-clippy-{}.json", n + 1),
    })
}

fn clippy(krate: &Path) -> Command {
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
    ]);
    for (lint, _) in CLIPPY_LINTS {
        cmd.args(["-W", lint]);
    }
    cmd.args(["--cap-lints", "warn"]).current_dir(krate);
    crate::runtime::process::scrub_child_env(&mut cmd);
    cmd
}

fn analyse_rust(
    krate: &Path,
    dir: &Path,
    evidence: &Path,
) -> Result<std::result::Result<Vec<Violation>, Outcome>> {
    let root = match workspace_root(krate)? {
        Ok(root) => root,
        Err(stderr) => {
            return Ok(Err(Outcome::fail(
                "cargo metadata failed",
                None,
                &format!(
                    "cargo metadata failed in {}; fix the crate, commit, then run finish again: {stderr}",
                    krate.display()
                ),
            )))
        }
    };
    let mut cmd = clippy(krate);
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
    Ok(Ok(clippy_violations(&json, &root, dir)))
}

fn rust_starts(dir: &Path, files: &[String]) -> BTreeMap<String, Vec<u64>> {
    files
        .iter()
        .map(|f| {
            let text = std::fs::read_to_string(dir.join(f)).unwrap_or_default();
            (f.clone(), fn_starts(&text))
        })
        .collect()
}

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

const NO_SOURCES: &str = "No source files found matching the provided patterns";

struct Analyser<'a> {
    node: &'a Path,
    cli: &'a Path,
    dir: &'a Path,
}

impl Analyser<'_> {
    fn command(&self, files: &[String], config: Option<&Path>) -> Command {
        let mut cmd = if self.cli.extension().is_some_and(|e| e == "js") {
            let mut c = Command::new(self.node);
            c.arg(self.cli);
            c
        } else {
            Command::new(self.cli)
        };
        cmd.args(["--format", "json", "--threshold-action", "none"]);
        if let Some(config) = config {
            cmd.arg("--config").arg(config);
        }
        cmd.args(files).current_dir(self.dir);
        crate::runtime::process::scrub_child_env(&mut cmd);
        cmd
    }
}

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

impl Analyser<'_> {
    fn run(
        &self,
        files: &[String],
        config: Option<&Path>,
        evidence: &Path,
    ) -> Result<std::result::Result<Option<String>, Outcome>> {
        let mut cmd = self.command(files, config);
        cmd.stdout(std::fs::File::create(evidence)?);
        let out = cmd
            .output()
            .with_context(|| format!("running {}", self.cli.display()))?;
        let json = std::fs::read_to_string(evidence)?;
        if json.trim().is_empty() {
            if String::from_utf8_lossy(&out.stderr).contains(NO_SOURCES) {
                return Ok(Ok(None));
            }
            return Ok(Err(Outcome::fail(
                "no output",
                Some(evidence.to_path_buf()),
                &format!(
                    "the analyser printed nothing: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            )));
        }
        Ok(Ok(Some(json)))
    }

    fn run_groups(
        &self,
        files: &[String],
        evidence_dir: &Path,
        seq: i64,
    ) -> Result<std::result::Result<Vec<(PathBuf, String)>, Outcome>> {
        let mut reports = Vec::new();
        for (i, (cfg, group)) in config::groups(self.dir, files).into_iter().enumerate() {
            let name = match i {
                0 => format!("{seq}-complexity.json"),
                n => format!("{seq}-complexity-{}.json", n + 1),
            };
            let path = evidence_dir.join(name);
            match self.run(&group, cfg.as_deref(), &path)? {
                Ok(Some(json)) => reports.push((path, json)),
                Ok(None) => {}
                Err(failed) => return Ok(Err(failed)),
            }
        }
        Ok(Ok(reports))
    }
}

type RustFindings = (Vec<Violation>, Vec<PathBuf>);

pub fn rust_violations(
    dir: &Path,
    crates: &BTreeMap<PathBuf, Vec<String>>,
    changed: &BTreeMap<String, Vec<(u64, u64)>>,
    evidence_dir: &Path,
    seq: i64,
) -> Result<std::result::Result<RustFindings, Outcome>> {
    let mut found = Vec::new();
    let mut evidence = Vec::new();
    for (i, (krate, files)) in crates.iter().enumerate() {
        let path = clippy_evidence(evidence_dir, seq, i);
        let group = match analyse_rust(krate, dir, &path)? {
            Ok(group) => group,
            Err(failed) => return Ok(Err(failed)),
        };
        for v in keep_changed(group, &rust_starts(dir, files), changed) {
            push_new(&mut found, v);
        }
        evidence.push(path);
    }
    Ok(Ok((found, evidence)))
}

pub fn step(cx: &Ctx<'_>) -> Result<Outcome> {
    let git = &cx.tools.git;
    let base = cx.diff_base()?;
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
    let rust = rust_crates(&names, &cx.dir);
    let tree = git.tree(&cx.dir).ok();
    if files.is_empty() && rust.is_empty() {
        return Ok(
            Outcome::not_applicable(
                "no changed file is in a language that the analyser (TypeScript, JavaScript, Python) or clippy (Rust) reads",
            )
                .judged(tree),
        );
    }
    let mut found = Vec::new();
    let mut evidence = Vec::new();
    let mut reports = Vec::new();
    if !files.is_empty() {
        let Some(cli) = &cx.tools.complexity_cli else {
            return Ok(Outcome::fail(
                "no analyser",
                None,
                "the complexity-sweep skill's analyser was not found. Install the code plugin's complexity-sweep skill (and run npm install in its code/core and code/cli), or set HORCH_COMPLEXITY_CLI to its code/cli/dist/index.js",
            ));
        };
        let analyser = Analyser {
            node: &cx.tools.node,
            cli,
            dir: &cx.dir,
        };
        match analyser.run_groups(&files, &cx.evidence_dir, cx.seq)? {
            Ok(r) => reports = r,
            Err(failed) => return Ok(failed),
        }
        if reports.is_empty() && rust.is_empty() {
            return Ok(Outcome::not_applicable("no analysable source files").judged(tree));
        }
    }
    let diff = git.run(&cx.dir, &["diff", "-U0", &format!("{base}...HEAD"), "--"])?;
    let changed = changed_lines(&diff);
    for (path, json) in reports {
        found.extend(in_changed_functions(&json, &changed, &cx.dir)?);
        evidence.push(path);
    }
    match rust_violations(&cx.dir, &rust, &changed, &cx.evidence_dir, cx.seq)? {
        Ok((v, paths)) => {
            found.extend(v);
            evidence.extend(paths);
        }
        Err(failed) => return Ok(failed),
    }
    let rust_files: usize = rust.values().map(Vec::len).sum();
    let outcome = if found.is_empty() {
        Outcome::pass(
            "0 violations",
            evidence.first().cloned(),
            &format!("{} files analysed", files.len() + rust_files),
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
        let v = clippy_violations(&json, Path::new("/repo"), Path::new("/repo"));
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

    fn judge(git: &Git, d: &Path) -> Vec<Violation> {
        let names = git
            .run(
                d,
                &["diff", "--name-only", "--diff-filter=AMR", "base...HEAD"],
            )
            .unwrap();
        let crates = rust_crates(&names, d);
        assert_eq!(
            crates,
            BTreeMap::from([(d.to_path_buf(), vec!["src/lib.rs".to_string()])])
        );
        let diff = git.run(d, &["diff", "-U0", "base...HEAD", "--"]).unwrap();
        let (found, evidence) = rust_violations(d, &crates, &changed_lines(&diff), d, 0)
            .unwrap()
            .expect("clippy runs");
        assert_eq!(evidence, vec![d.join("0-clippy.json")]);
        found
    }

    #[test]
    fn trf_07_clippy_fails_an_over_complex_changed_function_and_passes_a_clean_change() {
        let (dir, git) = crate_repo();
        let d = dir.path();

        std::fs::write(d.join("src/lib.rs"), LIB.replace("x + 1", "x + 2")).unwrap();
        commit(&git, d);
        assert_eq!(judge(&git, d), Vec::<Violation>::new());

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
        let out = violations(&v, &[d.join("0-clippy.json")]);
        assert!(
            out.note
                .contains("src/lib.rs:5 tangled cognitiveComplexity=5 (max 2)"),
            "{out:?}"
        );
    }

    const FAKE: &str = r#"cfg=default
files=
while [ $# -gt 0 ]; do
  case "$1" in
    --config) cfg="$2"; shift ;;
    --format|--threshold-action) shift ;;
    *) files="$files $1" ;;
  esac
  shift
done
printf '{"files":[],"violations":['
sep=
for f in $files; do
  printf '%s{"functionName":"f","filePath":"%s","line":1,"metric":"%s","value":2,"threshold":1}' "$sep" "$f" "$cfg"
  sep=,
done
printf ']}\n'
"#;

    #[test]
    fn t_35_each_file_is_judged_by_its_nearest_config_and_both_groups_report() {
        let top = tempfile::tempdir().unwrap();
        let dir = top.path().join("repo");
        let evidence = top.path().join("evidence");
        std::fs::create_dir_all(dir.join("skills/hark/engine/hark")).unwrap();
        std::fs::create_dir_all(&evidence).unwrap();
        let cfg = dir.join("skills/hark/engine/.static-analysisrc.json");
        std::fs::write(&cfg, "{}").unwrap();
        let cli = top.path().join("cli.js");
        std::fs::write(&cli, FAKE).unwrap();
        let analyser = Analyser {
            node: Path::new("/bin/sh"),
            cli: &cli,
            dir: &dir,
        };
        let files = vec![
            "skills/hark/engine/hark/dsp.py".to_string(),
            "lib.ts".to_string(),
        ];
        let reports = analyser
            .run_groups(&files, &evidence, 3)
            .unwrap()
            .expect("the analyser runs");
        let paths: Vec<&PathBuf> = reports.iter().map(|(p, _)| p).collect();
        assert_eq!(
            paths,
            vec![
                &evidence.join("3-complexity.json"),
                &evidence.join("3-complexity-2.json")
            ]
        );
        let changed: BTreeMap<String, Vec<(u64, u64)>> =
            files.iter().map(|f| (f.clone(), vec![(1, 1)])).collect();
        let mut found = Vec::new();
        for (_, json) in &reports {
            found.extend(in_changed_functions(json, &changed, &dir).unwrap());
        }
        let judged: Vec<(&str, &str)> = found
            .iter()
            .map(|v| (v.file_path.as_str(), v.metric.as_str()))
            .collect();
        let cfg = cfg.to_string_lossy();
        assert_eq!(
            judged,
            vec![
                ("lib.ts", "default"),
                ("skills/hark/engine/hark/dsp.py", cfg.as_ref()),
            ]
        );
    }

    const EXCLUDES_ALL: &str =
        "echo 'No source files found matching the provided patterns.' >&2\nexit 2\n";

    const CRASHES: &str = "echo 'TypeError: x is undefined' >&2\nexit 1\n";

    #[test]
    fn a_group_with_no_analysable_files_gives_no_report_but_a_crash_fails() {
        let top = tempfile::tempdir().unwrap();
        let run = |script: &str| -> (Vec<(PathBuf, String)>, Option<Outcome>) {
            let cli = top.path().join("cli.js");
            std::fs::write(&cli, script).unwrap();
            let analyser = Analyser {
                node: Path::new("/bin/sh"),
                cli: &cli,
                dir: top.path(),
            };
            match analyser
                .run_groups(&["tests/test_a.py".to_string()], top.path(), 2)
                .unwrap()
            {
                Ok(reports) => (reports, None),
                Err(failed) => (Vec::new(), Some(failed)),
            }
        };
        let (reports, failed) = run(EXCLUDES_ALL);
        assert_eq!(reports, Vec::new());
        assert!(failed.is_none(), "{failed:?}");
        let failed = run(CRASHES).1.expect("a crash fails");
        assert_eq!(failed.verdict.as_deref(), Some("no output"));
        assert!(
            failed.note.contains("TypeError: x is undefined"),
            "{failed:?}"
        );
    }

    #[test]
    fn trf_07_rust_files_outside_a_cargo_project_are_not_sent_to_clippy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(rust_crates("a.rs\nb.ts\n", dir.path()).is_empty());
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(
            rust_crates("a.rs\nb.ts\n", dir.path()),
            BTreeMap::from([(dir.path().to_path_buf(), vec!["a.rs".to_string()])])
        );
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn primary(file: &str) -> String {
        format!(
            r#"{{"reason":"compiler-message","message":{{"message":"the function has a cognitive complexity of (5/2)","code":{{"code":"clippy::cognitive_complexity"}},"spans":[{{"file_name":"{file}","line_start":3,"is_primary":true,"column_start":4,"column_end":5,"text":[{{"text":"fn f() {{"}}]}}]}}}}"#
        )
    }

    #[test]
    fn grs_01_nested_cargo_crates_are_grouped_and_paths_prefixed() {
        let top = tempfile::tempdir().unwrap();
        let dir = top.path().join("work");
        touch(&top.path().join("Cargo.toml"));
        touch(&dir.join("rust/Cargo.toml"));
        touch(&dir.join("game/native/Cargo.toml"));
        let names =
            "rust/src/lib.rs\nscripts/x.rs\nrust/src/a/b.rs\ngame/native/src/lib.rs\na.ts\n";
        assert_eq!(
            rust_crates(names, &dir),
            BTreeMap::from([
                (
                    dir.join("game/native"),
                    vec!["game/native/src/lib.rs".to_string()]
                ),
                (
                    dir.join("rust"),
                    vec!["rust/src/lib.rs".to_string(), "rust/src/a/b.rs".to_string()]
                ),
            ])
        );

        let rust = dir.join("rust");
        let paths = |file: &str, root: &Path| -> Vec<String> {
            clippy_violations(&primary(file), root, &dir)
                .into_iter()
                .map(|v| v.file_path)
                .collect()
        };
        assert_eq!(paths("src/lib.rs", &rust), vec!["rust/src/lib.rs"]);
        let absolute = rust.join("src/lib.rs");
        assert_eq!(
            paths(&absolute.to_string_lossy(), &rust),
            vec!["rust/src/lib.rs"]
        );
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("crates/core/src")).unwrap();
        std::fs::write(
            dir.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        touch(&dir.join("crates/core/src/lib.rs"));
        let root = workspace_root(&dir.join("crates/core"))
            .unwrap()
            .expect("cargo metadata runs");
        assert_eq!(root, dir.canonicalize().unwrap());
        assert_eq!(
            paths("crates/core/src/lib.rs", &root),
            vec!["crates/core/src/lib.rs"],
            "a workspace member reports paths from the workspace root"
        );
    }

    #[test]
    fn grs_01_each_crate_writes_its_own_clippy_evidence() {
        let dir = Path::new("/e");
        assert_eq!(clippy_evidence(dir, 4, 0), dir.join("4-clippy.json"));
        assert_eq!(clippy_evidence(dir, 4, 1), dir.join("4-clippy-2.json"));
    }
}
