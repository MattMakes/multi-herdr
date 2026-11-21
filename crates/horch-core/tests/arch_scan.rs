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
    for needle in [
        ".to_string() ==",
        ".to_string().starts_with(",
        ".to_string().ends_with(",
    ] {
        let mut rest = line;
        while let Some(at) = rest.find(needle) {
            let before = &rest[..at];
            let receiver = before
                .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            if RECEIVERS.contains(&receiver) || before.ends_with(".root_cause()") {
                return Some(needle);
            }
            rest = &rest[at + 1..];
        }
    }
    None
}

/// `lines` with each line that starts with `.` joined to the line before
/// it, so a method chain that spans lines is matched as 1 line.
fn chained(lines: Vec<(usize, String)>) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (n, line) in lines {
        let trimmed = line.trim_start();
        match out.last_mut() {
            Some((_, prev)) if trimmed.starts_with('.') => prev.push_str(trimmed),
            _ => out.push((n, line)),
        }
    }
    out
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
            for (n, line) in chained(code_lines(path)) {
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
    let split = chained(vec![
        (
            7,
            "let not_work_tree = error.root_cause().to_string()".to_string(),
        ),
        (
            8,
            "    .starts_with(\"git rev-parse --show-toplevel failed (128):\");".to_string(),
        ),
        (9, "if err.to_string()".to_string()),
        (10, "    .ends_with(\"(128)\") {".to_string()),
    ]);
    assert_eq!(split.len(), 2, "{split:?}");
    assert_eq!((split[0].0, split[1].0), (7, 9));
    for (_, hit) in &split {
        assert!(error_text_match(hit).is_some(), "missed: {hit}");
    }
    let miss = chained(vec![
        (1, "if name.to_string()".to_string()),
        (2, "    .starts_with(\"fleet/\") {".to_string()),
    ]);
    assert_eq!(error_text_match(&miss[0].1), None, "false hit: {miss:?}");
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
/// - `harness/launch.rs`, `horch_platform::start::FORBIDDEN_ENV`: the
///   forbidden list moved to the platform start function
///   (`docs/tickets/fleetd-p7/MAP.md`, Q1), and its callers keep the old
///   path.
const REEXPORT_ALLOWED: &[(&str, &str)] = &[
    (
        "horch-core/skills/catalog.rs",
        "horch_marketplace as marketplace",
    ),
    (
        "horch-core/harness/launch.rs",
        "horch_platform::start::FORBIDDEN_ENV",
    ),
];

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

/// A child's environment is cleaned in 1 place (#88, SR-7): the platform
/// start function (`horch-platform/src/start.rs`) and the launcher of
/// fleetd remove each forbidden name. A hand-built removal of the API key
/// elsewhere is the sign of a start that goes around them.
#[test]
fn arc_88_children_are_scrubbed_in_one_place() {
    let needle = concat!("env_remove(\"ANTHROPIC", "_API_KEY\")");
    let mut found = Vec::new();
    for krate in [
        "horch-core",
        "horch",
        "horch-marketplace",
        "horch-platform",
        "herdr-install",
        "agent-docs-sync",
    ] {
        let root = crate_src(krate);
        if !root.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        files.retain(|f| {
            let rel = relative(f, &root);
            !matches!(
                (krate, rel.as_str()),
                ("horch-platform", "start.rs")
                    | ("horch-core", "fleetd/worker_profiles/adapters/launcher.rs")
            )
        });
        for line in forbidden_lines(&root, &files, &[needle]) {
            found.push(format!("{krate}/{line}"));
        }
    }
    assert!(
        found.is_empty(),
        "start the process with horch_platform::start::Command or the launcher:\n{}",
        found.join("\n")
    );
}

/// PL-2: the 1 site that cannot call the platform layer, with its reason.
const OS_CFG_EXEMPT: &[(&str, &str)] = &[(
    "horch-core/build.rs",
    "a build script runs on the build host before the crate compiles, so it cannot call the platform layer of the target",
)];

/// The predicate words that make a `cfg` an OS `cfg`.
const OS_WORDS: [&str; 4] = ["target_os", "unix", "windows", "target_family"];

/// The production code of a file text.
struct Production {
    /// The production lines, numbered from 1.
    lines: Vec<(usize, String)>,
    /// The child modules that a test-only `cfg` declares (`mod x;`).
    test_mods: Vec<String>,
}

/// The production code of `text`. Unlike `code_lines`, it skips each item
/// under a group of attributes, also over more than 1 line, that holds a
/// test-only `cfg(..)`: the item to its `;` or to the `}` that closes its
/// body. The lines after the item are code again.
fn production_lines(text: &str) -> Production {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Production {
        lines: Vec::new(),
        test_mods: Vec::new(),
    };
    let mut i = 0;
    while i < lines.len() {
        if is_comment(lines[i]) {
            i += 1;
        } else if !is_attribute(lines[i]) {
            out.lines.push((i + 1, lines[i].to_string()));
            i += 1;
        } else {
            let (group, next) = attribute_group(&lines, i);
            i = next;
            if !gates_test_only(&group) {
                out.lines.extend(group);
                continue;
            }
            if let Some(name) = lines.get(i).and_then(|l| declared_module(l)) {
                out.test_mods.push(name);
            }
            i = item_end(&lines, i);
        }
    }
    out
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

fn is_attribute(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("#[") || t.starts_with("#![")
}

/// The attributes from line `start`, each to the line of its closing `]`,
/// with the comment lines between them skipped, and the index of the line
/// after the group.
fn attribute_group(lines: &[&str], start: usize) -> (Vec<(usize, String)>, usize) {
    let mut group = Vec::new();
    let mut i = start;
    while i < lines.len() && (is_attribute(lines[i]) || is_comment(lines[i])) {
        if is_comment(lines[i]) {
            i += 1;
            continue;
        }
        let mut depth = 0;
        while i < lines.len() {
            depth += lines[i].matches('[').count() as i32 - lines[i].matches(']').count() as i32;
            group.push((i + 1, lines[i].to_string()));
            i += 1;
            if depth <= 0 {
                break;
            }
        }
    }
    (group, i)
}

/// Whether a group of attributes holds a `cfg(..)` that is test-only.
fn gates_test_only(group: &[(usize, String)]) -> bool {
    let text: String = group.iter().map(|(_, l)| format!("{l}\n")).collect();
    cfgs(&text)
        .iter()
        .any(|c| c.form == CfgForm::Gate && test_only(&c.pred))
}

/// The name of the module that `line` declares without a body (`mod x;`).
fn declared_module(line: &str) -> Option<String> {
    let item = line.trim_start();
    let item = item
        .strip_prefix("pub(crate) ")
        .or_else(|| item.strip_prefix("pub "))
        .unwrap_or(item);
    let (name, _) = item.strip_prefix("mod ")?.split_once(';')?;
    Some(name.trim().to_string())
}

/// The lexer state of `lex` between characters.
#[derive(Clone, Copy)]
enum Lex {
    Code,
    Str,
    RawStr(usize),
    Block(usize),
}

/// Rust source after `lex`: the code of each line without comments and
/// without the content of string and char literals (a string stays as
/// `""`), and each string literal with the index of its first line.
struct Lexed {
    code: Vec<String>,
    strings: Vec<(usize, String)>,
}

/// Split `lines` into code and string literals. Line comments, nested block
/// comments, string, raw string and char literals are lexed.
fn lex(lines: &[&str]) -> Lexed {
    let mut out = Lexed {
        code: Vec::new(),
        strings: Vec::new(),
    };
    let mut state = Lex::Code;
    let (mut literal, mut first) = (String::new(), 0);
    for (i, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut code = String::new();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            let next = chars.get(k + 1).copied();
            let mut close = false;
            match state {
                Lex::Str if c == '\\' => {
                    literal.push(c);
                    literal.extend(next);
                    k += 1;
                }
                Lex::Str if c == '"' => close = true,
                Lex::RawStr(hashes)
                    if c == '"'
                        && chars[k + 1..].iter().take_while(|h| **h == '#').count() >= hashes =>
                {
                    k += hashes;
                    close = true;
                }
                Lex::Str | Lex::RawStr(_) => literal.push(c),
                Lex::Block(depth) if c == '*' && next == Some('/') => {
                    k += 1;
                    state = if depth == 1 {
                        Lex::Code
                    } else {
                        Lex::Block(depth - 1)
                    };
                }
                Lex::Block(depth) if c == '/' && next == Some('*') => {
                    k += 1;
                    state = Lex::Block(depth + 1);
                }
                Lex::Block(_) => {}
                Lex::Code => match c {
                    '/' if next == Some('/') => break,
                    '/' if next == Some('*') => {
                        k += 1;
                        code.push(' ');
                        state = Lex::Block(1);
                    }
                    '"' => {
                        code.push('"');
                        first = i;
                        state = Lex::Str;
                    }
                    'r' if raw_string_hashes(&chars, k).is_some() => {
                        let hashes = raw_string_hashes(&chars, k).unwrap_or(0);
                        k += hashes + 1;
                        code.push('"');
                        first = i;
                        state = Lex::RawStr(hashes);
                    }
                    '\'' if chars.get(k + 2) == Some(&'\'') => {
                        k += 2;
                        code.push_str("''");
                    }
                    '\'' if next == Some('\\') => {
                        k += 2 + chars[k + 2..].iter().take_while(|c| **c != '\'').count();
                        code.push_str("''");
                    }
                    _ => code.push(c),
                },
            }
            if close {
                code.push('"');
                out.strings.push((first, std::mem::take(&mut literal)));
                state = Lex::Code;
            }
            k += 1;
        }
        if matches!(state, Lex::Str | Lex::RawStr(_)) {
            literal.push('\n');
        }
        out.code.push(code);
    }
    out
}

/// The index of the line after the item that starts on line `start`: the
/// line of its `;` outside a body and outside brackets (`[u8; 4]`), or of
/// the `}` that closes its body. Braces in string, raw string and char
/// literals and in comments do not count.
fn item_end(lines: &[&str], start: usize) -> usize {
    let (mut depth, mut nested) = (0, 0);
    for (i, line) in lex(&lines[start..]).code.iter().enumerate() {
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return start + i + 1;
                    }
                }
                '[' | '(' => nested += 1,
                ']' | ')' => nested -= 1,
                ';' if depth == 0 && nested == 0 => return start + i + 1,
                _ => {}
            }
        }
    }
    lines.len()
}

