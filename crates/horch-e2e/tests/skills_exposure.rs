//! End-to-end: each harness sees exactly the skills its teammate's activation
//! plan activates, through its own native mechanism (SKL-06). A teammate
//! names `tdd` and `debug` (set A); `execute`, a catalog skill it does not
//! activate, is set B. The fakes' `inspect_skills` scenario records what the
//! launch exposed while the agent runs.

use std::path::Path;
use std::process::Output;

use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

/// Set A, sorted.
const ACTIVATED: [&str; 2] = ["debug", "tdd"];
/// Set B: in the catalog, not activated.
const NOT_ACTIVATED: &str = "execute";

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Write overlay teammate `name` on `agent`, naming `skills`.
fn teammate(h: &Harness, name: &str, agent: &str, model: &str, skills: &[&str]) {
    let dir = h.home.join(".config/horch/teammates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!(
            "---\nname: {name}\nbrief_description: SKL-06 exposure probe.\nbase: fleet-worker\n\
             agent: {agent}\nmodel: {model}\nskills: [{}]\n---\nProbe.\n",
            skills.join(", ")
        ),
    )
    .unwrap();
}

/// A harness with workspace `w1` (root pane `w1:p1`), the `inspect_skills`
/// scenario, and an overlay teammate `name` on `agent` that names `skills`.
fn world(test: &str, name: &str, agent: &str, model: &str, skills: &[&str]) -> Harness {
    let mut h = Harness::new(test);
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "inspect_skills");
    teammate(&h, name, agent, model, skills);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let created = create
        .args(["workspace", "create", "--label", test])
        .output()
        .unwrap();
    assert!(created.status.success(), "{}", text(&created));
    h
}

/// Spawn `name`, then run its worker as the new pane would, and return what
/// the agent's fake saw. `before_worker` runs between the two.
fn launch(h: &Harness, name: &str, before_worker: impl FnOnce()) -> Value {
    let out = h.run(&["spawn", name, "x", "--from-pane", "w1:p1", "--no-tile"]);
    assert!(out.status.success(), "spawn {name}: {}", text(&out));
    let pane = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();
    before_worker();
    let mut worker = h.horch(&["worker", &format!("{name}-1")]);
    worker.env("HERDR_PANE_ID", &pane);
    let out = worker.output().unwrap();
    assert!(out.status.success(), "worker {name}: {}", text(&out));
    // fake-herdr records every mutating call (the spawn's split) as a
    // violation; the agents' fakes record a forbidden environment.
    let violations: Vec<String> = h
        .violations()
        .into_iter()
        .filter(|v| !v.starts_with("herdr: mutating call"))
        .collect();
    assert!(violations.is_empty(), "{violations:?}");
    report(h)
}

