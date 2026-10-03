//! MKT-01..08, MKT-10 and NFR-07 for the marketplace crate. Git tests build
//! a bare repository fixture in a tempdir with real git. Without git on
//! PATH they skip, unless `HORCH_REQUIRE_GIT=1`.

use std::fs;
use std::path::{Path, PathBuf};

use horch_marketplace::{
    BundledFile, BundledSkill, Catalog, FaultPoint, GitError, GitRevision, GitRunner,
    InstallOptions, Installer, IntegrityViolation, LockEntry, Lockfile, MarketplaceError,
    ReinstallAction, SkillId, SkillSource, Step, Store,
};
use tempfile::TempDir;

/// The only code in this file that starts a process. Every fixture repo is
/// built through it, and it refuses a directory outside the temp dir.
mod fixture {
    use std::path::Path;
    use std::process::Command;

    pub fn git(config: &Path, dir: &Path, args: &[&str]) -> Option<String> {
        assert!(
            dir.starts_with(std::env::temp_dir()),
            "fixture git outside the temp dir: {}",
            dir.display()
        );
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .ok()?;
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }
}

struct Repo {
    tmp: TempDir,
    config: PathBuf,
}

impl Repo {
    /// A work repo at `<tmp>/work` that pushes to the bare `<tmp>/remote.git`.
    /// `None` when git is missing and not required.
    fn new() -> Option<Self> {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("gitconfig");
        fs::write(&config, "").unwrap();
        if fixture::git(&config, tmp.path(), &["--version"]).is_none() {
            assert!(
                std::env::var("HORCH_REQUIRE_GIT").as_deref() != Ok("1"),
                "HORCH_REQUIRE_GIT=1 but git is not on PATH"
            );
            eprintln!("skip: git is not on PATH");
            return None;
        }
        let repo = Self { tmp, config };
        repo.git(
            repo.tmp.path(),
            &["init", "--quiet", "--bare", "-b", "main", "remote.git"],
        );
        repo.git(repo.tmp.path(), &["init", "--quiet", "-b", "main", "work"]);
        repo.git(&repo.work(), &["remote", "add", "origin", "../remote.git"]);
        Some(repo)
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        fixture::git(&self.config, dir, args).expect("git ran before")
    }

    fn work(&self) -> PathBuf {
        self.tmp.path().join("work")
    }

    fn url(&self) -> String {
        format!("file://{}", self.tmp.path().join("remote.git").display())
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.work().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Commit everything, push `main` and tags, and return the commit.
    fn commit(&self, msg: &str) -> String {
        self.git(&self.work(), &["add", "-A"]);
        self.git(&self.work(), &["commit", "--quiet", "-m", msg]);
        self.git(
            &self.work(),
            &["push", "--quiet", "--tags", "origin", "main"],
        );
        self.git(&self.work(), &["rev-parse", "HEAD"])
    }

    fn source(&self, rev: &str) -> SkillSource {
        SkillSource::parse(&format!("{}@{rev}", self.url()))
            .unwrap()
            .with_subdir("skills/demo")
    }

    fn installer(&self) -> Installer {
        installer_at(self.tmp.path(), self.runner())
    }

    fn runner(&self) -> GitRunner {
        GitRunner::new("git").with_env("GIT_CONFIG_GLOBAL", &self.config)
    }

    fn store(&self) -> Store {
        Store::new(self.tmp.path().join("store"))
    }
}

fn installer_at(tmp: &Path, git: GitRunner) -> Installer {
    Installer::new(Store::new(tmp.join("store")), git, Catalog::new())
}

/// A runner whose binary does not exist: any git call fails to spawn.
fn no_git() -> GitRunner {
    GitRunner::new("/nonexistent/horch-test-git")
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: A demo skill.\n---\n# {name}\n")
}

fn local_skill(tmp: &Path, name: &str) -> PathBuf {
    let dir = tmp.join("src").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), skill_md(name)).unwrap();
    dir
}

fn local(dir: &Path) -> SkillSource {
    SkillSource::Local {
        path: dir.to_owned(),
    }
}

fn opts() -> InstallOptions {
    InstallOptions::default()
}

fn lock(store: &Store) -> Lockfile {
    Lockfile::read(&store.lock_path()).unwrap()
}

/// After a failed install: no lock entry and nothing left in staging.
fn assert_nothing_installed(store: &Store) {
    assert!(lock(store).skills.is_empty());
    let staging = store.staging_dir();
    if staging.exists() {
        assert_eq!(fs::read_dir(staging).unwrap().count(), 0);
    }
}