/// The count of `#` of the raw string literal that starts at `chars[k]`
/// (`r"`, `r#"`, ...), or `None` when no raw string starts there.
fn raw_string_hashes(chars: &[char], k: usize) -> Option<usize> {
    if k > 0 && (chars[k - 1].is_alphanumeric() || chars[k - 1] == '_') && chars[k - 1] != 'b' {
        return None;
    }
    let hashes = chars[k + 1..].iter().take_while(|c| **c == '#').count();
    (chars.get(k + 1 + hashes) == Some(&'"')).then_some(hashes)
}

/// The text inside the parenthesis that opens at byte `open` of `text`.
fn balanced(text: &str, open: usize) -> Option<&str> {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The form of a `cfg`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum CfgForm {
    /// `#[cfg(..)]`: it gates the item under it.
    Gate,
    /// `#[cfg_attr(..)]`: it gates only an attribute.
    Attr,
    /// `cfg!(..)`: an expression.
    Macro,
}

/// A `cfg` in a text: its byte position, its form and its predicate.
struct Cfg {
    at: usize,
    form: CfgForm,
    pred: String,
}

/// Each `cfg(..)`, `cfg_attr(..)` and `cfg!(..)` of `text`. The predicate
/// of `cfg_attr` is its first argument.
fn cfgs(text: &str) -> Vec<Cfg> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("cfg") {
        if text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let rest = &text[at + 3..];
        let (open, form) = if rest.starts_with("_attr(") {
            (at + 8, CfgForm::Attr)
        } else if rest.starts_with("!(") {
            (at + 4, CfgForm::Macro)
        } else if rest.starts_with('(') {
            (at + 3, CfgForm::Gate)
        } else {
            continue;
        };
        let Some(body) = balanced(text, open) else {
            continue;
        };
        let pred = match form {
            CfgForm::Attr => arguments(body).first().copied().unwrap_or(""),
            _ => body,
        };
        out.push(Cfg {
            at,
            form,
            pred: pred.to_string(),
        });
    }
    out
}

/// The arguments of `body`, split at each comma outside parentheses.
fn arguments(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut from = 0;
    for (i, c) in body.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(body[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(body[from..].trim());
    out.retain(|a| !a.is_empty());
    out
}

/// The identifiers of a predicate, without the text of its string literals.
fn predicate_words(pred: &str) -> Vec<&str> {
    pred.split('"')
        .step_by(2)
        .flat_map(|part| part.split(|c: char| !(c.is_alphanumeric() || c == '_')))
        .filter(|w| !w.is_empty())
        .collect()
}

fn names_os(pred: &str) -> bool {
    predicate_words(pred).iter().any(|w| OS_WORDS.contains(w))
}

/// Whether `pred` is false in each build without `test`.
fn test_only(pred: &str) -> bool {
    without_test(pred) == Some(false)
}

/// The value of `pred` in a build without `test`: `test` is false, and each
/// other option is unknown (`None`).
fn without_test(pred: &str) -> Option<bool> {
    let pred = pred.trim();
    let call = |name: &str| {
        pred.strip_prefix(name)
            .map(str::trim_start)
            .and_then(|r| r.strip_prefix('('))
            .and_then(|r| r.strip_suffix(')'))
            .map(arguments)
    };
    if let Some(args) = call("any") {
        let values: Vec<Option<bool>> = args.iter().map(|a| without_test(a)).collect();
        if values.contains(&Some(true)) {
            Some(true)
        } else if values.iter().all(|v| *v == Some(false)) {
            Some(false)
        } else {
            None
        }
    } else if let Some(args) = call("all") {
        let values: Vec<Option<bool>> = args.iter().map(|a| without_test(a)).collect();
        if values.contains(&Some(false)) {
            Some(false)
        } else if values.iter().all(|v| *v == Some(true)) {
            Some(true)
        } else {
            None
        }
    } else if let Some(args) = call("not") {
        without_test(args.first().copied().unwrap_or("")).map(|v| !v)
    } else if pred == "test" {
        Some(false)
    } else {
        None
    }
}

/// The OS `cfg` sites of a file text, and its test-only child modules.
struct Scan {
    /// Each site as its first line number and its text.
    sites: Vec<(usize, String)>,
    test_mods: Vec<String>,
}

/// The OS `cfg` sites of `text`: `cfg(..)`, `cfg_attr(..)` and `cfg!(..)`
/// in production code whose predicate names an OS word and is not
/// test-only.
fn os_cfg_sites(text: &str) -> Scan {
    let production = production_lines(text);
    let lines = &production.lines;
    let mut joined = String::new();
    let mut starts = Vec::new();
    for (_, line) in lines {
        starts.push(joined.len());
        joined.push_str(line);
        joined.push('\n');
    }
    let mut sites: Vec<(usize, String)> = Vec::new();
    for cfg in cfgs(&joined) {
        if !names_os(&cfg.pred) || test_only(&cfg.pred) {
            continue;
        }
        let k = starts.partition_point(|s| *s <= cfg.at) - 1;
        if sites.last().is_none_or(|(n, _)| *n != lines[k].0) {
            sites.push((lines[k].0, lines[k].1.trim().to_string()));
        }
    }
    Scan {
        sites,
        test_mods: production.test_mods,
    }
}

#[test]
fn pl2_scan_finds_each_os_cfg_form_and_skips_test_code() {
    let text = "\
// #[cfg(unix)] in a comment
#[cfg(unix)]
fn a() {}
#[cfg(target_os = \"linux\")]
fn b() {}
#[cfg(any(windows, target_os = \"macos\"))]
fn c() {}
#[cfg_attr(target_family = \"unix\", allow(dead_code))]
fn d() {}
fn e() -> bool { cfg!(windows) }
#[cfg(not(any(
    target_os = \"macos\",
    target_os = \"linux\"
)))]
fn f() {}
#[cfg(feature = \"x\")]
fn g() {}
#[cfg(all(test, unix))]
mod test_support;
#[cfg(all(test, unix))]
fn h() {}
#[cfg(test)]
fn helper() -> &'static str {
    let _ = cfg!(unix);
    \"}\"
}
#[cfg(any(test, unix))]
fn j() {}
#[cfg(all(not(test), windows))]
fn k() {}
#[cfg(unix)]
#[cfg(test)]
mod tests {
    #[cfg(windows)]
    fn i() { let _ = '{'; }
}
#[cfg(windows)]
fn l() {}
";
    let Scan { sites, test_mods } = os_cfg_sites(text);
    let lines: Vec<usize> = sites.iter().map(|(n, _)| *n).collect();
    assert_eq!(lines, vec![2, 4, 6, 8, 10, 11, 27, 29, 37], "{sites:?}");
    assert_eq!(sites[5].1, "#[cfg(not(any(");
    assert_eq!(test_mods, vec!["test_support".to_string()]);

    let gated =
        "fn a() {}\n#[cfg(all(test, unix))]\nmod tests {\n    #[cfg(unix)]\n    fn b() {}\n}\nfn c() {}\n";
    assert_eq!(os_cfg_sites(gated).sites, Vec::new());
    assert_eq!(
        production_lines(gated).lines,
        vec![(1, "fn a() {}".to_string()), (7, "fn c() {}".to_string())]
    );
}

/// A predicate is test-only when it cannot hold in a build without `test`.
#[test]
fn pl2_test_only_predicates_are_the_ones_false_without_test() {
    for pred in [
        "test",
        "all(test, unix)",
        "all(unix, any(test, all(test, windows)))",
    ] {
        assert!(test_only(pred), "{pred}");
    }
    for pred in [
        "unix",
        "any(test, unix)",
        "not(test)",
        "all(not(test), unix)",
        "not(all(test, unix))",
    ] {
        assert!(!test_only(pred), "{pred}");
    }
}

/// The production files of the workspace outside the platform layer: each
/// member crate of the root `Cargo.toml`, its `src/` and its `build.rs`.
/// `crates/horch-platform` is the layer; `crates/horch-e2e` is test code.
fn production_files() -> (PathBuf, Vec<PathBuf>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let start = manifest.find("members = [").unwrap();
    let end = start + manifest[start..].find(']').unwrap();
    let mut files = Vec::new();
    for member in manifest[start..end].split('"').skip(1).step_by(2) {
        // ui-prop-tester: a standalone tool that no horch crate links.
        if member == "crates/horch-platform"
            || member == "crates/horch-e2e"
            || member == "crates/ui-prop-tester"
        {
            continue;
        }
        let dir = root.join(member);
        rust_files(&dir.join("src"), &mut files);
        if dir.join("build.rs").is_file() {
            files.push(dir.join("build.rs"));
        }
    }
    (root.join("crates"), files)
}

/// The path of each file of a child module that a `cfg` naming `test`
/// declares in `parent`, as a file or a folder prefix.
fn test_module_paths(parent: &Path, names: &[String]) -> Vec<PathBuf> {
    let stem = parent.file_stem().unwrap();
    let dir = if ["lib", "main", "mod"].iter().any(|s| stem == *s) {
        parent.parent().unwrap().to_path_buf()
    } else {
        parent.with_extension("")
    };
    names
        .iter()
        .flat_map(|n| [dir.join(format!("{n}.rs")), dir.join(n)])
        .collect()
}

