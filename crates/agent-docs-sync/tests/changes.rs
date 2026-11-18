//! CHANGES.md and the `also_git` source (step 2 of 1 sync). Each page comes
//! from memory through the fetch port, and each repository is a local bare
//! repository in a temporary folder; no test uses the network.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use agent_docs_sync::{parse_sources, sync, Fetch, Outcome, Report, Source};

const INDEX: &str = "https://docs.test/llms.txt";

/// The tests that start git hold this lock, because 1 test changes `PATH`
/// and the forbidden names of this process.
static GIT: Mutex<()> = Mutex::new(());

fn git_lock() -> MutexGuard<'static, ()> {
    GIT.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fetch port over pages in memory, by file stem.
struct Pages(BTreeMap<String, String>);

impl Pages {
    fn new(pages: &[(&str, &str)]) -> Self {
        let mut map: BTreeMap<String, String> = pages
            .iter()
            .map(|(stem, text)| (format!("https://docs.test/{stem}.md"), text.to_string()))
            .collect();
        let index: String = pages
            .iter()
            .map(|(stem, _)| format!("- [{stem}](https://docs.test/{stem}.md): {stem}\n"))
            .collect();
        map.insert(INDEX.to_string(), index);
        Pages(map)
    }
}

impl Fetch for Pages {
    fn get(&self, url: &str) -> anyhow::Result<String> {
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("404 for {url}"))
    }
}

/// A codex source with these includes and, when given, an `also_git`.
fn codex(include: &[&str], also_git: Option<&Path>) -> Source {
    let include: Vec<String> = include.iter().map(|i| format!("{i:?}")).collect();
    let mut text = format!(
        "[codex]\nkind = \"llms_txt\"\nurl = \"{INDEX}\"\nmd_suffix = \".md\"\n\
         include = [{}]\nversion_cmd = \"codex --version\"\n",
        include.join(", ")
    );
    if let Some(repo) = also_git {
        text.push_str(&format!("also_git = {:?}\n", repo.display().to_string()));
    }
    parse_sources(&text).unwrap().remove(0)
}

/// Sync `pages` as `version` of codex, with the includes of `pages`.
fn sync_pages(out: &Path, version: &str, pages: &[(&str, &str)]) -> Report {
    let include: Vec<&str> = pages.iter().map(|(stem, _)| *stem).collect();
    let report = sync(&codex(&include, None), version, &Pages::new(pages), out).unwrap();
    assert!(report.failed().is_empty(), "{report:?}");
    report
}

fn changes(out: &Path) -> String {
    std::fs::read_to_string(out.join("codex/CHANGES.md")).expect("CHANGES.md is written")
}

/// The lines of the section `## <title>` of CHANGES.md.
fn section(text: &str, title: &str) -> Vec<String> {
    let head = format!("## {title}\n");
    let start = text
        .find(&head)
        .unwrap_or_else(|| panic!("no section {title}:\n{text}"))
        + head.len();
    let body = &text[start..];
    let end = body.find("\n## ").unwrap_or(body.len());
    body[..end]
        .lines()
        .filter(|l| l.starts_with("- "))
        .map(str::to_string)
        .collect()
}

#[test]
fn changes_list_added_updated_and_removed_pages() {
    let out = tempfile::tempdir().unwrap();
    sync_pages(
        out.path(),
        "1.0.0",
        &[
            ("config-reference", "old text\n"),
            ("sandboxing", "same text\n"),
            ("hooks", "hooks\n"),
        ],
    );
    sync_pages(
        out.path(),
        "1.1.0",
        &[
            ("config-reference", "new text\n"),
            ("sandboxing", "same text\n"),
            ("agents-md", "agents\n"),
        ],
    );

    let text = changes(out.path());
    assert!(text.contains("1.0.0"), "{text}");
    assert_eq!(
        section(&text, "Pages"),
        [
            "- added: `agents-md.md`",
            "- updated: `config-reference.md`",
            "- removed: `hooks.md`",
        ],
        "{text}"
    );
    let versions = std::fs::read_to_string(out.path().join("codex/versions.json")).unwrap();
    let versions: Vec<String> = serde_json::from_str(&versions).unwrap();
    assert_eq!(versions, ["1.0.0", "1.1.0"]);
}