fn integrity_err(r: horch_marketplace::Result<LockEntry>) -> IntegrityViolation {
    match r {
        Err(MarketplaceError::Integrity(v)) => v,
        other => panic!("expected an integrity error, got {other:?}"),
    }
}

fn src_files() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.push((name, fs::read_to_string(&path).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn mkt_01_no_core_dependency() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(
        !manifest.contains("horch-core"),
        "Cargo.toml names horch-core"
    );
    for (name, text) in src_files() {
        let lower = text.to_ascii_lowercase();
        assert!(!lower.contains("horch_core"), "{name} names horch_core");
        assert!(!lower.contains("teammate"), "{name} knows about teammates");
    }
}

#[test]
fn mkt_02_branch_resolves_to_sha() {
    let Some(repo) = Repo::new() else { return };
    repo.write("skills/demo/SKILL.md", &skill_md("demo"));
    let first = repo.commit("first");
    repo.git(&repo.work(), &["tag", "-a", "v1", "-m", "v1"]);
    repo.git(&repo.work(), &["tag", "light"]);
    repo.git(
        &repo.work(),
        &["push", "--quiet", "--tags", "origin", "main"],
    );
    let installer = repo.installer();

    for rev in ["v1", "light", "main"] {
        let entry = installer.install(&repo.source(rev), &opts()).unwrap();
        assert_eq!(
            entry.resolved_commit.as_deref(),
            Some(first.as_str()),
            "{rev}"
        );
        assert_eq!(entry.requested_revision.as_deref(), Some(rev));
        assert_eq!(entry.version.0, format!("git+{}", &first[..12]));
    }
    let default = SkillSource::parse(&repo.url())
        .unwrap()
        .with_subdir("skills/demo");
    let entry = installer.install(&default, &opts()).unwrap();
    assert_eq!(entry.resolved_commit.as_deref(), Some(first.as_str()));
    assert_eq!(entry.requested_revision.as_deref(), Some("HEAD"));

    // Move the branch. The installed entry and its files do not change.
    let installed = installer.install(&repo.source("main"), &opts()).unwrap();
    repo.write("skills/demo/extra.md", "new");
    let second = repo.commit("second");
    assert_ne!(first, second);
    let locked = lock(&repo.store());
    assert_eq!(locked.skills, vec![installed.clone()]);
    let dir = repo.store().version_dir(&installed.id, &installed.version);
    assert!(!dir.join("extra.md").exists());

    // A new install resolves the moved branch; a pinned commit still works.
    let moved = installer.install(&repo.source("main"), &opts()).unwrap();
    assert_eq!(moved.resolved_commit.as_deref(), Some(second.as_str()));
    let pinned = installer.install(&repo.source(&first), &opts()).unwrap();
    assert_eq!(pinned.resolved_commit.as_deref(), Some(first.as_str()));
    assert!(matches!(
        &pinned.source,
        SkillSource::Git { revision: GitRevision::Commit(c), .. } if c == &first
    ));

    let missing = installer.install(&repo.source("no-such-ref"), &opts());
    assert!(matches!(
        missing,
        Err(MarketplaceError::RevisionNotFound { .. })
    ));
    let absent = installer.install(&repo.source(&"0".repeat(40)), &opts());
    assert!(absent.is_err());
    assert_eq!(lock(&repo.store()).skills, vec![pinned]);
}

#[test]
fn mkt_03_lifecycle_order() {
    let Some(repo) = Repo::new() else { return };
    repo.write("skills/demo/SKILL.md", &skill_md("demo"));
    repo.commit("first");
    let mut steps = Vec::new();
    repo.installer()
        .install_observed(&repo.source("main"), &opts(), Some(&mut steps))
        .unwrap();
    assert_eq!(
        steps,
        [
            Step::Catalog,
            Step::Resolve,
            Step::Fetch,
            Step::Validate,
            Step::Verify,
            Step::Materialize,
            Step::Lock,
        ]
    );
}

#[test]
fn mkt_04_lock_entry_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = local_skill(tmp.path(), "demo");
    let installer = installer_at(tmp.path(), no_git());
    installer.install(&local(&dir), &opts()).unwrap();

    let text = fs::read_to_string(installer.store().lock_path()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut top: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    top.sort();
    assert_eq!(top, ["skills", "version"]);
    assert_eq!(json["version"], 1);
    let entry = json["skills"][0].as_object().unwrap();
    let mut keys: Vec<_> = entry.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "digest",
            "id",
            "installed_at",
            "requested_revision",
            "resolved_commit",
            "source",
            "version",
        ]
    );
    assert_eq!(entry["id"], "demo");
    assert!(entry["digest"].as_str().unwrap().starts_with("sha256:"));
    assert!(entry["version"].as_str().unwrap().starts_with("local+"));
    assert!(entry["installed_at"].as_str().unwrap().ends_with('Z'));

    // Entries are sorted by id.
    installer
        .install(&local(&local_skill(tmp.path(), "alpha")), &opts())
        .unwrap();
    let ids: Vec<_> = lock(installer.store())
        .skills
        .iter()
        .map(|e| e.id.to_string())
        .collect();
    assert_eq!(ids, ["alpha", "demo"]);
}