fn report(h: &Harness) -> Value {
    let mut path = h.log.clone().into_os_string();
    path.push(".skills.json");
    let text = std::fs::read_to_string(Path::new(&path))
        .unwrap_or_else(|e| panic!("no skills report at {path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// The fake saw exactly set A, and set B nowhere.
fn sees_exactly_a(r: &Value) {
    assert_eq!(strings(&r["skills"]), ACTIVATED, "{r}");
    assert!(!r.to_string().contains(NOT_ACTIVATED), "{r}");
}

/// Every dir the fake was pointed at is in this launch's skill bundle.
fn dirs_in_bundle(r: &Value) {
    let dirs = strings(&r["dirs"]);
    assert!(!dirs.is_empty(), "{r}");
    for dir in dirs {
        assert!(dir.contains("/skill-bundles/"), "not a bundle dir: {r}");
    }
}

#[test]
fn skl_06_e2e_exposure_claude() {
    let h = world(
        "skl06c",
        "skl-claude",
        "claude",
        "sonnet",
        &["tdd", "debug"],
    );
    let r = launch(&h, "skl-claude", || {});
    sees_exactly_a(&r);
    dirs_in_bundle(&r);
    assert_eq!(strings(&r["flags"]).len(), 2, "1 --plugin-dir: {r}");
    assert_eq!(r["plugins"], serde_json::json!(["horch"]), "{r}");
    // The bundle dir is named for the execution: the brief's record id.
    let path = h.tmp.join("herdr-orchestration-w1/skl-claude-1.brief.json");
    let brief: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let record = brief["record_id"].as_str().unwrap();
    assert!(
        strings(&r["flags"])[1].ends_with(&format!("/skill-bundles/{record}")),
        "{r}"
    );
    let settings: Value = serde_json::from_str(r["settings"].as_str().unwrap()).unwrap();
    assert_eq!(settings["disableBundledSkills"], true, "{settings}");
}

#[test]
fn skl_06_e2e_exposure_codex() {
    if cfg!(windows) {
        return;
    }
    let h = world(
        "skl06x",
        "skl-codex",
        "codex",
        "gpt-5.6-sol",
        &["tdd", "debug"],
    );
    let r = launch(&h, "skl-codex", || {});
    sees_exactly_a(&r);
    let home = r["env"]["CODEX_HOME"].as_str().unwrap();
    assert_ne!(
        Path::new(home),
        h.home.join(".codex"),
        "a private CODEX_HOME: {r}"
    );
    let files = strings(&r["files"]);
    for id in ACTIVATED {
        assert!(files.contains(&format!("{id}/SKILL.md")), "{r}");
    }
}

#[test]
fn skl_06_e2e_exposure_opencode() {
    let h = world(
        "skl06o",
        "skl-opencode",
        "opencode",
        "opencode/big-pickle",
        &["tdd", "debug"],
    );
    let r = launch(&h, "skl-opencode", || {});
    sees_exactly_a(&r);
    dirs_in_bundle(&r);
    assert_eq!(strings(&r["dirs"]).len(), 1, "1 skills.paths entry: {r}");
}

#[test]
fn skl_06_e2e_exposure_pi() {
    let h = world(
        "skl06p",
        "skl-pi",
        "pi",
        "ollama/qwen3.8",
        &["tdd", "debug"],
    );
    let r = launch(&h, "skl-pi", || {});
    sees_exactly_a(&r);
    dirs_in_bundle(&r);
    assert_eq!(strings(&r["flags"]).len(), 2, "1 --skill: {r}");
}

#[test]
fn skl_06_e2e_exposure_prime() {
    let h = world(
        "skl06r",
        "skl-prime",
        "prime",
        "anthropic/claude-opus-5-5",
        &["tdd", "debug"],
    );
    let r = launch(&h, "skl-prime", || {});
    sees_exactly_a(&r);
    dirs_in_bundle(&r);
    assert_eq!(strings(&r["flags"]).len(), 2, "1 --skill: {r}");
}

fn git(h: &Harness, dir: &Path, args: &[&str]) {
    let mut full = vec!["-C", dir.to_str().unwrap()];
    full.extend_from_slice(args);
    let out = h.git_cmd(&full).output().expect("running git");
    assert!(out.status.success(), "git {args:?} failed: {}", text(&out));
}

/// A skill installed from a local git repository launches with git gone:
/// the roster check accepts its id, and the launch copies it from the store.
#[test]
fn skl_06_e2e_marketplace_offline() {
    let mut h = world("skl06m", "skl-market", "claude", "sonnet", &["tdd"]).with_git();
    if h.git_bin().is_none() {
        return;
    }
    let bare = h.root.join("remote.git");
    let work = h.root.join("upstream");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::create_dir_all(work.join("skills/demo")).unwrap();
    git(&h, &bare, &["init", "--quiet", "--bare"]);
    git(&h, &bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&h, &work, &["init", "--quiet"]);
    git(&h, &work, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(
        work.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: A demo skill for the SKL-06 e2e.\n---\nbody\n",
    )
    .unwrap();
    git(&h, &work, &["add", "-A"]);
    git(
        &h,
        &work,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "demo"],
    );
    git(
        &h,
        &work,
        &["push", "--quiet", bare.to_str().unwrap(), "main:main"],
    );
    let source = format!("file://{}@main", bare.display());
    let out = h.run(&["skills", "install", &source, "--path", "skills/demo"]);
    assert!(out.status.success(), "install: {}", text(&out));

    // The roster check accepts the installed id.
    teammate(&h, "skl-market", "claude", "sonnet", &["demo", "tdd"]);
    let check = h.run(&["teammates", "--check"]);
    let said = text(&check);
    assert!(!said.contains("unknown bundled skill 'demo'"), "{said}");

    // No git from here on: a launch needs neither the network nor git.
    h.set("HORCH_GIT_BIN", "/nonexistent");
    // `horch spawn` still checks names against the compiled-in catalog
    // (`skills::ensure_supported`) until U26 switches it to
    // `ensure_supported_in`. So the spawn reads a file without `demo`, and
    // `demo` is added to the teammate the brief carries to the worker.
    teammate(&h, "skl-market", "claude", "sonnet", &["tdd"]);
    let r = launch(&h, "skl-market", || {
        let path = h.tmp.join("herdr-orchestration-w1/skl-market-1.brief.json");
        let mut brief: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        brief["resolved"]["skills"] = serde_json::json!(["demo", "tdd"]);
        std::fs::write(&path, brief.to_string()).unwrap();
    });
    assert_eq!(strings(&r["skills"]), ["demo", "tdd"], "{r}");
    assert!(!r.to_string().contains(NOT_ACTIVATED), "{r}");
}