/// Each OS `cfg` site of the workspace production code outside the platform
/// layer, as `<crate>/<path>:<line>: <text>`.
fn workspace_os_cfg_sites() -> Vec<String> {
    let (root, files) = production_files();
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let scan = os_cfg_sites(&std::fs::read_to_string(file).unwrap());
        test_paths.extend(test_module_paths(file, &scan.test_mods));
        scanned.push((file, scan.sites));
    }
    let mut found = Vec::new();
    for (file, sites) in scanned {
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
        {
            continue;
        }
        let rel = relative(file, &root);
        if OS_CFG_EXEMPT.iter().any(|(path, _)| *path == rel) {
            continue;
        }
        for (line, text) in sites {
            found.push(format!("{rel}:{line}: {}", text.trim()));
        }
    }
    found
}

/// PL-2: no OS `cfg` in production code outside the platform layer, except
/// the sites of `OS_CFG_EXEMPT`.
#[test]
fn fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform() {
    let found = workspace_os_cfg_sites();
    let exempt: Vec<String> = OS_CFG_EXEMPT
        .iter()
        .map(|(path, why)| format!("{path}: {why}"))
        .collect();
    assert!(
        found.is_empty(),
        "{} OS cfg sites outside crates/horch-platform:\n{}\nexempt:\n{}",
        found.len(),
        found.join("\n"),
        exempt.join("\n")
    );
}

/// Each exempt file still has an OS `cfg`, so the exemption list stays true.
#[test]
fn pl2_exempt_build_script_still_needs_its_exemption() {
    let (root, _) = production_files();
    for (path, why) in OS_CFG_EXEMPT {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert!(
            !os_cfg_sites(&text).sites.is_empty(),
            "{path} has no OS cfg any more; remove its exemption ({why})"
        );
    }
}

// ─── fleetd module rules (docs/tickets/fleetd-p1/MAP.md, Q13) ───────────────

/// The contexts under `fleetd/`, each with the ledger tables it owns.
const FLEETD_TABLE_OWNERS: &[(&str, &[&str])] = &[
    (
        "fleet_runtime",
        &[
            "fleetd",
            "jobs",
            "pass_cursors",
            "events",
            "requests",
            "polite_stops",
        ],
    ),
    (
        "task_execution",
        &[
            "tasks",
            "task_deps",
            "workers",
            "attempts",
            "leases",
            "worker_waits",
            "ingested",
        ],
    ),
    ("worker_profiles", &[]),
    (
        "ideas",
        &[
            "needs_you",
            "questions",
            "ideas",
            "requirements",
            "task_covers",
        ],
    ),
    ("worktree_custody", &["worktrees", "worktree_modes"]),
    (
        "integration",
        &[
            "features",
            "branches",
            "bundles",
            "bundle_members",
            "gate_runs",
            "remote_tips",
            "trial_merges",
        ],
    ),
];

/// The module files of Phase 5 (docs/tickets/fleetd-p5/MAP.md, Q16), under
/// `fleetd/`. Each exists and is a `pub mod` of the `mod.rs` of its folder.
const FLEETD_PHASE5_FILES: &[&str] = &[
    "fleet_runtime/domain/requests.rs",
    "fleet_runtime/domain/authorize.rs",
    "fleet_runtime/domain/operator.rs",
    "fleet_runtime/application/requests.rs",
    "fleet_runtime/application/handlers_run.rs",
    "fleet_runtime/application/handlers_task.rs",
    "fleet_runtime/application/handlers_worker.rs",
    "fleet_runtime/application/handlers_custody.rs",
    "fleet_runtime/adapters/requests_store.rs",
    "fleet_runtime/adapters/channel_server.rs",
    "fleet_runtime/adapters/request_spool.rs",
    "fleet_runtime/adapters/request_wire.rs",
    "fleet_runtime/adapters/protected_files.rs",
    "fleet_runtime/adapters/token_file.rs",
    "fleet_runtime/adapters/peer_process.rs",
    "fleet_runtime/adapters/channel_client.rs",
    "fleet_runtime/adapters/job_result_file.rs",
    "fleet_runtime/adapters/polite_stops_store.rs",
    "task_execution/domain/outbox.rs",
    "task_execution/application/outbox_ingest.rs",
    "task_execution/adapters/outbox_reader.rs",
    "task_execution/adapters/outbox_writer.rs",
    "task_execution/adapters/attempt_files.rs",
    "worker_profiles/domain/operator_boundary.rs",
    "worker_profiles/adapters/sandbox_rules.rs",
];

/// The module files of Phase 6 (docs/tickets/fleetd-p6/MAP.md, Q16), under
/// `fleetd/`. Each exists and is a `pub mod` of the `mod.rs` of its folder.
const FLEETD_PHASE6_FILES: &[&str] = &[
    "ideas/domain/idea.rs",
    "ideas/domain/idea_folder.rs",
    "ideas/domain/approval.rs",
    "ideas/domain/plan.rs",
    "ideas/domain/plan_validator.rs",
    "ideas/domain/plan_rules_dag.rs",
    "ideas/domain/plan_rules_coverage.rs",
    "ideas/domain/plan_rules_profiles.rs",
    "ideas/domain/plan_rules_gates.rs",
    "ideas/domain/plan_rules_judges.rs",
    "ideas/domain/plan_rules_model_guide.rs",
    "ideas/domain/answer.rs",
    "ideas/application/intake.rs",
    "ideas/application/direct.rs",
    "ideas/application/approval.rs",
    "ideas/application/amend.rs",
    "ideas/application/park_cancel.rs",
    "ideas/application/dispatcher.rs",
    "ideas/application/sub_orchestrators.rs",
    "ideas/application/waiting.rs",
    "ideas/application/plan_commit.rs",
    "ideas/application/replan.rs",
    "ideas/application/rollup.rs",
    "ideas/application/question_flow.rs",
    "ideas/application/answers.rs",
    "ideas/application/notes_ingest.rs",
    "ideas/application/queries.rs",
    "ideas/application/release.rs",
    "ideas/adapters/ideas_store.rs",
    "ideas/adapters/idea_folder_files.rs",
    "ideas/adapters/plan_yaml.rs",
    "ideas/adapters/idea_notes_reader.rs",
    "ideas/adapters/needs_you_folder.rs",
    "ideas/adapters/notify.rs",
    "integration/domain/trial_merge.rs",
    "integration/domain/stack.rs",
    "integration/application/new_features.rs",
    "integration/application/trial_merges.rs",
    "integration/application/stacks.rs",
    "integration/application/stack_edges.rs",
    "integration/application/merged_with_ideas.rs",
    "integration/application/judge_branch.rs",
    "integration/adapters/exec_trial_merge.rs",
    "integration/adapters/trial_merges_store.rs",
    "task_execution/domain/judge_pair.rs",
    "task_execution/application/judge_pair.rs",
    "task_execution/application/planned_tasks.rs",
    "fleet_runtime/application/handlers_idea.rs",
];

/// The 1 door: it writes the state of each entity, so it names the tables
/// of every context.
/// The module files of Phase 7 (docs/tickets/fleetd-p7/MAP.md, Q17), under
/// `fleetd/`. Each exists and is a `pub mod` of the `mod.rs` of its folder.
const FLEETD_PHASE7_FILES: &[&str] = &[
    "worker_profiles/domain/conformance.rs",
    "worker_profiles/domain/health.rs",
    "worker_profiles/domain/docs_jobs.rs",
    "worker_profiles/application/conformance.rs",
    "worker_profiles/application/health.rs",
    "worker_profiles/application/docs_jobs.rs",
    "worker_profiles/application/docs_sync.rs",
    "worker_profiles/application/drift_idea.rs",
    "worker_profiles/adapters/conformance_runner.rs",
    "worker_profiles/adapters/docs_dir.rs",
    "task_execution/domain/context_limit.rs",
];

const FLEETD_DOOR: &str = "fleetd/fleet_runtime/adapters/sqlite.rs";

/// Primitive types, which a path such as `u64::MAX` can start with.
const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64",
];

/// The code of `path` outside tests, after `lex`, with the line number of
/// each lexed line.
fn lexed_file(path: &Path) -> (Vec<usize>, Lexed) {
    let lines = code_lines(path);
    let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
    (lines.iter().map(|(n, _)| *n).collect(), lex(&texts))
}

/// A token of lexed code for `code_paths`.
#[derive(Clone, PartialEq)]
enum Tok {
    Ident(String),
    Sep,
    Punct(char),
}

/// The tokens of `code`, each with the index of its line.
fn tokens(code: &[String]) -> Vec<(usize, Tok)> {
    let mut out = Vec::new();
    for (i, line) in code.iter().enumerate() {
        let c: Vec<char> = line.chars().collect();
        let mut k = 0;
        while k < c.len() {
            let start = k;
            if c[k].is_alphanumeric() || c[k] == '_' {
                while k < c.len() && (c[k].is_alphanumeric() || c[k] == '_') {
                    k += 1;
                }
                if !c[start].is_ascii_digit() {
                    out.push((i, Tok::Ident(c[start..k].iter().collect())));
                }
                continue;
            }
            if c[k] == ':' && c.get(k + 1) == Some(&':') {
                out.push((i, Tok::Sep));
                k += 2;
                continue;
            }
            if !c[k].is_whitespace() {
                out.push((i, Tok::Punct(c[k])));
            }
            k += 1;
        }
    }
    out
}