#[test]
fn mkt_05_install_is_transactional() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = local_skill(tmp.path(), "demo");
    let installer = installer_at(tmp.path(), no_git());
    let faulted = InstallOptions {
        fault: FaultPoint::parse("abort-after-materialize-before-lock"),
    };
    assert!(faulted.fault.is_some());
    let err = installer.install(&local(&dir), &faulted).unwrap_err();
    assert!(matches!(err, MarketplaceError::Fault(_)), "{err}");
    assert_nothing_installed(installer.store());
    // The orphan version dir exists, but readers ignore it.
    assert_eq!(
        fs::read_dir(installer.store().skills_dir().join("demo"))
            .unwrap()
            .count(),
        1
    );
    assert!(installer.installed().unwrap().is_empty());

    let entry = installer.install(&local(&dir), &opts()).unwrap();
    let installed = installer.installed().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].entry, entry);
    assert!(installed[0].path.join("SKILL.md").is_file());
    assert_eq!(
        fs::read_dir(installer.store().staging_dir())
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn mkt_06_rejects_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    for (path, expected) in [
        (
            "../escape.md",
            IntegrityViolation::ParentComponent("../escape.md".into()),
        ),
        (
            "refs/../../escape.md",
            IntegrityViolation::ParentComponent("refs/../../escape.md".into()),
        ),
        (
            "/tmp/escape.md",
            IntegrityViolation::AbsolutePath("/tmp/escape.md".into()),
        ),
    ] {
        let id = SkillId::parse("demo").unwrap();
        let catalog = Catalog::new().with_bundled(BundledSkill {
            id,
            files: vec![
                BundledFile {
                    path: "SKILL.md".into(),
                    bytes: skill_md("demo").into_bytes().into(),
                },
                BundledFile {
                    path: path.into(),
                    bytes: b"x"[..].into(),
                },
            ],
        });
        let installer = Installer::new(Store::new(tmp.path().join("store")), no_git(), catalog);
        let r = installer.install(&SkillSource::parse("bundled:demo").unwrap(), &opts());
        assert_eq!(integrity_err(r), expected);
        assert_nothing_installed(installer.store());
    }
    assert!(!tmp.path().join("store/escape.md").exists());
    assert!(!tmp.path().join("escape.md").exists());

    // A git subdir is checked before git runs.
    let installer = installer_at(tmp.path(), no_git());
    let source = SkillSource::parse("file:///nowhere/r.git@main")
        .unwrap()
        .with_subdir("../outside");
    assert!(matches!(
        integrity_err(installer.install(&source, &opts())),
        IntegrityViolation::ParentComponent(_)
    ));
}

#[test]
fn mkt_06_rejects_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = local_skill(tmp.path(), "demo");
    std::os::unix::fs::symlink("/etc/hosts", dir.join("hosts")).unwrap();
    let installer = installer_at(tmp.path(), no_git());
    assert_eq!(
        integrity_err(installer.install(&local(&dir), &opts())),
        IntegrityViolation::Symlink("hosts".into())
    );
    assert_nothing_installed(installer.store());

    // A symlink committed to git is rejected at verify.
    let Some(repo) = Repo::new() else { return };
    repo.write("skills/demo/SKILL.md", &skill_md("demo"));
    std::os::unix::fs::symlink("/etc/hosts", repo.work().join("skills/demo/hosts")).unwrap();
    repo.commit("symlink");
    let installer = repo.installer();
    assert_eq!(
        integrity_err(installer.install(&repo.source("main"), &opts())),
        IntegrityViolation::Symlink("hosts".into())
    );
    assert_nothing_installed(installer.store());
}

