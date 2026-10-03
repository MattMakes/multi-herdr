//! `horch skills` and `horch marketplace` (MKT-08, MKT-09): the legacy
//! listing is unchanged, the new commands print their documented JSON keys,
//! and an installed git skill materializes with no git and no network.
//!
//! Every command runs with a temp home and data root. Real git runs only on
//! temp repos, with an empty global config.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::skills::{plan_activation, MaterializedSkills, SkillCatalog};
use horch_core::teammates::{Agent, Teammate};
use serde_json::Value;

const PHASES: [&str; 4] = ["research", "plan", "implementation", "validation"];

/// A temp home, state root and data root, and a `horch` that sees only them.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    git: Option<PathBuf>,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "state", "data"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("gitconfig"), "").unwrap();
        World {
            _tmp: tmp,
            root,
            git: None,
        }
    }

    /// The real `git` by absolute path, or `None` (a skip) unless
    /// `HORCH_REQUIRE_GIT=1`.
    fn with_git(mut self) -> World {
        let found = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join(format!("git{}", std::env::consts::EXE_SUFFIX)))
                .find(|p| p.is_file() && p.is_absolute())
        });
        if found.is_none() {
            assert!(
                std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
                "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
            );
        }
        self.git = found;
        self
    }

    fn data_root(&self) -> PathBuf {
        self.root.join("data/horch")
    }

    fn command(&self, args: &[&str], git_bin: &Path) -> Command {
        let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", "/oracle/project")
            .env("HORCH_TEAMMATES_DIR", teammates)
            .env("HORCH_GIT_BIN", git_bin)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }

    fn horch_with(&self, args: &[&str], git_bin: &Path) -> String {
        let out = self.command(args, git_bin).output().unwrap();
        assert!(
            out.status.success(),
            "horch {args:?} failed: {}",
            text(&out)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn horch(&self, args: &[&str]) -> String {
        self.horch_with(args, Path::new("/nonexistent"))
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        let out = Command::new(self.git.as_ref().unwrap())
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("ANTHROPIC_API_KEY")
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Horch Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@horch.invalid")
            .env("GIT_COMMITTER_NAME", "Horch Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@horch.invalid")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed: {}", text(&out));
    }

    /// A bare repository on `main` whose root is one valid skill, `demo`.
    fn bare_skill_repo(&self) -> PathBuf {
        let bare = self.root.join("remote.git");
        let work = self.root.join("upstream");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::create_dir_all(work.join("skills/demo/references")).unwrap();
        std::fs::write(
            work.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: A demo skill for the CLI tests.\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(work.join("skills/demo/references/notes.md"), "notes\n").unwrap();
        self.git(&bare, &["init", "--quiet", "--bare"]);
        self.git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        self.git(&work, &["init", "--quiet"]);
        self.git(&work, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        self.git(&work, &["add", "-A"]);
        self.git(&work, &["commit", "--quiet", "--no-gpg-sign", "-m", "demo"]);
        self.git(
            &work,
            &["push", "--quiet", bare.to_str().unwrap(), "main:main"],
        );
        bare
    }
}

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn oracle(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oracles/skills")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort();
    k
}

#[test]
fn mkt_09_legacy_skills_flags_output_unchanged() {
    let w = World::new();
    // The 4 legacy forms, and the same forms through the `list` alias.
    for prefix in [&["skills"][..], &["skills", "list"]] {
        let run = |extra: &[&str]| {
            let mut args = prefix.to_vec();
            args.extend_from_slice(extra);
            w.horch(&args)
        };
        assert_eq!(run(&[]), oracle("skills.txt"), "{prefix:?}");
        assert_eq!(run(&["--json"]), oracle("skills-json.txt"), "{prefix:?}");
        for phase in PHASES {
            assert_eq!(
                run(&["--phase", phase]),
                oracle(&format!("skills-phase-{phase}.txt")),
                "{prefix:?} {phase}"
            );
            assert_eq!(
                run(&["--phase", phase, "--json"]),
                oracle(&format!("skills-phase-{phase}-json.txt")),
                "{prefix:?} {phase} json"
            );
        }
    }
}

#[test]
fn mkt_09_cli_list_show_json() {
    let w = World::new();
    let show: Value = serde_json::from_str(&w.horch(&["skills", "show", "tdd", "--json"])).unwrap();
    assert_eq!(
        keys(&show),
        [
            "description",
            "digest",
            "id",
            "install_path",
            "provenance",
            "source",
            "version"
        ]
    );
    assert_eq!(show["id"], "tdd");
    assert_eq!(show["source"], "bundled");
    assert!(show["version"].as_str().unwrap().starts_with("bundled+"));
    assert!(show["digest"].as_str().unwrap().starts_with("sha256:"));
    assert!(!show["description"].as_str().unwrap().is_empty());
    assert!(show["provenance"].is_object());
    assert!(show["install_path"].is_null());

    // An empty store lists no skills.
    let empty: Value = serde_json::from_str(&w.horch(&["marketplace", "list", "--json"])).unwrap();
    assert_eq!(keys(&empty), ["skills", "store"]);
    assert_eq!(empty["skills"], Value::Array(vec![]));
    assert_eq!(empty["store"], w.data_root().display().to_string());

    // A local install lists with every lock field plus path and presence.
    let src = w.root.join("src/demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: A local demo.\n---\nBody\n",
    )
    .unwrap();
    w.horch(&["skills", "install", src.to_str().unwrap()]);
    let list: Value = serde_json::from_str(&w.horch(&["marketplace", "list", "--json"])).unwrap();
    let entry = &list["skills"][0];
    assert_eq!(
        keys(entry),
        [
            "digest",
            "id",
            "installed_at",
            "path",
            "present",
            "requested_revision",
            "resolved_commit",
            "source",
            "version"
        ]
    );
    assert_eq!(entry["id"], "demo");
    assert_eq!(entry["present"], true);
    let shown: Value =
        serde_json::from_str(&w.horch(&["skills", "show", "demo", "--json"])).unwrap();
    assert_eq!(shown["description"], "A local demo.");
    assert_eq!(shown["install_path"], entry["path"]);
    assert_eq!(shown["version"], entry["version"]);

    // A SKILL.md description cannot write control bytes to the terminal.
    let loud = w.root.join("src/loud");
    std::fs::create_dir_all(&loud).unwrap();
    std::fs::write(
        loud.join("SKILL.md"),
        "---\nname: loud\ndescription: \"\\e[31mred\\e[0m\"\n---\nBody\n",
    )
    .unwrap();
    w.horch(&["skills", "install", loud.to_str().unwrap()]);
    let shown = w.horch(&["skills", "show", "loud"]);
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    assert!(shown.contains("?[31mred?[0m"), "{shown:?}");

    // A rejected source with a credential is not echoed back.
    let out = w
        .command(
            &[
                "skills",
                "install",
                "https://user:hunter2@example.invalid/r.git",
            ],
            Path::new("/nonexistent"),
        )
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!text(&out).contains("hunter2"), "{}", text(&out));
}

#[test]
fn mkt_08_runtime_needs_no_network() {
    let w = World::new().with_git();
    let Some(git) = w.git.clone() else {
        return;
    };
    let bare = w.bare_skill_repo();
    let spec = format!("file://{}@main", bare.display());
    w.horch_with(&["skills", "install", &spec, "--path", "skills/demo"], &git);

    // From here on there is no git: the store alone must serve the skill.
    let doctor = w.horch(&["skills", "doctor"]);
    assert!(doctor.contains("0 problem(s)"), "{doctor}");
    let refresh = w.horch(&["marketplace", "refresh"]);
    assert!(
        refresh.contains("1 skill(s) present and verified"),
        "{refresh}"
    );

    let catalog = SkillCatalog::installed(&w.data_root()).unwrap();
    let entry = catalog
        .lookup("demo")
        .expect("the installed skill is in the catalog");
    assert_eq!(entry.description, "A demo skill for the CLI tests.");
    let teammate = Teammate {
        name: "marketplace-user".into(),
        agent: Agent::Claude,
        skills: vec!["demo".into()],
        ..Teammate::default()
    };
    let plan = plan_activation(&teammate, None, &catalog).unwrap();
    assert_eq!(plan.activated.len(), 1);
    assert!(plan.activated[0]
        .source
        .ends_with(&format!("@{}", entry_commit(&catalog))));
    let state = w.root.join("state");
    let bundle = MaterializedSkills::materialize(&plan, &catalog, &state, "exec-1")
        .unwrap()
        .expect("one activated skill");
    let dir = bundle.skills_dir().join("demo");
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        "---\nname: demo\ndescription: A demo skill for the CLI tests.\n---\nBody\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("references/notes.md")).unwrap(),
        "notes\n"
    );
    drop(bundle);

    // A changed store file stops the launch instead of exposing it.
    let version = &catalog.lookup("demo").unwrap().version;
    let stored = w
        .data_root()
        .join("skills/demo")
        .join(&version.0)
        .join("SKILL.md");
    std::fs::write(&stored, "---\nname: demo\ndescription: changed\n---\n").unwrap();
    let err = MaterializedSkills::materialize(&plan, &catalog, &state, "exec-2").unwrap_err();
    assert!(format!("{err:#}").contains("the lock pins"), "{err:#}");
}

fn entry_commit(catalog: &SkillCatalog) -> String {
    match &catalog.lookup("demo").unwrap().source {
        horch_core::skills::CatalogSource::Marketplace {
            resolved_commit: Some(c),
            ..
        } => c.clone(),
        other => panic!("not a git marketplace entry: {other:?}"),
    }
}