/// Each path in `code`, as its segments with the index of its first line.
/// A path has a `::`, or it follows `use` or `extern crate`. A leading `::`
/// gives an empty first segment, and a path in a `use` group (`a::{b, c}`)
/// starts with the prefix of the group.
fn code_paths(code: &[String]) -> Vec<(usize, Vec<String>)> {
    let toks = tokens(code);
    let tok = |i: usize| toks.get(i).map(|(_, t)| t);
    let ident = |i: usize| match tok(i) {
        Some(Tok::Ident(name)) => Some(name.clone()),
        _ => None,
    };
    let mut out = Vec::new();
    let mut groups: Vec<(usize, Vec<String>)> = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    while i < toks.len() {
        let prev = i.checked_sub(1).and_then(tok);
        match &toks[i].1 {
            Tok::Punct('{') => depth += 1,
            Tok::Punct('}') => {
                if groups.last().is_some_and(|(d, _)| *d == depth) {
                    groups.pop();
                }
                depth -= 1;
            }
            _ => {}
        }
        let leading = toks[i].1 == Tok::Sep
            && ident(i + 1).is_some()
            && !matches!(prev, Some(Tok::Ident(_) | Tok::Punct('>' | ')' | ']')));
        let starts = ident(i).is_some() && prev != Some(&Tok::Sep);
        if !leading && !starts {
            i += 1;
            continue;
        }
        let line = toks[i].0;
        let declares = matches!(prev, Some(Tok::Ident(w)) if w == "use")
            || (matches!(prev, Some(Tok::Ident(w)) if w == "crate")
                && matches!(i.checked_sub(2).and_then(tok), Some(Tok::Ident(w)) if w == "extern"));
        let mut segs = match groups.last() {
            Some((d, prefix)) if *d == depth => prefix.clone(),
            _ => Vec::new(),
        };
        let in_group = !segs.is_empty();
        let mut opens_group = false;
        if leading {
            segs.push(String::new());
            i += 1;
        }
        while let Some(name) = ident(i) {
            segs.push(name);
            i += 1;
            if tok(i) != Some(&Tok::Sep) {
                break;
            }
            match tok(i + 1) {
                Some(Tok::Ident(_)) => i += 1,
                Some(Tok::Punct('{')) => {
                    groups.push((depth + 1, segs.clone()));
                    opens_group = true;
                    i += 1;
                    break;
                }
                Some(Tok::Punct('*')) => {
                    segs.push("*".to_string());
                    break;
                }
                _ => break,
            }
        }
        if !opens_group && (segs.len() > 1 || in_group || declares || leading) {
            out.push((line, segs));
        }
    }
    out
}

/// The paths of `code` that leave the domain module `module` (its path
/// below `crate`, as `["fleetd", ctx, "domain", ...]`), each with the index
/// of its line. `siblings` are the modules of that `domain/` folder.
fn outward_paths(code: &[String], module: &[&str], siblings: &[String]) -> Vec<(usize, String)> {
    let inside =
        |abs: &[String]| abs.len() >= 3 && abs.iter().zip(&module[..3]).all(|(a, d)| a == d);
    code_paths(code)
        .into_iter()
        .filter(|(_, segs)| {
            let first = segs[0].as_str();
            let ok = match first {
                "" => segs.get(1).is_some_and(|s| s == "std"),
                "std" | "self" | "Self" => true,
                "crate" => inside(&segs[1..]),
                "super" => {
                    let ups = segs.iter().take_while(|s| *s == "super").count();
                    let mut abs: Vec<String> = module[..module.len().saturating_sub(ups)]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    abs.extend(segs[ups..].iter().cloned());
                    module.len() >= ups && inside(&abs)
                }
                _ if first.starts_with(char::is_uppercase) => true,
                _ => PRIMITIVES.contains(&first) || siblings.iter().any(|s| s == first),
            };
            !ok
        })
        .map(|(line, segs)| (line, segs.join("::")))
        .collect()
}

/// The SQL tables that the string `literal` names and that are not in
/// `owned`, in lower case. A string is SQL when its first word, up to a
/// space, is a statement keyword (so `update-ref` is not), or it holds an
/// upper-case `BEFORE_TABLE` keyword.
/// Keywords and tables match in any case, as SQLite reads them.
fn foreign_tables(literal: &str, owned: &[&str]) -> Vec<String> {
    const STATEMENTS: &[&str] = &[
        "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "WITH", "REPLACE",
    ];
    const BEFORE_TABLE: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "REFERENCES"];
    let words: Vec<&str> = literal
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    let sql = literal
        .split_whitespace()
        .next()
        .is_some_and(|w| STATEMENTS.contains(&w.to_ascii_uppercase().as_str()))
        || words.iter().any(|w| BEFORE_TABLE.contains(w));
    if !sql {
        return Vec::new();
    }
    let upper: Vec<String> = words.iter().map(|w| w.to_ascii_uppercase()).collect();
    let mut found = Vec::new();
    for (n, w) in upper.iter().enumerate() {
        if !BEFORE_TABLE.contains(&w.as_str()) {
            continue;
        }
        let rest: Vec<&str> = upper[n + 1..].iter().map(String::as_str).collect();
        let skip = match rest.as_slice() {
            ["IF", "NOT", "EXISTS", ..] => 3,
            ["IF", "EXISTS", ..] => 2,
            _ => 0,
        };
        let Some(table) = words.get(n + 1 + skip).map(|t| t.to_ascii_lowercase()) else {
            continue;
        };
        if !owned.contains(&table.as_str()) && !found.contains(&table) {
            found.push(table);
        }
    }
    found
}

/// The `fleetd/` folder, after it asserts that each context of
/// `FLEETD_TABLE_OWNERS` has a `domain/` folder, that no other context
/// folder exists, and that each file of `FLEETD_PHASE5_FILES`,
/// `FLEETD_PHASE6_FILES` and `FLEETD_PHASE7_FILES` is a module.
fn fleetd_root() -> PathBuf {
    let fleetd = crate_src("horch-core").join("fleetd");
    for (ctx, _) in FLEETD_TABLE_OWNERS {
        let domain = fleetd.join(ctx).join("domain");
        assert!(domain.is_dir(), "{} is missing", domain.display());
    }
    for file in FLEETD_PHASE5_FILES
        .iter()
        .chain(FLEETD_PHASE6_FILES)
        .chain(FLEETD_PHASE7_FILES)
    {
        let path = fleetd.join(file);
        assert!(path.is_file(), "fleetd/{file} is missing");
        let name = path.file_stem().unwrap().to_string_lossy();
        let mod_rs = std::fs::read_to_string(path.with_file_name("mod.rs")).unwrap();
        assert!(
            mod_rs
                .lines()
                .any(|l| l.trim() == format!("pub mod {name};")),
            "fleetd/{file} has no `pub mod {name};` line in its mod.rs"
        );
    }
    for entry in std::fs::read_dir(&fleetd).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            !path.is_dir() || FLEETD_TABLE_OWNERS.iter().any(|(c, _)| *c == name),
            "fleetd/{name} is a context with no row in FLEETD_TABLE_OWNERS"
        );
    }
    fleetd
}

/// A domain module names only `std` and its own context's `domain` module:
/// no other crate, and no other module of `crate`.
#[test]
fn fitness_domain_imports_nothing_outward() {
    let fleetd = fleetd_root();
    let root = crate_src("horch-core");
    let mut found = Vec::new();
    for (ctx, _) in FLEETD_TABLE_OWNERS {
        let dir = fleetd.join(ctx).join("domain");
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        let siblings: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&dir).unwrap().iter().next().unwrap())
            .map(|n| n.to_string_lossy().trim_end_matches(".rs").to_string())
            .filter(|n| n != "mod")
            .collect();
        for path in &files {
            let below = relative(path, &dir);
            let mut module = vec!["fleetd", ctx, "domain"];
            module.extend(
                below
                    .trim_end_matches(".rs")
                    .split('/')
                    .filter(|s| *s != "mod"),
            );
            let (numbers, lexed) = lexed_file(path);
            for (line, p) in outward_paths(&lexed.code, &module, &siblings) {
                found.push(format!("{}:{}: {p}", relative(path, &root), numbers[line]));
            }
        }
    }
    assert!(
        found.is_empty(),
        "domain code names a path outside its domain module:\n{}",
        found.join("\n")
    );
}

/// The domain rule's own cases.
#[test]
fn fitness_domain_rule_finds_outward_paths() {
    let module = ["fleetd", "ideas", "domain", "needs_you"];
    let siblings = vec!["needs_you".to_string(), "other".to_string()];
    let outward = |src: &str, module: &[&str]| {
        let lines: Vec<&str> = src.lines().collect();
        outward_paths(&lex(&lines).code, module, &siblings)
    };
    for hit in [
        "use serde::Serialize;",
        "use serde;",
        "extern crate serde;",
        "use rusqlite::Connection;",
        "let x = horch_platform::probe();",
        "use crate::taskledger::Store;",
        "use crate::fleetd::task_execution::domain::Task;",
        "use crate::fleetd::ideas::application::X;",
        "use crate::{taskledger::Store};",
        "use std::{fmt, io};\nuse crate::{\n    fleetd::ideas::domain::other::Y,\n    taskledger,\n};",
        "use super::super::adapters::sqlite;",
        "use ::anyhow::Result;",
        "#[derive(serde::Deserialize)]",
        "let s = '\"'; // \"\nlet c = rusqlite::Connection::open(p);",
        "/* \" */ use rusqlite::Connection;",
    ] {
        assert!(!outward(hit, &module).is_empty(), "missed: {hit}");
    }
    for miss in [
        "use std::collections::BTreeMap;",
        "use std::{collections::BTreeMap, fmt};",
        "use crate::fleetd::ideas::domain::other::Y;",
        "use crate::{fleetd::ideas::domain::other::Y};",
        "use super::other::Y;",
        "use self::inner::Z;",
        "let n = u64::MAX; let v = Vec::<u8>::new();",
        "match s { State::Open => {} }",
        "let s = \"rusqlite::Connection\";",
        "fn f<T: std::fmt::Debug>(t: T) -> Self::Out {}",
        "let x = 1; // rusqlite::Connection",
        "/* rusqlite::Connection\n   serde::Serialize */ let x = 1;",
    ] {
        assert_eq!(outward(miss, &module), Vec::new(), "false hit: {miss}");
    }
    let lines = outward("let a = 1;\nuse serde::Serialize;", &module);
    assert_eq!(lines, vec![(1, "serde::Serialize".to_string())]);
    let mod_rs = ["fleetd", "ideas", "domain"];
    assert!(!outward("use super::application::X;", &mod_rs).is_empty());
}