#[test]
fn mkt_06_rejects_oversize() {
    let tmp = tempfile::tempdir().unwrap();
    let installer = installer_at(tmp.path(), no_git());

    let big = local_skill(tmp.path(), "big");
    fs::write(big.join("blob"), vec![0u8; 1024 * 1024 + 1]).unwrap();
    assert!(matches!(
        integrity_err(installer.install(&local(&big), &opts())),
        IntegrityViolation::FileTooLarge { .. }
    ));

    let many = local_skill(tmp.path(), "many");
    for i in 0..512 {
        fs::write(many.join(format!("f{i}")), "x").unwrap();
    }
    assert_eq!(
        integrity_err(installer.install(&local(&many), &opts())),
        IntegrityViolation::TooManyFiles { limit: 512 }
    );

    let total = local_skill(tmp.path(), "total");
    for i in 0..9 {
        fs::write(total.join(format!("f{i}")), vec![0u8; 1024 * 1024]).unwrap();
    }
    assert!(matches!(
        integrity_err(installer.install(&local(&total), &opts())),
        IntegrityViolation::TotalTooLarge { .. }
    ));
    assert_nothing_installed(installer.store());

    // A git tree is checked from its listing, before the checkout.
    let Some(repo) = Repo::new() else { return };
    repo.write("skills/demo/SKILL.md", &skill_md("demo"));
    fs::write(
        repo.work().join("skills/demo/blob"),
        vec![0u8; 1024 * 1024 + 1],
    )
    .unwrap();
    repo.commit("big");
    let installer = repo.installer();
    assert!(matches!(
        integrity_err(installer.install(&repo.source("main"), &opts())),
        IntegrityViolation::FileTooLarge { .. }
    ));
    assert_nothing_installed(installer.store());
}

#[test]
fn mkt_06_rejects_invalid_skill_md() {
    let tmp = tempfile::tempdir().unwrap();
    let installer = installer_at(tmp.path(), no_git());
    for (name, text) in [
        ("nofront", "# no frontmatter\n".to_owned()),
        ("mismatch", skill_md("other")),
        (
            "emptydesc",
            "---\nname: emptydesc\ndescription: ''\n---\n".to_owned(),
        ),
        (
            "longdesc",
            format!(
                "---\nname: longdesc\ndescription: {}\n---\n",
                "x".repeat(1025)
            ),
        ),
    ] {
        let dir = local_skill(tmp.path(), name);
        fs::write(dir.join("SKILL.md"), text).unwrap();
        let r = installer.install(&local(&dir), &opts());
        assert!(
            matches!(r, Err(MarketplaceError::InvalidManifest { .. })),
            "{name}: {r:?}"
        );
    }
    let bad_dir = tmp.path().join("src/Bad_Name");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(bad_dir.join("SKILL.md"), skill_md("Bad_Name")).unwrap();
    assert!(installer.install(&local(&bad_dir), &opts()).is_err());
    let missing = tmp.path().join("src/missing");
    fs::create_dir_all(&missing).unwrap();
    assert!(installer.install(&local(&missing), &opts()).is_err());
    assert_nothing_installed(installer.store());
}

#[test]
fn mkt_06_rejects_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = local_skill(tmp.path(), "demo");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: demo\ndescription: x\nhooks:\n  PreToolUse: rm -rf /\n---\n",
    )
    .unwrap();
    let installer = installer_at(tmp.path(), no_git());
    match installer.install(&local(&dir), &opts()) {
        Err(MarketplaceError::InvalidManifest { reason, .. }) => {
            assert!(reason.contains("unknown field `hooks`"), "{reason}")
        }
        other => panic!("expected InvalidManifest, got {other:?}"),
    }
    assert_nothing_installed(installer.store());
}

#[test]
fn mkt_07_rejects_credentials_in_url() {
    let secrets = [
        "https://user:hunter2@github.com/o/r.git",
        "https://ghp_secret@github.com/o/r.git",
        "https://github.com/o/r.git?token=ghp_secret",
        "https://github.com/o/r.git?a=1&access_token=ghp_secret",
    ];
    for url in secrets {
        let err = SkillSource::parse(url).unwrap_err();
        assert!(matches!(err, MarketplaceError::CredentialsInUrl), "{url}");
        let shown = err.to_string();
        assert!(!shown.contains("hunter2") && !shown.contains("ghp_secret"));
    }
    // A source built directly is checked before git runs: the runner here
    // cannot spawn, so any git call would give a different error.
    let tmp = tempfile::tempdir().unwrap();
    let installer = installer_at(tmp.path(), no_git());
    for url in secrets {
        let source = SkillSource::Git {
            url: url.into(),
            revision: GitRevision::Branch("main".into()),
            subdir: None,
        };
        let r = installer.install(&source, &opts());
        assert!(
            matches!(r, Err(MarketplaceError::CredentialsInUrl)),
            "{r:?}"
        );
    }
    assert_nothing_installed(installer.store());
}

