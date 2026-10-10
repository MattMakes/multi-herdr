//! Source scans for the module rules of the architecture refactor.
//!
//! A scan reads the source as text, so a rule holds for code that no test
//! happens to run. Comment lines and `#[cfg(test)]` modules are skipped: a
//! test may build a `MapEnv`, and a doc comment may name what it replaced.

use std::path::{Path, PathBuf};

/// Files that still read the process environment, with the phase that
/// removes each entry. Do not add entries. The last one (`teammates.rs`) is
/// gone.
const PENDING: &[&str] = &[];

/// Ambient environment access. `std::env::consts` is allowed.
const AMBIENT: &[&str] = &[
    "std::env::var",
    "env::var_os",
    "set_var",
    "remove_var",
    "std::env::current_dir",
    "std::env::temp_dir",
];

fn crate_src(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name)
        .join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The lines of `path` that are code outside tests, numbered from 1. A file
/// named `tests.rs` is a `#[cfg(test)]` module; a file's own `#[cfg(test)]`
/// module ends the scan of that file.
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    if path.file_name().is_some_and(|n| n == "tests.rs") {
        return Vec::new();
    }
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((i, line)) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]")
            && lines
                .peek()
                .is_some_and(|(_, next)| next.trim_start().starts_with("mod "))
        {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        out.push((i + 1, line.to_string()));
    }
    out
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// Domain code never calls `std::env`: only `runtime/` reads the process
/// environment, and the `horch` binary's bootstrap builds the context.
#[test]
fn arc_05_no_ambient_env_in_core() {
    let root = crate_src("horch-core");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(files.len() > 20, "scanned only {} files", files.len());

    let mut found = Vec::new();
    let mut pending_used = Vec::new();
    for path in &files {
        let rel = relative(path, &root);
        if rel.starts_with("runtime/") {
            continue;
        }
        for (n, line) in code_lines(path) {
            if let Some(needle) = AMBIENT.iter().find(|a| line.contains(*a)) {
                if PENDING.contains(&rel.as_str()) {
                    pending_used.push(rel.clone());
                    continue;
                }
                found.push(format!("{rel}:{n}: {needle}: {}", line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "ambient environment access outside runtime/:\n{}",
        found.join("\n")
    );
    // An entry nothing needs any more must go, so the list only shrinks.
    for entry in PENDING {
        assert!(
            pending_used.iter().any(|p| p == entry),
            "PENDING entry {entry} reads no environment any more; remove it"
        );
    }
}

/// The environment is the child's transport, not this process's scratch
/// space. A2 began with 44 environment access sites; the mutations among
/// them are gone from both crates.
#[test]
fn arc_06_env_mutation_sites_reduced() {
    let mut count = 0;
    let mut sites = Vec::new();
    for name in ["horch-core", "horch"] {
        let root = crate_src(name);
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for path in &files {
            for (n, line) in code_lines(path) {
                if line.contains("set_var(") || line.contains("remove_var(") {
                    count += 1;
                    sites.push(format!("{name}/{}:{n}", relative(path, &root)));
                }
            }
        }
    }
    println!("environment mutation sites outside tests: {count} {sites:?}");
    assert!(count < 44, "{count} sites: {sites:?}");
    assert_eq!(count, 0, "no process environment mutation left: {sites:?}");
}

/// The scan's own filter: test modules and comment lines are not code.
#[test]
fn arc_05_scan_skips_tests_and_comments() {
    let tmp = std::env::temp_dir().join(format!("arch-scan-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let file = tmp.join("x.rs");
    std::fs::write(
        &file,
        "// std::env::var in a comment\nfn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { std::env::set_var(\"A\", \"1\"); }\n}\n",
    )
    .unwrap();
    let lines = code_lines(&file);
    std::fs::remove_dir_all(&tmp).unwrap();
    assert_eq!(lines, vec![(2, "fn a() {}".to_string())]);
}

/// Code that matches on text that looks like error text, each with the
/// reason it is not a Rust error value. Do not add entries without a reason.
///
/// - `horch-core/telemetry/readers.rs`, `FreeUsageLimitError`: the `error`
///   field of an OpenCode message row is external data from OpenCode's
///   database, not an error this code produced. Its name is the only signal.
const ALLOWED: &[(&str, &str)] = &[("horch-core/telemetry/readers.rs", "FreeUsageLimitError")];

/// The needle in `line` that matches on an error's text, if any.
fn error_text_match(line: &str) -> Option<&'static str> {
    const RECEIVERS: [&str; 5] = ["e", "err", "error", "why", "cause"];
    if line.contains(".to_string().contains(") {
        return Some(".to_string().contains(");
    }
    if line.contains("format!(\"{") && line.contains("\").contains(") {
        return Some("format!(\"{..}\").contains(");
    }
    if line.contains(".contains(\"error") {
        return Some(".contains(\"error");
    }
    let mut rest = line;
    while let Some(at) = rest.find(".to_string() ==") {
        let receiver = rest[..at]
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default();
        if RECEIVERS.contains(&receiver) {
            return Some(".to_string() ==");
        }
        rest = &rest[at + 1..];
    }
    None
}

/// Application code branches on typed errors, never on their message text:
/// a reworded message must not change behavior (ARC-22).
#[test]
fn arc_22_no_error_string_matching() {
    let mut found = Vec::new();
    let mut allowed_used = Vec::new();
    let mut scanned = 0;
    for name in ["horch-core", "horch"] {
        let root = crate_src(name);
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        scanned += files.len();
        for path in &files {
            let rel = format!("{name}/{}", relative(path, &root));
            for (n, line) in code_lines(path) {
                let Some(needle) = error_text_match(&line) else {
                    continue;
                };
                if let Some(entry) = ALLOWED
                    .iter()
                    .find(|(file, text)| *file == rel && line.contains(text))
                {
                    allowed_used.push(*entry);
                    continue;
                }
                found.push(format!("{rel}:{n}: {needle}: {}", line.trim()));
            }
        }
    }
    assert!(scanned > 40, "scanned only {scanned} files");
    assert!(
        found.is_empty(),
        "matching on error text outside tests:\n{}",
        found.join("\n")
    );
    for entry in ALLOWED {
        assert!(
            allowed_used.contains(entry),
            "ALLOWED entry {entry:?} matches nothing any more; remove it"
        );
    }
}

/// The ARC-22 matcher's own cases.
#[test]
fn arc_22_matcher_finds_error_text_checks() {
    for hit in [
        "if e.to_string().contains(\"not found\") {",
        "if format!(\"{e:#}\").contains(\"locked\") {",
        "if err.to_string() == \"boom\" {",
        "Err(why) if why.to_string() == \"x\" => {}",
        "if msg.contains(\"error: no such pane\") {",
    ] {
        assert!(error_text_match(hit).is_some(), "missed: {hit}");
    }
    for miss in [
        "if role.to_string() == wanted {",
        "reason: why.to_string(),",
        "if line.contains(\"not installed\") {",
    ] {
        assert_eq!(error_text_match(miss), None, "false hit: {miss}");
    }
}

/// Files outside `harness/` that still match on a harness variant, with the
/// phase that removes each entry. Do not add entries. A10 removed the last
/// one (`skills.rs`).
const HARNESS_MATCH_PENDING: &[&str] = &[];

/// Whether `line` is a match arm (or a `matches!`) on a `HarnessKind` /
/// `Agent` variant. A comparison such as `t.agent == Agent::None` or an
/// assignment is not a match.
fn harness_match(line: &str) -> bool {
    for prefix in ["HarnessKind::", "Agent::"] {
        for (i, _) in line.match_indices(prefix) {
            // `Agent::` inside a longer path segment, e.g. `SubAgent::`.
            if line[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            // The right side of a comparison, even inside a match guard.
            let before = line[..i].trim_end();
            if before.ends_with("==") || before.ends_with("!=") {
                continue;
            }
            let rest = &line[i + prefix.len()..];
            if !rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            let after = rest
                .trim_start_matches(|c: char| c.is_alphanumeric() || c == '_')
                .trim_start();
            if line.contains("matches!(")
                || after.starts_with("=>")
                || (after.starts_with('|') && !after.starts_with("||"))
            {
                return true;
            }
        }
    }
    false
}

/// Adding a harness is one new module: outside tests, harness variants are
/// matched only under `harness/` and in `roster/validation.rs`.
#[test]
fn arc_10_harness_match_only_in_harness() {
    let mut found = Vec::new();
    let mut pending_used = Vec::new();
    for name in ["horch-core", "horch"] {
        let root = crate_src(name);
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for path in &files {
            let rel = relative(path, &root);
            if name == "horch-core"
                && (rel.starts_with("harness/") || rel == "roster/validation.rs")
            {
                continue;
            }
            for (n, line) in code_lines(path) {
                if !harness_match(&line) {
                    continue;
                }
                if name == "horch-core" && HARNESS_MATCH_PENDING.contains(&rel.as_str()) {
                    pending_used.push(rel.clone());
                    continue;
                }
                found.push(format!("{name}/{rel}:{n}: {}", line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "harness variant matches outside harness/:\n{}",
        found.join("\n")
    );
    for entry in HARNESS_MATCH_PENDING {
        assert!(
            pending_used.iter().any(|p| p == entry),
            "HARNESS_MATCH_PENDING entry {entry} matches no harness any more; remove it"
        );
    }
}

/// The scan's own filter: arms and `matches!` count, comparisons do not.
#[test]
fn arc_10_scan_tells_matches_from_comparisons() {
    assert!(harness_match("        Agent::Claude => 1,"));
    assert!(harness_match(
        "        HarnessKind::Pi | HarnessKind::Prime => 2,"
    ));
    assert!(harness_match("    matches!(kind, Agent::Codex)"));
    assert!(!harness_match("    if t.agent == Agent::None {"));
    assert!(!harness_match("        teammate.agent = Agent::Codex;"));
    assert!(!harness_match("    a == Agent::Pi || b"));
    assert!(!harness_match(
        "        None if t.agent == Agent::Codex => 1,"
    ));
    assert!(!harness_match("    SubAgent::Claude => 1,"));
}

/// The core skill model knows no harness: no file under `skills/` (nor
/// `skills.rs`) names a harness variant or one harness's way of exposing
/// skills. Each adapter's `expose_skills` owns that (SKL-05). The whole text
/// is scanned, tests and comments included.
#[test]
fn skl_05_core_skill_model_has_no_harness_flags() {
    const FORBIDDEN: [&str; 6] = [
        "HarnessKind",
        "Agent::",
        "--plugin-dir",
        "--skill",
        "OPENCODE_CONFIG_CONTENT",
        "CODEX_HOME",
    ];
    let root = crate_src("horch-core");
    let mut files = vec![root.join("skills.rs")];
    rust_files(&root.join("skills"), &mut files);
    assert!(files.len() >= 6, "scanned only {files:?}");
    let mut found = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for (n, line) in text.lines().enumerate() {
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    found.push(format!("{}:{}: {needle}", relative(path, &root), n + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "harness exposure in the core skill model:\n{}",
        found.join("\n")
    );
}

// ─── ARC-25 ─────────────────────────────────────────────────────────────────

/// `pub use` lines that re-export an item from outside the file's own child
/// modules, each with the reason. Do not add entries without a reason.
///
/// - `skills/catalog.rs`, `horch_marketplace as marketplace`: the `horch`
///   binary may depend on `horch-core` only (NFR-05), and it drives the
///   marketplace through this path.
const REEXPORT_ALLOWED: &[(&str, &str)] = &[(
    "horch-core/skills/catalog.rs",
    "horch_marketplace as marketplace",
)];

/// The top-level `horch-core` files that were re-export shims (or the
/// `ledger.rs` facade) during the refactor. None may come back.
const REMOVED_SHIM_FILES: [&str; 18] = [
    "agent.rs",
    "balance.rs",
    "balance_policy.rs",
    "codex.rs",
    "herdr.rs",
    "launch.rs",
    "layout.rs",
    "ledger.rs",
    "mailbox.rs",
    "message.rs",
    "opencode.rs",
    "paneshell.rs",
    "plugins.rs",
    "policy.rs",
    "prime.rs",
    "quota.rs",
    "teammates.rs",
    "tile.rs",
];

/// Signatures of the wrappers the refactor kept for old callers. Matched
/// against code lines outside tests.
const REMOVED_SHIM_ITEMS: [&str; 17] = [
    "fn from_process(",
    "fn send_line(&self",
    "fn ensure_supported(",
    "fn configure(&self, teammate",
    "fn apply_env(",
    "fn briefing(&self",
    "fn resolve_all(",
    "fn mints_session_id(",
    "fn harvests_session_id(",
    "fn runs_a_daemon(",
    "fn uses_execpolicy(",
    "fn load_with(",
    "fn state_root()",
    "fn project_dir()",
    "pub fn command(",
    "pub fn command_with_skills(",
    "HarnessKind as Agent",
];

/// The child modules a file declares (`mod x;`, `pub mod x;`, ...).
fn child_modules(lines: &[(usize, String)]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|(_, line)| {
            let t = line.trim();
            let rest = t.strip_prefix("pub ").unwrap_or(t);
            let rest = rest
                .strip_prefix("pub(crate) ")
                .or_else(|| rest.strip_prefix("pub(super) "))
                .unwrap_or(rest);
            let name = rest.strip_prefix("mod ")?.strip_suffix(';')?;
            Some(name.trim().to_string())
        })
        .collect()
}

/// The re-export on `line`, if it is a `pub use` (any visibility).
fn reexport(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix("pub ").or_else(|| {
        t.strip_prefix("pub(")
            .and_then(|r| r.split_once(") ").map(|(_, r)| r))
    })?;
    rest.strip_prefix("use ")
}

/// The refactor ends with no shim: no file only re-exports, every `pub use`
/// names one of the file's own child modules, and none of the removed shim
/// files or wrappers exist (ARC-25).
#[test]
fn arc_25_no_shim_modules() {
    let core = crate_src("horch-core");
    for file in REMOVED_SHIM_FILES {
        assert!(!core.join(file).exists(), "horch-core/src/{file} is back");
    }

    let mut found = Vec::new();
    let mut allowed_used = Vec::new();
    let mut scanned = 0;
    for name in ["horch-core", "horch", "horch-marketplace", "horch-e2e"] {
        let root = crate_src(name);
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        scanned += files.len();
        for path in &files {
            let rel = format!("{name}/{}", relative(path, &root));
            let lines = code_lines(path);
            let children = child_modules(&lines);
            let code: Vec<&str> = lines
                .iter()
                .map(|(_, l)| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with("#!") && !l.starts_with("#["))
                .collect();
            if children.is_empty()
                && !code.is_empty()
                && code
                    .iter()
                    .all(|l| l.starts_with("use ") || reexport(l).is_some())
                && code.iter().any(|l| reexport(l).is_some())
            {
                found.push(format!("{rel}: the file only re-exports"));
            }
            for (n, line) in &lines {
                for item in REMOVED_SHIM_ITEMS {
                    if line.contains(item) {
                        found.push(format!("{rel}:{n}: removed shim item `{item}`"));
                    }
                }
                let Some(target) = reexport(line) else {
                    continue;
                };
                let first = target
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default();
                if children.iter().any(|c| c == first) || first == "self" {
                    continue;
                }
                if let Some(entry) = REEXPORT_ALLOWED
                    .iter()
                    .find(|(file, text)| *file == rel && line.contains(text))
                {
                    allowed_used.push(*entry);
                    continue;
                }
                found.push(format!(
                    "{rel}:{n}: re-export from outside the file's modules: {}",
                    line.trim()
                ));
            }
        }
    }
    assert!(scanned > 60, "scanned only {scanned} files");
    assert!(found.is_empty(), "shims left:\n{}", found.join("\n"));
    for entry in REEXPORT_ALLOWED {
        assert!(
            allowed_used.contains(entry),
            "REEXPORT_ALLOWED entry {entry:?} matches nothing any more; remove it"
        );
    }
}

/// The ARC-25 scan's own cases: a glob shim is caught, a module's own
/// re-exports are not.
#[test]
fn arc_25_scan_tells_shims_from_module_apis() {
    let lines = |src: &str| -> Vec<(usize, String)> {
        src.lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect()
    };
    assert_eq!(
        child_modules(&lines("pub mod a;\nmod b;\npub(crate) mod c;\nfn d() {}")),
        ["a", "b", "c"]
    );
    assert_eq!(reexport("pub use crate::x::*;"), Some("crate::x::*;"));
    assert_eq!(reexport("pub(crate) use a::B;"), Some("a::B;"));
    assert_eq!(reexport("use a::B;"), None);
    assert_eq!(reexport("pub fn used() {}"), None);
}

// ─── Spec A §3 module rules ─────────────────────────────────────────────────

/// Lines of the code in `files` (outside tests and comments) that name a
/// `forbidden` item, as `file:line: needle`.
fn forbidden_lines(root: &Path, files: &[PathBuf], forbidden: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for path in files {
        for (n, line) in code_lines(path) {
            if let Some(needle) = forbidden.iter().find(|f| line.contains(*f)) {
                found.push(format!("{}:{n}: {needle}", relative(path, root)));
            }
        }
    }
    found
}

/// Telemetry observes and does not route (Spec A §3): it reads quota and
/// executions, and never makes or builds a routing decision.
#[test]
fn arc_23_telemetry_never_routes() {
    let root = crate_src("horch-core");
    let mut files = Vec::new();
    rust_files(&root.join("telemetry"), &mut files);
    assert!(files.len() >= 4, "scanned only {files:?}");
    let found = forbidden_lines(
        &root,
        &files,
        &[
            "routing::balance",
            "routing::decision",
            "routing::eligible",
            "RoutingDecision",
            "decide(",
        ],
    );
    assert!(found.is_empty(), "telemetry routes:\n{}", found.join("\n"));
}

/// Prompts hold no execution policy (Spec A §3): the prompt text never
/// branches on a harness, a balance mode, an execution state or routing.
#[test]
fn arc_10_prompts_hold_no_execution_policy() {
    let root = crate_src("horch-core");
    let files = [root.join("prompts.rs")];
    let found = forbidden_lines(
        &root,
        &files,
        &["HarnessKind", "BalanceMode", "ExecutionStatus", "routing::"],
    );
    assert!(
        found.is_empty(),
        "execution policy in prompts:\n{}",
        found.join("\n")
    );
}

/// The evaluation domain has no adapters (Spec A §3, CMP-02): the winner
/// rule and the judgment parser start no process and touch no workspace,
/// git, environment or file.
#[test]
fn cmp_02_evaluation_domain_has_no_adapter_imports() {
    let root = crate_src("horch-core");
    let files = [
        root.join("evaluation/winner.rs"),
        root.join("evaluation/parser.rs"),
    ];
    let found = forbidden_lines(
        &root,
        &files,
        &[
            "std::process",
            "Command",
            "herdr",
            "workspace::",
            "launch::",
            "vcs::",
            "std::env",
            "std::fs",
        ],
    );
    assert!(
        found.is_empty(),
        "adapters in the evaluation domain:\n{}",
        found.join("\n")
    );
}

/// The compaction rules are pure (CTX-07): windows, thresholds and the row
/// policy start no process and touch no file, environment or workspace.
#[test]
fn ctx_07_compaction_pure_modules_do_no_io() {
    let root = crate_src("horch-core");
    let files = [
        root.join("compaction/window.rs"),
        root.join("compaction/policy.rs"),
    ];
    for file in &files {
        assert!(file.is_file(), "{} is missing", file.display());
    }
    let found = forbidden_lines(
        &root,
        &files,
        &[
            "std::fs",
            "std::process",
            "std::env",
            "Command",
            "herdr",
            "workspace::",
            "launch::",
        ],
    );
    assert!(
        found.is_empty(),
        "I/O in the pure compaction modules:\n{}",
        found.join("\n")
    );
}