/// Each SQL table that a file under `fleetd/<context>/` names is a table of
/// that context. The 1 door is exempt.
#[test]
fn fitness_no_context_touches_foreign_tables() {
    let fleetd = fleetd_root();
    let root = crate_src("horch-core");
    let mut found = Vec::new();
    for (ctx, owned) in FLEETD_TABLE_OWNERS {
        let mut files = Vec::new();
        rust_files(&fleetd.join(ctx), &mut files);
        for path in &files {
            let rel = relative(path, &root);
            if rel == FLEETD_DOOR {
                continue;
            }
            let (numbers, lexed) = lexed_file(path);
            for (line, literal) in &lexed.strings {
                for table in foreign_tables(literal, owned) {
                    found.push(format!(
                        "{rel}:{}: table `{table}` is not a table of {ctx}",
                        numbers[*line]
                    ));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a context names a table of another context:\n{}",
        found.join("\n")
    );
}

/// The table rule's own cases.
#[test]
fn fitness_table_rule_finds_foreign_tables() {
    let owned = ["needs_you"];
    let tables = |src: &str| -> Vec<String> {
        let lines: Vec<&str> = src.lines().collect();
        lex(&lines)
            .strings
            .iter()
            .flat_map(|(_, s)| foreign_tables(s, &owned))
            .collect()
    };
    for (hit, table) in [
        (
            "conn.execute(\"UPDATE tasks SET status = ?1\", [])",
            "tasks",
        ),
        (
            "q(\"SELECT id FROM needs_you JOIN workers ON 1\")",
            "workers",
        ),
        ("q(r#\"INSERT INTO\n  events (at) VALUES (?1)\"#)", "events"),
        ("q(\"CREATE TABLE IF NOT EXISTS jobs (id)\")", "jobs"),
        ("q(\"DELETE FROM attempts\")", "attempts"),
        ("q(\"x INTEGER REFERENCES runs(id)\")", "runs"),
        ("q(\"select * from tasks\")", "tasks"),
        ("q(\"SELECT * FROM Tasks\")", "tasks"),
        ("// \"\nq(\"DELETE FROM attempts\")", "attempts"),
    ] {
        assert_eq!(tables(hit), vec![table], "{hit}");
    }
    for miss in [
        "q(\"SELECT id FROM needs_you WHERE id = ?1\")",
        "q(\"INSERT INTO NEEDS_YOU (id) VALUES (?1)\")",
        "let s = \"a message from the user\";",
        "git.run(dir, &[\"update-ref\", \"-m\", m, r, c, \"\"])",
        "let from_tasks = tasks.len();",
        "// q(\"DELETE FROM tasks\")",
        "/* q(\"DELETE FROM tasks\") */",
    ] {
        assert_eq!(tables(miss), Vec::<String>::new(), "{miss}");
    }
}

// ─── OR-2: the state writes go through the 1 door ───────────────────────────

/// Whether the SQL string `literal` writes the state column of a table in
/// [`STATE_COLUMNS`]: an `UPDATE` of that table whose `SET` clause names
/// that column. Keywords and names match in any case, as SQLite reads them.
fn writes_state_column(literal: &str) -> bool {
    let words = sql_words(literal);
    (0..words.len()).any(|at| {
        set_clause_of_state_table(&words, at)
            .is_some_and(|(set, column)| assigns(&words[set..], column))
    })
}

/// The words of the SQL string `literal`: names and keywords in lower case,
/// and each other sign as 1 word.
fn sql_words(literal: &str) -> Vec<String> {
    tokens(&[literal.to_string()])
        .into_iter()
        .map(|(_, t)| match t {
            Tok::Ident(name) => name.to_ascii_lowercase(),
            Tok::Sep => "::".to_string(),
            Tok::Punct(c) => c.to_string(),
        })
        .collect()
}

/// The tables whose state column only the 1 door writes, with that
/// column. `worktrees.status` keeps the writes of today
/// (`docs/tickets/fleetd-p3/MAP.md`, Q3): only its `state` is here. The
/// task branch, the bundle and the feature are entities of Phase 4
/// (`docs/tickets/fleetd-p4/06-integration-entities-in-the-door.md`), and
/// the idea is an entity of Phase 6 (`docs/tickets/fleetd-p6/04-door.md`).
const STATE_COLUMNS: &[(&str, &str)] = &[
    ("tasks", "status"),
    ("workers", "status"),
    ("worktrees", "state"),
    ("branches", "landing_state"),
    ("bundles", "state"),
    ("features", "state"),
    ("ideas", "state"),
];

/// When the words from `at` start `UPDATE [OR <conflict>] [main.]<table>`
/// for a table of [`STATE_COLUMNS`], with an optional alias (`[AS] t`), and
/// then `SET`: the index of the first word after `SET`, and the state
/// column of the table.
fn set_clause_of_state_table(words: &[String], at: usize) -> Option<(usize, &'static str)> {
    let word = |i: usize| words.get(i).map(String::as_str);
    if word(at) != Some("update") {
        return None;
    }
    let mut next = at + 1;
    if word(next) == Some("or") {
        next += 2;
    }
    if word(next) == Some("main") && word(next + 1) == Some(".") {
        next += 2;
    }
    let column = STATE_COLUMNS
        .iter()
        .find(|(table, _)| word(next) == Some(*table))
        .map(|(_, column)| *column)?;
    next += 1;
    // The alias: `AS t` or a bare `t`.
    if word(next) == Some("as") {
        next += 2;
    } else if word(next) != Some("set") {
        next += 1;
    }
    (word(next) == Some("set")).then_some((next + 1, column))
}

/// Whether the assignments of a `SET` clause, from its first word to its
/// end, assign `column` (also as `t.<column>`). The clause ends at a
/// `WHERE`, `FROM`, `RETURNING` or `;` outside parentheses.
fn assigns(clause: &[String], column: &str) -> bool {
    let mut depth = 0;
    let mut starts_assignment = true;
    for (i, word) in clause.iter().enumerate() {
        let (name, after_column) = if clause.get(i + 1).is_some_and(|w| w == ".") {
            (clause.get(i + 2), i + 3)
        } else {
            (Some(word), i + 1)
        };
        if depth == 0
            && starts_assignment
            && name.is_some_and(|c| c == column)
            && clause.get(after_column).is_some_and(|w| w == "=")
        {
            return true;
        }
        match word.as_str() {
            "(" => depth += 1,
            ")" => depth -= 1,
            "where" | "from" | "returning" | ";" if depth == 0 => return false,
            _ => {}
        }
        starts_assignment = depth == 0 && word == ",";
    }
    false
}

/// The direct state writes that are not a state change of a row in flight:
/// the path, a part of the SQL text, and why it is exempt.
const STATE_WRITES_EXEMPT: &[(&str, &str, &str)] = &[
    (
        "horch-core/src/taskledger/schema.rs",
        "UPDATE worktrees SET state = CASE",
        "the release backfill of Phase 3 gives each row its first state \
         (docs/tickets/fleetd-p3/MAP.md, Q19)",
    ),
    (
        "horch-core/src/taskledger/schema.rs",
        "UPDATE worktrees SET state = 'removed' WHERE status IN",
        "the worktree_paths step makes each row that today's triage archived or \
         trashed removed: its folder is gone (fleetd-p3 ticket 13, Q5)",
    ),
];

fn exempt(found: &str) -> bool {
    STATE_WRITES_EXEMPT
        .iter()
        .any(|(path, sql, _)| found.starts_with(&format!("{path}:")) && found.contains(sql))
}

/// Each direct write of a state column of [`STATE_COLUMNS`] in the
/// production code of `horch-core` and `horch`, as `<path>:<line>: <sql>`,
/// outside the 1 door.
fn direct_state_writes() -> Vec<String> {
    let door = format!("horch-core/src/{FLEETD_DOOR}:");
    let mut found = production_sql(writes_state_column);
    found.retain(|f| !f.starts_with(&door));
    found
}

/// Each SQL string of the production code of `horch-core` and `horch`
/// that `matches`, as `<path>:<line>: <sql>`.
fn production_sql(matches: fn(&str) -> bool) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for name in ["horch-core", "horch"] {
        rust_files(&crate_src(name), &mut files);
    }
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let prod = production_lines(&std::fs::read_to_string(file).unwrap());
        test_paths.extend(test_module_paths(file, &prod.test_mods));
        scanned.push((file, prod.lines));
    }
    let mut found = Vec::new();
    for (file, lines) in scanned {
        let rel = relative(file, &root);
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
        {
            continue;
        }
        let texts: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        for (i, literal) in lex(&texts).strings {
            if matches(&literal) {
                let sql = literal.split_whitespace().collect::<Vec<_>>().join(" ");
                found.push(format!("{rel}:{}: {sql}", lines[i].0));
            }
        }
    }
    found
}

/// OR-2: each production write of a state column of [`STATE_COLUMNS`]
/// goes through `transition()`, so each state change has its event and
/// cause.
#[test]
fn fitness_status_writes_go_through_transition() {
    let mut found = direct_state_writes();
    found.retain(|f| !exempt(f));
    assert!(
        found.is_empty(),
        "a state write does not go through transition():\n{}",
        found.join("\n")
    );
}

/// Each exemption of [`STATE_WRITES_EXEMPT`] still exempts a write.
#[test]
fn each_state_write_exemption_is_still_needed() {
    let found = direct_state_writes();
    for (path, sql, why) in STATE_WRITES_EXEMPT {
        assert!(
            found
                .iter()
                .any(|f| f.starts_with(&format!("{path}:")) && f.contains(sql)),
            "{path} has no `{sql}` any more; remove its exemption ({why})"
        );
    }
}

/// The state write matcher's own cases.
#[test]
fn status_write_matcher_finds_a_direct_update() {
    let writes = |src: &str| -> Vec<bool> {
        let lines: Vec<&str> = src.lines().collect();
        lex(&lines)
            .strings
            .iter()
            .map(|(_, s)| writes_state_column(s))
            .collect()
    };
    for hit in [
        "c.execute(\"UPDATE tasks SET status = ?2 WHERE task_id = ?1\", p)",
        "c.execute(\"UPDATE workers SET status = 'done'\", [])",
        "c.execute(\"update Workers set status=?2\", p)",
        "c.execute(&format!(\"UPDATE workers SET agent_pid = ?2,\n    status = {working} WHERE worker_id = ?1\"), p)",
        "c.execute(r#\"UPDATE tasks\n SET summary = ?2, status = ?3\"#, p)",
        "c.execute(\"UPDATE OR IGNORE tasks SET status = 'x'\", p)",
        "c.execute(\"UPDATE runs SET x = 1; UPDATE tasks SET status = 'x'\", p)",
        "c.execute(\"UPDATE tasks AS t SET status = 'done'\", p)",
        "c.execute(\"UPDATE workers w SET status = 'done'\", p)",
        "c.execute(\"UPDATE main.tasks SET t.status = 'done'\", p)",
        "c.execute(\"UPDATE worktrees SET head_sha = ?2, state = 'removed'\", p)",
        "c.execute(\"UPDATE worktrees w SET w.state = ?2 WHERE worktree_id = ?1\", p)",
    ] {
        assert_eq!(writes(hit), vec![true], "{hit}");
    }
    for miss in [
        "c.execute(\"UPDATE tasks SET summary = ?2 WHERE status = 'running'\", p)",
        "c.execute(\"UPDATE workers SET last_status = ?2\", p)",
        "c.execute(\"UPDATE worktrees SET status = ?2\", p)",
        "c.execute(\"UPDATE worktrees SET head_sha = ?2 WHERE state = 'active'\", p)",
        "c.execute(\"UPDATE tasks SET state = ?2\", p)",
        "c.execute(\"UPDATE task_steps SET status = ?2\", p)",
        "c.execute(\"UPDATE workers SET x = (SELECT status = 'a' FROM tasks)\", p)",
        "c.execute(\"SELECT status FROM tasks\", p)",
        "let s = \"the update of tasks set the status\";",
    ] {
        assert_eq!(writes(miss), vec![false], "{miss}");
    }
    let src = "fn a() {}\n// c.execute(\"UPDATE tasks SET status = 'x'\", p)\n";
    assert_eq!(writes(src), Vec::<bool>::new());
}

/// The production files that may move a worktree to `removed` through the
/// door, and why.
const REMOVED_MOVERS: &[(&str, &str)] = &[
    (
        "horch-core/src/fleetd/worktree_custody/application/remove.rs",
        "the worktree_remove job: a landed or rescued tree, behind the remove guard",
    ),
    (
        "horch-core/src/taskledger/triage/apply.rs",
        "settle_gone: the operator decision on a tree whose folder is gone, behind \
         settle_refusal (fleetd-p3 ticket 13, review F1)",
    ),
];

/// The arguments of `body`, split at each comma outside (), [] and {}.
fn call_arguments(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut from = 0;
    for (i, c) in body.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(body[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(body[from..].trim());
    out.retain(|a| !a.is_empty());
    out
}

/// Whether `text` holds a call of `transition(` whose target is
/// `"removed"`: the 5th argument of the door.
fn moves_to_removed(text: &str) -> bool {
    text.match_indices("transition(").any(|(at, _)| {
        let ident = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let open = at + "transition".len();
        !ident
            && balanced(text, open)
                .is_some_and(|body| call_arguments(body).get(4) == Some(&"\"removed\""))
    })
}

/// Each production file of `horch-core` and `horch` that moves a worktree
/// to `removed` through the door.
fn removed_movers() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for name in ["horch-core", "horch"] {
        rust_files(&crate_src(name), &mut files);
    }
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let prod = production_lines(&std::fs::read_to_string(file).unwrap());
        test_paths.extend(test_module_paths(file, &prod.test_mods));
        scanned.push((file, prod.lines));
    }
    let mut found = Vec::new();
    for (file, lines) in scanned {
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
        {
            continue;
        }
        let text: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        if moves_to_removed(&text.join("\n")) {
            found.push(relative(file, &root));
        }
    }
    found
}

/// RR-9 and FM-27: only the worktree_remove job and the triage settle of a
/// gone folder move a worktree to `removed`; no other caller uses the door
/// move `orphaned` to `removed`.
#[test]
fn fitness_only_custody_and_triage_move_a_worktree_to_removed() {
    let found = removed_movers();
    let extra: Vec<&String> = found
        .iter()
        .filter(|f| !REMOVED_MOVERS.iter().any(|(p, _)| p == f))
        .collect();
    assert!(
        extra.is_empty(),
        "a worktree moves to removed outside custody and triage: {extra:?}"
    );
    for (path, why) in REMOVED_MOVERS {
        assert!(
            found.iter().any(|f| f == path),
            "{path} no longer moves a worktree to removed; drop its entry ({why})"
        );
    }
}

/// The matcher of the rule above finds a door call to `removed` and no
/// other call.
#[test]
fn removed_matcher_finds_a_door_call_to_removed() {
    assert!(moves_to_removed(
        "transition(\n    tx,\n    None,\n    Entity::Worktree,\n    id,\n    \"removed\",\n    Cause { actor: a, text: t },\n    r,\n)"
    ));
    assert!(moves_to_removed(
        "transition(&tx, None, Entity::Worktree, &id, \"removed\", cause, &NeedsYouList)"
    ));
    assert!(!moves_to_removed(
        "transition(tx, None, Entity::Worktree, id, \"created\", cause, r)"
    ));
    assert!(!moves_to_removed(
        "my_transition(tx, None, Entity::Worktree, id, \"removed\", cause, r)"
    ));
}

/// The production folder whose files may run `git worktree add`, `remove`
/// and `prune`: the worktree custody adapters of fleetd.
const WORKTREE_CUSTODY: &str = "horch-core/src/fleetd/worktree_custody/adapters/";

/// The production files outside worktree custody that may run `git worktree
/// add`, `remove` or `prune`, and why.
const WORKTREE_CALL_EXEMPT: &[(&str, &str)] = &[(
    "horch-core/src/vcs/landing.rs",
    "the claims of `horch worktree new` and `land` make trees that belong to no \
     ledger run; the reconciler adopts such a tree as orphaned and never removes \
     it (docs/tickets/fleetd-p3/MAP.md, Q15)",
)];

/// Whether `text` holds the string literal `"worktree"` followed in the same
/// call by `"add"`, `"remove"` or `"prune"`: as the next item of a list
/// (`["worktree", "add"]`) or as the next `.arg(..)` or `.args(..)` of a
/// command.
fn runs_git_worktree_change(text: &str) -> bool {
    text.match_indices("\"worktree\"").any(|(at, lit)| {
        let mut rest = text[at + lit.len()..].trim_start();
        if let Some(r) = rest.strip_prefix(',') {
            rest = r;
        } else if let Some(r) = rest.strip_prefix(')') {
            let r = r.trim_start();
            match [".arg(", ".args([", ".args(&["]
                .iter()
                .find_map(|call| r.strip_prefix(call))
            {
                Some(r) => rest = r,
                None => return false,
            }
        } else {
            return false;
        }
        let rest = rest.trim_start();
        ["\"add\"", "\"remove\"", "\"prune\""]
            .iter()
            .any(|verb| rest.starts_with(verb))
    })
}

/// Each production file of the workspace, outside `horch-e2e` (test code),
/// that runs `git worktree add`, `remove` or `prune`, relative to `crates/`:
/// each member's `src/` and its `build.rs`.
fn git_worktree_changers() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let start = manifest.find("members = [").unwrap();
    let end = start + manifest[start..].find(']').unwrap();
    let mut files = Vec::new();
    for member in manifest[start..end].split('"').skip(1).step_by(2) {
        if member == "crates/horch-e2e" {
            continue;
        }
        let dir = root.join(member);
        rust_files(&dir.join("src"), &mut files);
        if dir.join("build.rs").is_file() {
            files.push(dir.join("build.rs"));
        }
    }
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let prod = production_lines(&std::fs::read_to_string(file).unwrap());
        test_paths.extend(test_module_paths(file, &prod.test_mods));
        scanned.push((file, prod.lines));
    }
    let mut found = Vec::new();
    for (file, lines) in scanned {
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
        {
            continue;
        }
        let text: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
        if runs_git_worktree_change(&text.join("\n")) {
            found.push(relative(file, &root.join("crates")));
        }
    }
    found
}