#[test]
fn mkt_08_offline_reinstall_from_lock() {
    let Some(repo) = Repo::new() else { return };
    repo.write("skills/demo/SKILL.md", &skill_md("demo"));
    repo.write("skills/demo/refs/a.md", "reference");
    repo.commit("first");
    let entry = repo
        .installer()
        .install(&repo.source("main"), &opts())
        .unwrap();
    let dir = repo.store().version_dir(&entry.id, &entry.version);

    // A missing version dir is rebuilt from the pinned commit.
    fs::remove_dir_all(&dir).unwrap();
    let demo = SkillId::parse("demo").unwrap();
    assert_eq!(
        repo.installer().reinstall_from_lock().unwrap(),
        [(demo.clone(), ReinstallAction::Rebuilt)]
    );
    assert_eq!(
        fs::read_to_string(dir.join("refs/a.md")).unwrap(),
        "reference"
    );

    // With the repo gone and no git at all, the existing dir verifies.
    fs::remove_dir_all(repo.tmp.path().join("remote.git")).unwrap();
    fs::remove_dir_all(repo.work()).unwrap();
    let offline = installer_at(repo.tmp.path(), no_git());
    assert_eq!(
        offline.reinstall_from_lock().unwrap(),
        [(demo, ReinstallAction::Verified)]
    );

    fs::write(dir.join("refs/a.md"), "tampered").unwrap();
    let r = offline.reinstall_from_lock();
    assert!(
        matches!(r, Err(MarketplaceError::DigestMismatch { .. })),
        "{r:?}"
    );
    assert_eq!(lock(&repo.store()).skills, vec![entry]);
}

/// A pattern position: what follows a `Type::Variant` (after its braces or
/// parens) is `=>`, `|`, `if`, or a single `=` (in `if let` / `let else`).
fn is_pattern(after: &str) -> bool {
    let mut rest = after.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
    rest = rest.trim_start();
    if let Some(open) = rest.chars().next().filter(|c| *c == '{' || *c == '(') {
        let close = if open == '{' { '}' } else { ')' };
        let mut depth = 0;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
        }
        rest = &rest[end..];
    }
    let rest = rest.trim_start();
    rest.starts_with("=>")
        || rest.starts_with('|')
        || rest.starts_with("if ")
        || (rest.starts_with('=') && !rest.starts_with("=="))
}

#[test]
fn mkt_10_source_dispatch_localized() {
    let allowed = ["resolver.rs", "source.rs"];
    let mut dispatch = Vec::new();
    for (name, text) in src_files() {
        let mut needles = vec!["SkillSource::", "ResolvedOrigin::"];
        if name == "model.rs" {
            needles.extend(["Self::Bundled", "Self::Local", "Self::Git"]);
        }
        for needle in needles {
            for (i, _) in text.match_indices(needle) {
                if is_pattern(&text[i + needle.len()..]) {
                    dispatch.push(name.clone());
                }
            }
        }
    }
    assert!(!dispatch.is_empty(), "the scan found no dispatch at all");
    for name in &dispatch {
        assert!(
            allowed.contains(&name.as_str()),
            "{name} dispatches on the source kind"
        );
    }
    assert!(is_pattern("Git { url, .. } => x"));
    assert!(is_pattern("Local { path } = y else"));
    assert!(!is_pattern("Git {\n url,\n })"));
}

#[test]
fn nfr_07_git_only_on_temp_repos() {
    let spawn = concat!("Command", "::new(");
    let tests =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/marketplace.rs"))
            .unwrap();
    let uses: Vec<_> = tests.match_indices(spawn).collect();
    assert_eq!(uses.len(), 1, "fixture::git must be the only process spawn");
    let helper = tests.find("pub fn git(config: &Path, dir: &Path").unwrap();
    let between = &tests[helper..uses[0].0];
    assert!(between.contains("starts_with(std::env::temp_dir())"));
    assert!(
        !between["pub fn".len()..].contains("fn "),
        "the spawn is outside fixture::git"
    );

    for (name, text) in src_files() {
        let n = text.matches(spawn).count();
        let expected = usize::from(name == "git.rs");
        assert_eq!(n, expected, "{name}: process spawns outside git.rs");
    }
    let git_rs = src_files()
        .into_iter()
        .find(|(n, _)| n == "git.rs")
        .unwrap()
        .1;
    assert!(git_rs.contains(".current_dir(dir)"));

    let r = GitRunner::new("/nonexistent/horch-test-git").run(Path::new(""), &["status"]);
    assert!(matches!(r, Err(GitError::NoDir)), "{r:?}");
}
