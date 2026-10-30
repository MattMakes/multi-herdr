//! The complexity-sweep step (#38): the `complexity-sweep` skill's analyser
//! on the files the branch changed. It passes only when no function the
//! branch changed has a violation. horch alone decides "not applicable":
//! when no changed file is in a language the analyser reads.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Ctx, Outcome};

/// The extensions the analyser parses: TypeScript and JavaScript (its core
/// is `ts-morph`), and Python. It has no Rust or GDScript parser: it reads
/// those files as TypeScript and reports functions that do not exist, so
/// they are not sent to it (#204).
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

#[derive(Debug, Clone, Deserialize)]
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
            continue;
        }
        let Some(rest) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some(plus) = rest.split_whitespace().find(|t| t.starts_with('+')) else {
            continue;
        };
        let plus = &plus[1..];
        let (start, count) = match plus.split_once(',') {
            Some((s, c)) => (s.parse::<u64>().unwrap_or(0), c.parse::<u64>().unwrap_or(1)),
            None => (plus.parse::<u64>().unwrap_or(0), 1),
        };
        if count == 0 {
            continue;
        }
        if let Some(f) = &file {
            out.entry(f.clone())
                .or_default()
                .push((start, start + count - 1));
        }
    }
    out
}

/// The violations in functions whose span (its line to the line before the
/// next function in the file) holds a changed line.
pub fn in_changed_functions(
    json: &str,
    changed: &BTreeMap<String, Vec<(u64, u64)>>,
    root: &Path,
) -> Result<Vec<Violation>> {
    let a: Analysis = serde_json::from_str(json).context("reading the analyser's JSON")?;
    let rel = |p: &str| -> String {
        let path = Path::new(p);
        path.strip_prefix(root)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.trim_start_matches("./").to_string())
    };
    let mut starts: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for f in &a.files {
        let mut lines: Vec<u64> = f.functions.iter().map(|x| x.line).collect();
        lines.sort();
        starts.insert(rel(&f.file_path), lines);
    }
    let mut out = Vec::new();
    for v in a.violations {
        let file = rel(&v.file_path);
        let Some(ranges) = changed.get(&file) else {
            continue;
        };
        let end = starts
            .get(&file)
            .and_then(|s| s.iter().find(|l| **l > v.line).map(|l| l - 1))
            .unwrap_or(u64::MAX);
        if ranges.iter().any(|(a, b)| *a <= end && *b >= v.line) {
            out.push(v);
        }
    }
    Ok(out)
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
    let files: Vec<String> = names
        .lines()
        .filter(|f| {
            Path::new(f)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e))
        })
        .map(|s| s.to_string())
        .collect();
    let tree = git.tree(&cx.dir).ok();
    if files.is_empty() {
        return Ok(
            Outcome::not_applicable("no changed file is in a language the analyser reads")
                .judged(tree),
        );
    }
    let Some(cli) = &cx.tools.complexity_cli else {
        return Ok(Outcome::fail(
            "no analyser",
            None,
            "the complexity-sweep skill's analyser was not found. Install the code plugin's complexity-sweep skill (and run npm install in its code/core and code/cli), or set HORCH_COMPLEXITY_CLI to its code/cli/dist/index.js",
        ));
    };
    let mut cmd = if cli.extension().is_some_and(|e| e == "js") {
        let mut c = Command::new(&cx.tools.node);
        c.arg(cli);
        c
    } else {
        Command::new(cli)
    };
    cmd.args(["--format", "json", "--threshold-action", "none"])
        .args(&files)
        .current_dir(&cx.dir);
    crate::runtime::process::scrub_child_env(&mut cmd);
    // The report goes to a file, not a pipe: node exits before a piped
    // stdout drains, and a pipe keeps only the first 64 KiB.
    let evidence = cx.evidence_dir.join(format!("{}-complexity.json", cx.seq));
    cmd.stdout(std::fs::File::create(&evidence)?);
    let out = cmd
        .output()
        .with_context(|| format!("running {}", cli.display()))?;
    let json = std::fs::read_to_string(&evidence)?;
    if json.trim().is_empty() {
        return Ok(Outcome::fail(
            "no output",
            Some(evidence),
            &format!(
                "the analyser printed nothing: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    let diff = git.run(&cx.dir, &["diff", "-U0", &format!("{base}...HEAD"), "--"])?;
    let changed = changed_lines(&diff);
    let found = in_changed_functions(&json, &changed, &cx.dir)?;
    if found.is_empty() {
        return Ok(Outcome::pass(
            "0 violations",
            Some(evidence),
            &format!("{} files analysed", files.len()),
        )
        .judged(tree));
    }
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
    Ok(Outcome::fail(
        &format!("{} violations", found.len()),
        Some(evidence.clone()),
        &format!(
            "reduce the complexity of the functions you changed, commit, then run finish again: {}. Full analysis: {}",
            list.join("; "),
            evidence.display()
        ),
    )
    .judged(tree))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