/// FR-11 and FM-26: only worktree custody runs `git worktree add`, `remove`
/// or `prune`. `git worktree list` is a read and is not in the rule (Q16).
#[test]
fn fitness_only_worktree_custody_runs_git_worktree_add_remove_or_prune() {
    let found = git_worktree_changers();
    let extra: Vec<&String> = found
        .iter()
        .filter(|f| {
            !f.starts_with(WORKTREE_CUSTODY) && !WORKTREE_CALL_EXEMPT.iter().any(|(p, _)| p == f)
        })
        .collect();
    assert!(
        extra.is_empty(),
        "git worktree add, remove or prune runs outside worktree custody: {extra:?}"
    );
    for (path, why) in WORKTREE_CALL_EXEMPT {
        assert!(
            found.iter().any(|f| f == path),
            "{path} no longer runs git worktree add, remove or prune; drop its entry ({why})"
        );
    }
}

/// The matcher of the rule above finds a worktree change and no read.
#[test]
fn git_worktree_matcher_finds_add_remove_and_prune() {
    for hit in [
        "git(repo, &[\"worktree\", \"add\", \"-b\", branch, path, base])",
        "self.git(repo, &[\"worktree\", \"prune\"])",
        "steps.push((\"worktree remove\", vec![\"worktree\", \"remove\", &path]));",
        "let mut args = vec![\n    \"worktree\",\n    \"remove\",\n];",
        "Command::new(\"git\").arg(\"worktree\").arg(\"add\").arg(path)",
        "Command::new(\"git\").arg(\"worktree\")\n    .args([\"remove\", \"--force\"])",
        "Command::new(\"git\").arg(\"worktree\").args(&[\"add\", path])",
    ] {
        assert!(runs_git_worktree_change(hit), "{hit}");
    }
    for miss in [
        "git(repo, &[\"worktree\", \"list\", \"--porcelain\"])",
        "git(repo, &[\"worktree\", \"lock\", path])",
        "git(repo, &[\"worktree\", \"repair\", \"--\", tree])",
        "(\"worktree\", false),",
        "Entity::Worktree => \"worktree\",",
        "let s = \"git worktree add\";",
        "let s = \"say \\\"worktree\\\", \\\"add\\\"\";",
        "git(repo, &[\"branch\", \"-D\", \"add\"])",
    ] {
        assert!(!runs_git_worktree_change(miss), "{miss}");
    }
}