#[test]
fn changes_list_changed_flags_env_vars_and_settings_keys() {
    let out = tempfile::tempdir().unwrap();
    sync_pages(
        out.path(),
        "1.0.0",
        &[(
            "config-reference",
            "Use `--model <id>` and `--old-flag`. Set `CODEX_HOME` or `OLD_VAR`.\n\
             The keys `sandbox.mode` and `old.key`. Not `plain`, not `HOME`.\n",
        )],
    );
    sync_pages(
        out.path(),
        "1.1.0",
        &[(
            "config-reference",
            "Use `--model <name>` and `--new-flag`. Set `CODEX_HOME` or `NEW_VAR`.\n\
             The keys `sandbox.mode` and `model:provider`. Not `plain`, not `PATH`.\n",
        )],
    );

    let text = changes(out.path());
    assert_eq!(
        section(&text, "Flags"),
        ["- added: `--new-flag`", "- removed: `--old-flag`"],
        "{text}"
    );
    assert_eq!(
        section(&text, "Env vars"),
        ["- added: `NEW_VAR`", "- removed: `OLD_VAR`"],
        "{text}"
    );
    assert_eq!(
        section(&text, "Settings keys"),
        ["- added: `model:provider`", "- removed: `old.key`"],
        "{text}"
    );
}

#[test]
fn first_sync_writes_first_sync() {
    let out = tempfile::tempdir().unwrap();
    sync_pages(out.path(), "1.0.0", &[("config-reference", "text\n")]);

    let text = changes(out.path());
    assert!(text.contains("first sync"), "{text}");
    let versions = std::fs::read_to_string(out.path().join("codex/versions.json")).unwrap();
    let versions: Vec<String> = serde_json::from_str(&versions).unwrap();
    assert_eq!(versions, ["1.0.0"]);
}

/// Run git in test code, which is not production code (SR-7).
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@test.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("start git");
    assert!(status.success(), "git {args:?}");
}

/// A bare repository with `docs/guide.md` and `src/main.rs`.
fn bare_repo(root: &Path) -> std::path::PathBuf {
    let work = root.join("work");
    std::fs::create_dir_all(work.join("docs")).unwrap();
    std::fs::create_dir_all(work.join("src")).unwrap();
    std::fs::write(work.join("docs/guide.md"), "# Guide\n").unwrap();
    std::fs::write(work.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&work, &["init", "-q"]);
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "docs"]);
    let bare = root.join("repo.git");
    git(root, &["clone", "-q", "--bare", "work", "repo.git"]);
    bare
}

fn git_outcome(report: &Report) -> &Outcome {
    &report
        .pages
        .iter()
        .find(|p| p.file == "git")
        .unwrap_or_else(|| panic!("no git entry: {report:?}"))
        .outcome
}

#[test]
fn also_git_copies_the_docs_folder_from_a_local_repository() {
    let _lock = git_lock();
    let root = tempfile::tempdir().unwrap();
    let repo = bare_repo(root.path());
    let out = root.path().join("out");

    let source = codex(&["config-reference"], Some(&repo));
    let pages = Pages::new(&[("config-reference", "text\n")]);
    let report = sync(&source, "1.0.0", &pages, &out).unwrap();

    assert_eq!(git_outcome(&report), &Outcome::Added, "{report:?}");
    let copy = out.join("codex/1.0.0/git");
    assert_eq!(
        std::fs::read_to_string(copy.join("docs/guide.md")).unwrap(),
        "# Guide\n"
    );
    assert!(!copy.join("src/main.rs").exists(), "the clone is sparse");
}

/// A name that the fake git must see, so the test cannot pass because the
/// fake git wrote no environment.
const CONTROL: &str = "AGENT_DOCS_SYNC_TEST_CONTROL";

#[cfg(unix)]
#[test]
fn also_git_starts_git_through_the_platform_start_function() {
    use horch_platform::start::FORBIDDEN_ENV;
    use std::os::unix::fs::PermissionsExt;

    let _lock = git_lock();
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let env_file = root.path().join("git.env");
    let fake = bin.join("git");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\nenv > '{}'\nexit 1\n", env_file.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path = std::ffi::OsString::from(bin.as_os_str());
    path.push(":");
    path.push(&old_path);
    std::env::set_var("PATH", &path);
    std::env::set_var(CONTROL, "1");
    for name in FORBIDDEN_ENV {
        std::env::set_var(name, "forbidden-in-test");
    }

    let source = codex(&["config-reference"], Some(Path::new("/nowhere/repo.git")));
    let pages = Pages::new(&[("config-reference", "text\n")]);
    let report = sync(&source, "1.0.0", &pages, &root.path().join("out"));

    for name in FORBIDDEN_ENV {
        std::env::remove_var(name);
    }
    std::env::remove_var(CONTROL);
    std::env::set_var("PATH", &old_path);

    let report = report.unwrap();
    assert!(
        matches!(git_outcome(&report), Outcome::Failed(_)),
        "{report:?}"
    );
    let env = std::fs::read_to_string(&env_file).expect("the fake git ran");
    assert!(
        env.lines().any(|l| l.starts_with(&format!("{CONTROL}="))),
        "the fake git wrote no environment:\n{env}"
    );
    for name in FORBIDDEN_ENV {
        assert!(
            !env.lines().any(|l| l.starts_with(&format!("{name}="))),
            "git has {name}"
        );
    }
}
