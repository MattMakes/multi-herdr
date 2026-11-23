use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_docs_sync::{
    parse_sources, sources, sync, Fetch, Kind, Manifest, Outcome, Report, Source, MANIFEST_FILE,
};
use sha2::{Digest, Sha256};

struct Fixture {
    pages: BTreeMap<String, PathBuf>,
    asked: RefCell<Vec<String>>,
}

impl Fixture {
    fn new(tool: &str, pages: &[(&str, &str)]) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(tool);
        Fixture {
            pages: pages
                .iter()
                .map(|(url, file)| (url.to_string(), dir.join(file)))
                .collect(),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Fetch for Fixture {
    fn get(&self, url: &str) -> anyhow::Result<String> {
        self.asked.borrow_mut().push(url.to_string());
        match self.pages.get(url) {
            Some(path) => Ok(std::fs::read_to_string(path)?),
            None => anyhow::bail!("404 for {url}"),
        }
    }
}

fn herdr_fixture() -> Fixture {
    Fixture::new(
        "herdr",
        &[
            ("https://herdr.dev/sitemap.xml", "sitemap.xml"),
            ("https://herdr.dev/docs/", "index.html"),
            ("https://herdr.dev/docs/install/", "install.html"),
        ],
    )
}

fn source(name: &str) -> Source {
    sources()
        .unwrap()
        .into_iter()
        .find(|s| s.tool == name)
        .unwrap()
}

fn small_claude(include: &str) -> Source {
    let text = format!(
        "[claude]\nkind = \"llms_txt\"\nurl = \"https://code.claude.com/docs/llms.txt\"\n\
         md_suffix = \".md\"\ninclude = [{include}]\nversion_cmd = \"claude --version\"\n"
    );
    parse_sources(&text).unwrap().remove(0)
}

fn claude_fixture() -> Fixture {
    Fixture::new(
        "claude",
        &[
            ("https://code.claude.com/docs/llms.txt", "llms.txt"),
            (
                "https://code.claude.com/docs/en/cli-reference.md",
                "cli-reference.md",
            ),
            ("https://code.claude.com/docs/en/hooks.md", "hooks.md"),
        ],
    )
}

fn outcomes(report: &Report) -> BTreeMap<String, Outcome> {
    report
        .pages
        .iter()
        .map(|p| (p.file.clone(), p.outcome.clone()))
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn sources_file_has_the_4_tools_of_the_design() {
    let all = sources().unwrap();
    let names: Vec<&str> = all.iter().map(|s| s.tool.as_str()).collect();
    assert_eq!(names, ["agy", "claude", "codex", "herdr"]);

    let herdr = source("herdr");
    assert_eq!(herdr.kind, Kind::Sitemap);
    assert_eq!(herdr.url, "https://herdr.dev/sitemap.xml");
    assert_eq!(herdr.version_cmd, "herdr --version");

    let claude = source("claude");
    assert_eq!(claude.kind, Kind::LlmsTxt);
    assert_eq!(claude.url, "https://code.claude.com/docs/llms.txt");
    assert_eq!(claude.md_suffix.as_deref(), Some(".md"));
    assert_eq!(
        claude.include,
        [
            "cli-reference",
            "env-vars",
            "settings",
            "hooks",
            "sandboxing",
            "skills",
            "mcp",
            "headless"
        ]
    );
    assert_eq!(claude.version_cmd, "claude --version");

    let codex = source("codex");
    assert_eq!(codex.kind, Kind::LlmsTxt);
    assert_eq!(codex.url, "https://learn.chatgpt.com/docs/llms.txt");
    assert_eq!(
        codex.include,
        [
            "config-reference",
            "non-interactive-mode",
            "sandboxing",
            "hooks",
            "agents-md",
            "build-skills"
        ]
    );
    assert_eq!(
        codex.also_git.as_deref(),
        Some("https://github.com/openai/codex")
    );
    assert_eq!(codex.version_cmd, "codex --version");

    let agy = source("agy");
    assert_eq!(agy.kind, Kind::LlmsTxt);
    assert_eq!(agy.url, "https://antigravity.google/llms.txt");
    assert_eq!(
        agy.include,
        [
            "cli/reference",
            "cli/headless",
            "settings",
            "hooks",
            "sandbox",
            "mcp",
            "skills",
            "rules"
        ]
    );
    assert_eq!(agy.version_cmd, "agy --version");
}

#[test]
fn unknown_source_key_is_an_error_that_names_it() {
    let text = "[herdr]\nkind = \"sitemap\"\nurl = \"https://herdr.dev/sitemap.xml\"\n\
                version_cmd = \"herdr --version\"\nmirror = true\n";
    let err = format!("{:#}", parse_sources(text).unwrap_err());
    assert!(err.contains("mirror"), "{err}");
    assert!(err.contains("herdr"), "{err}");
}

#[test]
fn llms_txt_source_fetches_only_the_included_pages_with_the_md_suffix() {
    let out = tempfile::tempdir().unwrap();
    let fetch = claude_fixture();
    let report = sync(
        &small_claude("\"cli-reference\", \"hooks\""),
        "2.1.0",
        &fetch,
        out.path(),
    )
    .unwrap();

    assert_eq!(
        *fetch.asked.borrow(),
        [
            "https://code.claude.com/docs/llms.txt",
            "https://code.claude.com/docs/en/cli-reference.md",
            "https://code.claude.com/docs/en/hooks.md",
        ]
    );
    let dir = out.path().join("claude/2.1.0");
    assert_eq!(
        read(&dir.join("cli-reference.md")),
        read(&fetch.pages["https://code.claude.com/docs/en/cli-reference.md"])
    );
    assert_eq!(
        read(&dir.join("hooks.md")),
        read(&fetch.pages["https://code.claude.com/docs/en/hooks.md"])
    );
    assert!(!dir.join("overview.md").exists());
    assert!(!dir.join("settings.md").exists());
    assert!(report.failed().is_empty(), "{report:?}");
}

#[test]
fn sitemap_source_keeps_the_herdr_behavior() {
    let out = tempfile::tempdir().unwrap();
    let dir = out.path().join("herdr/0.5.1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("retired.md"), "# Gone\n").unwrap();
    std::fs::write(dir.join("install.md"), "# Old text\n").unwrap();

    let report = sync(&source("herdr"), "0.5.1", &herdr_fixture(), out.path()).unwrap();
    assert_eq!(report.dir, dir);
    let seen = outcomes(&report);
    assert_eq!(seen["README.md"], Outcome::Added);
    assert_eq!(seen["install.md"], Outcome::Updated);
    assert_eq!(seen["retired.md"], Outcome::Removed);
    assert!(!dir.join("retired.md").exists(), "a stale page is pruned");

    let readme = read(&dir.join("README.md"));
    assert!(readme.starts_with("# Herdr\n"), "{readme}");
    assert!(readme.contains("Herdr runs agents in panes."), "{readme}");
    assert!(!readme.contains("Site menu"), "{readme}");
    assert!(read(&dir.join("install.md")).contains("`herdr-install`"));

    let again = sync(&source("herdr"), "0.5.1", &herdr_fixture(), out.path()).unwrap();
    let seen = outcomes(&again);
    assert_eq!(seen.len(), 2, "{again:?}");
    assert!(seen.values().all(|o| *o == Outcome::Unchanged), "{again:?}");
}

#[test]
fn pages_go_to_the_folder_of_the_version_with_a_manifest_of_hashes() {
    let out = tempfile::tempdir().unwrap();
    sync(&source("herdr"), "0.5.1", &herdr_fixture(), out.path()).unwrap();
    sync(&source("herdr"), "0.6.0", &herdr_fixture(), out.path()).unwrap();

    for version in ["0.5.1", "0.6.0"] {
        let dir = out.path().join("herdr").join(version);
        let manifest: Manifest = serde_json::from_str(&read(&dir.join(MANIFEST_FILE))).unwrap();
        assert_eq!(manifest.tool, "herdr");
        assert_eq!(manifest.version, version);
        let files: Vec<&str> = manifest.pages.keys().map(String::as_str).collect();
        assert_eq!(files, ["README.md", "install.md"]);
        for (file, hash) in &manifest.pages {
            let want = format!("{:x}", Sha256::digest(read(&dir.join(file))));
            assert_eq!(*hash, want, "{file}");
        }
    }

    for bad in ["", ".", "..", "1/2", "1\\2"] {
        let err = sync(&source("herdr"), bad, &herdr_fixture(), out.path()).unwrap_err();
        assert!(format!("{err:#}").contains("version"), "{bad:?}: {err:#}");
    }
}

#[test]
fn a_page_that_fails_is_reported_and_the_others_are_written() {
    let out = tempfile::tempdir().unwrap();
    let dir = out.path().join("claude/2.1.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("retired.md"), "# Gone\n").unwrap();

    let report = sync(
        &small_claude("\"cli-reference\", \"settings\", \"mcp\""),
        "2.1.0",
        &claude_fixture(),
        out.path(),
    )
    .unwrap();

    let seen = outcomes(&report);
    assert_eq!(seen["cli-reference.md"], Outcome::Added);
    let Outcome::Failed(why) = &seen["settings.md"] else {
        panic!("{report:?}")
    };
    assert!(why.contains("404"), "{why}");
    let Outcome::Failed(why) = &seen["mcp.md"] else {
        panic!("{report:?}")
    };
    assert!(why.contains("llms.txt"), "{why}");
    assert_eq!(report.failed().len(), 2);

    assert!(dir.join("cli-reference.md").is_file());
    assert!(dir.join("retired.md").is_file());
    assert!(!seen.contains_key("retired.md"));
    let manifest: Manifest = serde_json::from_str(&read(&dir.join(MANIFEST_FILE))).unwrap();
    assert!(manifest.pages.contains_key("cli-reference.md"));
}