// ─── The state of a new worktree row is written, not defaulted ──────────────

/// Whether the SQL string `literal` inserts into `worktrees` without the
/// `state` column in its column list. A row that takes the default state
/// `orphaned` would list a live tree as orphaned (fleetd-p3, Q19).
fn inserts_worktree_without_state(literal: &str) -> bool {
    let words = sql_words(literal);
    let word = |i: usize| words.get(i).map(String::as_str);
    (0..words.len()).any(|at| {
        if !matches!(word(at), Some("insert" | "replace")) {
            return false;
        }
        let mut next = at + 1;
        if word(next) == Some("or") {
            next += 2;
        }
        if word(next) != Some("into") {
            return false;
        }
        next += 1;
        if word(next) == Some("main") && word(next + 1) == Some(".") {
            next += 2;
        }
        if word(next) != Some("worktrees") {
            return false;
        }
        if word(next + 1) != Some("(") {
            return true;
        }
        !words[next + 2..]
            .iter()
            .take_while(|w| *w != ")")
            .any(|w| w == "state")
    })
}

/// Each production `INSERT INTO worktrees` names the `state` column, so no
/// new row relies on the default `orphaned` of the Phase 3 migration.
#[test]
fn fitness_each_worktree_insert_writes_its_state() {
    let found = production_sql(inserts_worktree_without_state);
    assert!(
        found.is_empty(),
        "an INSERT INTO worktrees does not write the state column:\n{}",
        found.join("\n")
    );
}

/// The matcher of the rule above.
#[test]
fn worktree_insert_matcher_finds_an_insert_without_state() {
    for hit in [
        "INSERT INTO worktrees (worktree_id, path, status) VALUES (?1, ?2, ?3)",
        "insert or ignore into main.worktrees (worktree_id, path) values (?1, ?2)",
        "INSERT INTO worktrees SELECT * FROM other",
        "REPLACE INTO worktrees (worktree_id, state_text) VALUES (?1, ?2)",
    ] {
        assert!(inserts_worktree_without_state(hit), "{hit}");
    }
    for miss in [
        "INSERT INTO worktrees (worktree_id, path, status, state) VALUES (?1, ?2, ?3, ?4)",
        "INSERT INTO worktrees (worktree_id,\n state, path) VALUES (?1, ?2, ?3)",
        "INSERT INTO worktrees_v5 (worktree_id) SELECT worktree_id FROM worktrees",
        "INSERT INTO worktree_modes (worktree_id, run_id) VALUES (?1, ?2)",
        "UPDATE worktrees SET status = ?1",
    ] {
        assert!(!inserts_worktree_without_state(miss), "{miss}");
    }
}

/// The files of fleetd, the 1 writer of the ledger from Phase 5 (RR-5,
/// FM-24): its folder in `horch-core` and its composition root.
const FLEETD_FILES: &[&str] = &["horch-core/src/fleetd/", "horch/src/cmd/fleetd.rs"];

/// The 1 file that makes the writer of fleetd: `horch fleetd` and
/// `horch ledger watch --once`, which takes the fleetd lock.
const FLEETD_ROOT: &str = "horch/src/cmd/fleetd.rs";

/// The writable opens of the ledger outside [`FLEETD_FILES`]: the path, a
/// part of the line, and why. An entry with a ticket goes when that ticket
/// lands, and gate T-498 does not pass while 1 of them stays.
const LEDGER_WRITERS_EXEMPT: &[(&str, &str, &str)] = &[
    (
        "horch-core/src/taskledger/store.rs",
        "Connection::open(path)",
        "Store::open itself: it creates and migrates the ledger, and checks the seam first",
    ),
    (
        "horch-core/src/taskledger/store.rs",
        "Store::open(path)",
        "LedgerWriter::open: the writable open of fleetd, which only cmd/fleetd.rs makes, \
         and Store::second_writer, which needs a writable store first",
    ),
    (
        "horch-core/src/taskledger/schema.rs",
        "Connection::open(file)",
        "the migration copies the ledger to its backup; only Store::open runs it",
    ),
    (
        "horch-core/src/taskledger/mod.rs",
        "Store::open(&ctx.paths.ledger_db)",
        "taskledger::open, a writable open that only tests call",
    ),
];

/// True when `line` holds `needle` as the start of a path or a name: the
/// character before it is not part of a name.
fn starts_name(line: &str, at: usize) -> bool {
    !line[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The calls of `line` that open the ledger for a write, or make the writer
/// of fleetd. `ledger_store` says whether a bare `Store` of the file is the
/// ledger store (and not, for example, the telemetry store).
fn ledger_write_calls(line: &str, ledger_store: bool) -> Vec<&'static str> {
    let mut found = Vec::new();
    let calls = |needle: &str| -> Vec<usize> {
        line.match_indices(needle)
            .map(|(at, _)| at)
            .filter(|at| starts_name(line, *at) && !line[..*at].trim_end().ends_with("fn"))
            .collect()
    };
    for at in calls("Store::open(") {
        let before = &line[..at];
        let qualified = before.ends_with("taskledger::") || before.ends_with("taskledger::store::");
        if qualified || (ledger_store && !before.ends_with("::")) {
            found.push("Store::open(");
        }
    }
    for needle in [
        "taskledger::open(",
        "open_existing(",
        "Connection::open(",
        "ledger_conn(",
        "LedgerWriter::fleetd(",
    ] {
        if !calls(needle).is_empty() {
            found.push(needle);
        }
    }
    found
}

/// Whether a bare `Store` in the file at `rel` (relative to `crates/`)
/// with the production `lines` is the ledger store: a file of the ledger
/// module, or a file whose `use` of `taskledger` names `Store`.
fn names_ledger_store(rel: &str, lines: &[(usize, String)]) -> bool {
    if rel.starts_with("horch-core/src/taskledger/") {
        return true;
    }
    let text: String = lines.iter().map(|(_, l)| format!("{l}\n")).collect();
    text.split(';').any(|stmt| {
        let stmt = stmt.trim_start();
        let Some(path) = stmt
            .strip_prefix("pub use ")
            .or_else(|| stmt.strip_prefix("pub(crate) use "))
            .or_else(|| stmt.strip_prefix("use "))
        else {
            return false;
        };
        let words: Vec<&str> = path
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|w| !w.is_empty())
            .collect();
        words.contains(&"taskledger") && words.contains(&"Store")
    })
}

/// Each writable open of the ledger, and each writer of fleetd that is
/// made, in the production code outside [`FLEETD_FILES`], as
/// `<path>:<line>: <call>: <text>`.
fn ledger_writers() -> Vec<String> {
    let (root, files) = production_files();
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let prod = production_lines(&std::fs::read_to_string(file).unwrap());
        test_paths.extend(test_module_paths(file, &prod.test_mods));
        scanned.push((file, prod.lines));
    }
    let mut found = Vec::new();
    for (file, lines) in scanned {
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
        {
            continue;
        }
        let rel = relative(file, &root);
        let in_fleetd = FLEETD_FILES.iter().any(|f| rel.starts_with(f));
        let ledger_store = names_ledger_store(&rel, &lines);
        for (n, line) in &lines {
            for call in ledger_write_calls(line, ledger_store) {
                let allowed = if call == "LedgerWriter::fleetd(" {
                    rel == FLEETD_ROOT
                } else {
                    in_fleetd
                };
                if !allowed {
                    found.push(format!("{rel}:{n}: {call}: {}", line.trim()));
                }
            }
        }
    }
    found
}

fn ledger_writer_exempt(found: &str) -> bool {
    LEDGER_WRITERS_EXEMPT
        .iter()
        .any(|(path, part, _)| found.starts_with(&format!("{path}:")) && found.contains(part))
}

/// RR-5, FM-24 (Phase 1 Q13): no production file outside fleetd opens the
/// ledger for a write, and only the composition root of fleetd makes the
/// writer of fleetd. Each other process opens the ledger with
/// `open_existing_read_only`.
#[test]
fn fitness_only_fleet_runtime_opens_the_ledger_writable() {
    let mut found = ledger_writers();
    found.retain(|f| !ledger_writer_exempt(f));
    assert!(
        found.is_empty(),
        "a process other than fleetd opens the ledger for a write; open it with \
         taskledger::open_existing_read_only, or send a request to fleetd:\n{}",
        found.join("\n")
    );
}

