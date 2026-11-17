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

/// A child's environment is cleaned in 1 place, `scrub_child_env` (#88): a
/// hand-built removal of the API key is the sign of a child that keeps the
/// caller's `GIT_DIR`. Only the runtime and the launch builder name it.
#[test]
fn arc_88_children_are_scrubbed_in_one_place() {
    let needle = concat!("env_remove(\"ANTHROPIC", "_API_KEY\")");
    let mut found = Vec::new();
    for krate in [
        "horch-core",
        "horch",
        "horch-marketplace",
        "herdr-install",
        "herdr-docs-sync",
    ] {
        let root = crate_src(krate);
        if !root.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        files.retain(|f| {
            let rel = relative(f, &root);
            !(krate == "horch-core" && (rel.starts_with("runtime/") || rel == "harness/launch.rs"))
        });
        for line in forbidden_lines(&root, &files, &[needle]) {
            found.push(format!("{krate}/{line}"));
        }
    }
    assert!(
        found.is_empty(),
        "use runtime::process::scrub_child_env:\n{}",
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

/// The production lines of `text`, numbered from 1, and the names of the
/// child modules that a `cfg` naming `test` declares (`mod x;`). Unlike
/// `code_lines`, a group of attributes, also over more than 1 line, that
/// holds a `cfg` naming `test` is not code. When the group is over an inline
/// `mod`, the rest of the file is not code.
fn production_lines(text: &str) -> (Vec<(usize, String)>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let is_attr = |l: &str| l.trim_start().starts_with("#[") || l.trim_start().starts_with("#![");
    let mut out = Vec::new();
    let mut test_mods = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("//") {
            i += 1;
            continue;
        }
        if !is_attr(lines[i]) {
            out.push((i + 1, lines[i].to_string()));
            i += 1;
            continue;
        }
        let mut group = Vec::new();
        while i < lines.len() && (is_attr(lines[i]) || lines[i].trim_start().starts_with("//")) {
            if lines[i].trim_start().starts_with("//") {
                i += 1;
                continue;
            }
            let mut depth = 0;
            while i < lines.len() {
                depth +=
                    lines[i].matches('[').count() as i32 - lines[i].matches(']').count() as i32;
                group.push((i + 1, lines[i].to_string()));
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
        }
        let attrs: String = group.iter().map(|(_, l)| format!("{l}\n")).collect();
        let gated = cfg_predicates(&attrs)
            .iter()
            .any(|(_, gate, pred)| *gate && names_test(pred));
        if !gated {
            out.extend(group);
            continue;
        }
        let item = lines.get(i).map_or("", |l| l.trim_start());
        let item = item
            .strip_prefix("pub(crate) ")
            .or_else(|| item.strip_prefix("pub "))
            .unwrap_or(item);
        if let Some(rest) = item.strip_prefix("mod ") {
            match rest.split_once(';') {
                Some((name, _)) => {
                    test_mods.push(name.trim().to_string());
                    i += 1;
                }
                None => break,
            }
        }
    }
    (out, test_mods)
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

/// Each `cfg(..)`, `cfg_attr(..)` and `cfg!(..)` of `text`: its byte
/// position, whether it gates an item (`cfg(..)` only), and its predicate
/// (the first argument of `cfg_attr`).
fn cfg_predicates(text: &str) -> Vec<(usize, bool, String)> {
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
        let (open, gate, attr) = if rest.starts_with("_attr(") {
            (at + 8, false, true)
        } else if rest.starts_with("!(") {
            (at + 4, false, false)
        } else if rest.starts_with('(') {
            (at + 3, true, false)
        } else {
            continue;
        };
        let Some(body) = balanced(text, open) else {
            continue;
        };
        let pred = if attr { first_argument(body) } else { body };
        out.push((at, gate, pred.to_string()));
    }
    out
}

/// The text of `body` before its first comma outside parentheses.
fn first_argument(body: &str) -> &str {
    let mut depth = 0;
    for (i, c) in body.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return &body[..i],
            _ => {}
        }
    }
    body
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

fn names_test(pred: &str) -> bool {
    predicate_words(pred).contains(&"test") && !pred.replace(' ', "").contains("not(test)")
}

/// The OS `cfg` sites of `text`: `cfg(..)`, `cfg_attr(..)` and `cfg!(..)`
/// whose predicate names an OS word, each as its first line and its text.
fn os_cfg_sites(text: &str) -> (Vec<(usize, String)>, Vec<String>) {
    let (lines, test_mods) = production_lines(text);
    let mut joined = String::new();
    let mut starts = Vec::new();
    for (_, line) in &lines {
        starts.push(joined.len());
        joined.push_str(line);
        joined.push('\n');
    }
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (at, _, pred) in cfg_predicates(&joined) {
        if !names_os(&pred) || names_test(&pred) {
            continue;
        }
        let k = starts.partition_point(|s| *s <= at) - 1;
        if sites.last().is_none_or(|(n, _)| *n != lines[k].0) {
            sites.push((lines[k].0, lines[k].1.trim().to_string()));
        }
    }
    (sites, test_mods)
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
#[cfg(unix)]
#[cfg(test)]
mod tests {
    #[cfg(windows)]
    fn i() {}
}
";
    let (sites, test_mods) = os_cfg_sites(text);
    let lines: Vec<usize> = sites.iter().map(|(n, _)| *n).collect();
    assert_eq!(lines, vec![2, 4, 6, 8, 10, 11], "{sites:?}");
    assert_eq!(sites[5].1, "#[cfg(not(any(");
    assert_eq!(test_mods, vec!["test_support".to_string()]);

    let gated =
        "fn a() {}\n#[cfg(all(test, unix))]\nmod tests {\n    #[cfg(unix)]\n    fn b() {}\n}\n";
    assert_eq!(os_cfg_sites(gated).0, Vec::new());
    assert_eq!(
        production_lines(gated).0,
        vec![(1, "fn a() {}".to_string())]
    );
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
        if member == "crates/horch-platform" || member == "crates/horch-e2e" {
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
    let mut test_only: Vec<PathBuf> = Vec::new();
    for file in &files {
        let (sites, test_mods) = os_cfg_sites(&std::fs::read_to_string(file).unwrap());
        test_only.extend(test_module_paths(file, &test_mods));
        scanned.push((file, sites));
    }
    let mut found = Vec::new();
    for (file, sites) in scanned {
        if file.file_name().is_some_and(|n| n == "tests.rs")
            || test_only.iter().any(|t| file.starts_with(t))
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
            !os_cfg_sites(&text).0.is_empty(),
            "{path} has no OS cfg any more; remove its exemption ({why})"
        );
    }
}