/// Each exemption of [`LEDGER_WRITERS_EXEMPT`] still exempts a writer.
#[test]
fn each_ledger_writer_exemption_is_still_needed() {
    let found = ledger_writers();
    for (path, part, why) in LEDGER_WRITERS_EXEMPT {
        assert!(
            found
                .iter()
                .any(|f| f.starts_with(&format!("{path}:")) && f.contains(part)),
            "{path} has no `{part}` any more; remove its exemption ({why})"
        );
    }
}

/// The matchers of the rule above.
#[test]
fn ledger_writer_matcher_finds_each_writable_open() {
    for (line, store, call) in [
        ("let s = Store::open(&db)?;", true, "Store::open("),
        ("crate::taskledger::Store::open(&db)", false, "Store::open("),
        (
            "horch_core::taskledger::store::Store::open(p)",
            false,
            "Store::open(",
        ),
        (
            "let s = taskledger::open(ctx)?;",
            false,
            "taskledger::open(",
        ),
        (
            "horch_core::taskledger::open_existing(ctx)",
            false,
            "open_existing(",
        ),
        (
            "let c = Connection::open(path)?;",
            false,
            "Connection::open(",
        ),
        (
            "rusqlite::Connection::open(&db)",
            false,
            "Connection::open(",
        ),
        (
            "let c = super::fleetd::ledger_conn(&db)?;",
            false,
            "ledger_conn(",
        ),
        (
            "let w = LedgerWriter::fleetd();",
            false,
            "LedgerWriter::fleetd(",
        ),
    ] {
        assert_eq!(ledger_write_calls(line, store), [call], "{line}");
    }
    for (line, store) in [
        ("let s = Store::open(&dir, now, days)?;", false),
        ("ExecutionStore::open(&ctx.paths, &root)", true),
        ("store::Store::open(&dir)", true),
        ("taskledger::open_existing_read_only(ctx)?", false),
        ("Store::open_read_only(&db)", true),
        ("Connection::open_in_memory()", false),
        (
            "Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY)",
            false,
        ),
        (
            "pub fn open_existing(ctx: &RuntimeContext) -> Result<Option<Store>> {",
            false,
        ),
        (
            "pub(crate) fn ledger_conn(ledger: &Path) -> Result<Connection> {",
            false,
        ),
        ("let w = writer.open(&db)?;", true),
    ] {
        assert!(ledger_write_calls(line, store).is_empty(), "{line}");
    }
    let lines = |src: &str| -> Vec<(usize, String)> {
        src.lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect()
    };
    for src in [
        "use horch_core::taskledger::{self, Store};",
        "use horch_core::taskledger::{\n    self,\n    Store,\n};",
        "use crate::taskledger::store::Store;",
    ] {
        assert!(
            names_ledger_store("horch/src/cmd/x.rs", &lines(src)),
            "{src}"
        );
    }
    for src in [
        "use super::store::{self, Store};",
        "use horch_core::taskledger::{self, runs};",
    ] {
        assert!(
            !names_ledger_store("horch-core/src/telemetry/x.rs", &lines(src)),
            "{src}"
        );
    }
    assert!(names_ledger_store("horch-core/src/taskledger/x.rs", &[]));
}

/// SR-7: the 2 production files that may start a process directly: the
/// platform start function and the launcher (docs/tickets/fleetd-p7/MAP.md,
/// Q1). Each removes each forbidden name before the start.
const PROCESS_START_ALLOWED: [&str; 2] = [
    "horch-platform/src/start.rs",
    "horch-core/src/fleetd/worker_profiles/adapters/launcher.rs",
];

/// Each direct process start in the production code of `text`, with its
/// line number: a `Command::new` of a `process` module (`std::process`), or
/// a bare `Command::new`. `horch_platform::start::Command` has no `new` (its
/// start is `start::command`), so a bare `Command::new` is a `std` start. A
/// `Command` of another module (`clap::Command`) and a
/// `std::process::Command` as a type are not starts.
fn process_starts(text: &str) -> Vec<(usize, String)> {
    let production = production_lines(text);
    let texts: Vec<&str> = production.lines.iter().map(|(_, l)| l.as_str()).collect();
    code_paths(&lex(&texts).code)
        .into_iter()
        .filter(|(_, segs)| {
            segs.windows(2)
                .enumerate()
                .any(|(k, w)| w == ["Command", "new"] && (k == 0 || segs[k - 1] == "process"))
        })
        .map(|(i, segs)| (production.lines[i].0, segs.join("::")))
        .collect()
}

/// Each direct process start in the production code under `crates`, as
/// `<crate>/src/<path>:<line>: <path>`, outside `PROCESS_START_ALLOWED`.
/// It reads the `src/` of each crate. `horch-e2e` is test code: its library
/// starts the development binaries with the environment that a test gives,
/// and its `src/bin/` holds the fake agents. `ui-prop-tester` is a
/// standalone tool that no horch crate links. A `tests.rs` file and each
/// module that a test-only `cfg` declares are test code.
fn process_starts_under(crates: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        let dir = entry.unwrap().path();
        let own = dir
            .file_name()
            .is_some_and(|n| n != "horch-e2e" && n != "ui-prop-tester");
        if own && dir.join("src").is_dir() {
            rust_files(&dir.join("src"), &mut files);
        }
    }
    let mut scanned = Vec::new();
    let mut test_paths: Vec<PathBuf> = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        test_paths.extend(test_module_paths(file, &production_lines(&text).test_mods));
        scanned.push((file, process_starts(&text)));
    }
    let mut found = Vec::new();
    for (file, starts) in scanned {
        let rel = relative(file, crates);
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_paths.iter().any(|t| file.starts_with(t))
            || PROCESS_START_ALLOWED.contains(&rel.as_str())
        {
            continue;
        }
        for (line, path) in starts {
            found.push(format!("{rel}:{line}: {path}"));
        }
    }
    found.sort();
    found
}

/// The scans of PL-2 and SR-7 skip `ui-prop-tester`. That is safe only
/// while no other crate of the workspace has it in its dependencies.
#[test]
fn no_crate_of_horch_depends_on_ui_prop_tester() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let dir = entry.unwrap().path();
        let manifest = dir.join("Cargo.toml");
        if dir.file_name().is_some_and(|n| n == "ui-prop-tester") || !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest).unwrap();
        if text.contains("ui-prop-tester") || text.contains("ui_prop_tester") {
            found.push(relative(&manifest, &crates));
        }
    }
    assert!(
        found.is_empty(),
        "these crates name ui-prop-tester: {found:?}"
    );
}

/// SR-7: production code starts each process through the launcher or the
/// platform start function, and through no other `Command::new`.
#[test]
fn fitness_no_process_start_outside_the_launcher() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for allowed in PROCESS_START_ALLOWED {
        assert!(crates.join(allowed).is_file(), "{allowed} is missing");
    }
    let found = process_starts_under(&crates);
    assert!(
        found.is_empty(),
        "start the process with horch_platform::start::Command or the launcher:\n{}",
        found.join("\n")
    );
}

/// The process start rule's own cases: a start planted in a temporary file
/// is found with its file and line, and test code, the 2 allowed files,
/// `horch-e2e`, comments, strings and a `Command` of another module are not.
#[test]
fn fitness_process_start_rule_finds_a_planted_start() {
    let tmp = tempfile::tempdir().unwrap();
    let plant = |rel: &str, text: &str| {
        let path = tmp.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    plant(
        "a/src/lib.rs",
        "#[cfg(all(test, unix))]\nmod test_support;\nmod qualified;\nmod imported;\n\
         mod moved;\nmod grouped;\nmod typed;\n#[cfg(test)]\nmod tests {\n    \
         fn t() { std::process::Command::new(\"sh\"); }\n}\n",
    );
    plant(
        "a/src/qualified.rs",
        "#[cfg(test)]\nconst VARS: [&str; 1] = {\n    [\"\"; 1]\n};\n\
         // std::process::Command::new(\"sh\")\nfn f() {\n    \
         let s = \"std::process::Command::new\";\n    \
         let c = std::process::Command::new(\"sh\");\n}\n",
    );
    plant(
        "a/src/imported.rs",
        "use std::process::{Child, Command};\n\nfn f() -> Child {\n    \
         Command::new(\"sh\").spawn().unwrap()\n}\n",
    );
    plant(
        "a/src/grouped.rs",
        "use std::{fs, process};\nfn f() {\n    process::Command::new(\"sh\");\n}\n",
    );
    plant(
        "a/src/moved.rs",
        "use horch_platform::start;\n\
         fn f() {\n    start::command(\"sh\").spawn();\n    \
         clap::Command::new(\"ledger\");\n}\n",
    );
    plant(
        "a/src/typed.rs",
        "fn f(c: &mut std::process::Command) -> &mut std::process::Command {\n    c\n}\n",
    );
    plant(
        "a/src/test_support.rs",
        "fn f() { std::process::Command::new(\"sh\"); }\n",
    );
    plant(
        "a/src/x/tests.rs",
        "fn f() { std::process::Command::new(\"sh\"); }\n",
    );
    let start = "fn f() { std::process::Command::new(\"sh\"); }\n";
    plant("horch-platform/src/start.rs", start);
    plant(
        "horch-core/src/fleetd/worker_profiles/adapters/launcher.rs",
        start,
    );
    plant("horch-e2e/src/harness.rs", start);
    plant("horch-e2e/src/bin/fake-claude.rs", start);
    assert_eq!(
        process_starts_under(tmp.path()),
        vec![
            "a/src/grouped.rs:3: process::Command::new",
            "a/src/imported.rs:4: Command::new",
            "a/src/qualified.rs:8: std::process::Command::new",
        ]
    );
    let bare = "fn f() { Command::new(\"sh\"); }\n";
    assert_eq!(process_starts(bare), vec![(1, "Command::new".to_string())]);
}
