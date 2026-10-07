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

/// agy has no way to be pointed at a skills directory, so horch exposes
/// none to it. A teammate that names skills is refused before any pane
/// exists, instead of launching a worker that silently lacks them.
#[test]
fn skl_06_e2e_exposure_antigravity() {
    let h = world(
        "skl06ag",
        "skl-antigravity",
        "antigravity",
        "gemini-3.8-flash",
        &["tdd", "debug"],
    );
    let out = h.run(&[
        "spawn",
        "skl-antigravity",
        "x",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("selected skills cannot load"),
        "{}",
        text(&out)
    );
    assert!(h.calls_of("antigravity").is_empty());
    assert!(!h
        .calls_of("herdr")
        .iter()
        .any(|c| c["argv"].to_string().contains("split")));
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
    // `horch spawn` checks the names against the installed catalog.
    h.set("HORCH_GIT_BIN", "/nonexistent");
    let r = launch(&h, "skl-market", || {});
    assert_eq!(strings(&r["skills"]), ["demo", "tdd"], "{r}");
    assert!(!r.to_string().contains(NOT_ACTIVATED), "{r}");
}

/// G3 (LA-5): a split pane does not inherit `$XDG_DATA_HOME`. The worker
/// pane command sets the spawner's store as `HORCH_DATA_DIR` (G8; G3 used a
/// brief field), so the worker, run without the variable, still finds a
/// skill installed only in that store.
///
/// G7: the worker's agent inherits `HORCH_DATA_DIR`, so a `horch` command
/// it runs reads the same store. The orchestrator pane command sets
/// `HORCH_DATA_DIR` too.
#[cfg(unix)]
#[test]
fn worker_reads_the_skill_store_its_spawner_read() {
    let mut h = world("g3store", "g3-market", "claude", "sonnet", &["tdd"]).with_git();
    if h.git_bin().is_none() {
        return;
    }
    let data = h.root.join("other-data");
    h.set("XDG_DATA_HOME", data.to_string_lossy());
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
        "---\nname: demo\ndescription: A demo skill for the G3 store e2e.\n---\nbody\n",
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
    assert!(data.join("horch/marketplace.lock").is_file());
    assert!(!h.home.join(".local/share/horch/marketplace.lock").exists());
    teammate(&h, "g3-market", "claude", "sonnet", &["demo", "tdd"]);
    let store = data.join("horch").to_string_lossy().into_owned();

    // The agent: a probe records its environment, then runs fake claude.
    let agent_env = h.tmp.join("agent.env");
    let claude = h.bin.join("claude");
    let probe = h.write_bin(
        "claude-probe",
        format!(
            "#!/bin/sh\n/usr/bin/env > '{}'\nexec '{}' \"$@\"\n",
            agent_env.display(),
            claude.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_CLAUDE_BIN", probe.to_string_lossy());

    let out = h.run(&[
        "spawn",
        "g3-market",
        "x",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(out.status.success(), "spawn: {}", text(&out));
    let pane = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();
    let brief_path = h.tmp.join("herdr-orchestration-w1/g3-market-1.brief.json");
    let brief: Value =
        serde_json::from_str(&std::fs::read_to_string(&brief_path).unwrap()).unwrap();
    assert!(brief.get("data_root").is_none(), "{brief}");

    // G8: the worker pane command names the store, and the worker runs
    // from that line in a shell without `XDG_DATA_HOME`.
    let set = format!("'HORCH_DATA_DIR={store}'");
    let ran = h
        .calls_of("herdr")
        .into_iter()
        .filter_map(|c| c["ran"].as_str().map(str::to_owned))
        .find(|line| line.contains("'worker'"))
        .expect("horch spawn ran a worker command");
    assert!(ran.contains(&set), "{ran}");
    let mut worker = std::process::Command::new("/bin/sh");
    h.seal(&mut worker);
    worker
        .arg("-c")
        .arg(&ran)
        .env("HERDR_PANE_ID", &pane)
        .env_remove("XDG_DATA_HOME");
    let out = worker.output().unwrap();
    assert!(out.status.success(), "worker: {ran}\n{}", text(&out));
    let r = report(&h);
    assert_eq!(strings(&r["skills"]), ["demo", "tdd"], "{r}");

    // The agent sees the store, and a `horch` command it runs lists it.
    let env = std::fs::read_to_string(&agent_env).unwrap();
    assert!(
        env.lines().any(|l| l == format!("HORCH_DATA_DIR={store}")),
        "{env}"
    );
    assert!(
        !env.lines().any(|l| l.starts_with("XDG_DATA_HOME=")),
        "{env}"
    );
    let mut list = h.horch(&["marketplace", "list", "--json"]);
    list.env_remove("XDG_DATA_HOME")
        .env("HORCH_DATA_DIR", &store);
    let out = list.output().unwrap();
    assert!(out.status.success(), "list: {}", text(&out));
    let listed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(listed["store"], store.as_str(), "{listed}");
    assert!(listed.to_string().contains("\"demo\""), "{listed}");

    // The orchestrator: `horch fleet` names the store in the pane command,
    // so `pane-launch` sees it in a shell without `XDG_DATA_HOME`.
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["fleet", "--cwd", &project]);
    assert!(out.status.success(), "fleet: {}", text(&out));
    let ran = h
        .calls_of("herdr")
        .into_iter()
        .filter_map(|c| c["ran"].as_str().map(str::to_owned))
        .find(|line| line.contains("'pane-launch'"))
        .expect("horch fleet ran a pane-launch command");
    assert!(ran.contains(&set), "{ran}");
    let pane_env = h.tmp.join("orchestrator.env");
    let pane_probe = h.write_bin(
        "pane-probe",
        format!("#!/bin/sh\n/usr/bin/env > '{}'\n", pane_env.display()).as_bytes(),
    );
    let horch = format!("'{}'", Harness::horch_bin().display());
    assert!(ran.contains(&horch), "{ran}");
    let line = ran.replacen(&horch, &format!("'{}'", pane_probe.display()), 1);
    let mut shell = std::process::Command::new("/bin/sh");
    h.seal(&mut shell);
    shell.arg("-c").arg(&line).env_remove("XDG_DATA_HOME");
    let out = shell.output().unwrap();
    assert!(out.status.success(), "{line}\n{}", text(&out));
    let env = std::fs::read_to_string(&pane_env).unwrap();
    assert!(
        env.lines().any(|l| l == format!("HORCH_DATA_DIR={store}")),
        "{env}"
    );
}

/// Write an operator skill directory `<home>/.agents/skills/<name>/`, as
/// `xcrun agent skills export` would, and overlay teammate `name` on `agent`
/// that names `tdd` in `skills:` and the operator skill in
/// `operator_skills:`, with the directory written `~/`-relative. The
/// teammate states `compact_at`, so `--check` has nothing to say about it.
fn operator_world(test: &str, name: &str, agent: &str, model: &str) -> Harness {
    let h = world(test, name, agent, model, &["tdd"]);
    let skill = h.home.join(".agents/skills/test-modernizer");
    std::fs::create_dir_all(skill.join("references")).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: test-modernizer\ndescription: Operator-local test modernizer.\n---\nbody\n",
    )
    .unwrap();
    std::fs::write(skill.join("references/notes.md"), "notes\n").unwrap();
    // `--check` insists that a Claude fleet pane denies the Agent tool.
    let deny = if agent == "claude" {
        "disallowed_tools: [Agent]\n"
    } else {
        ""
    };
    std::fs::write(
        h.home.join(format!(".config/horch/teammates/{name}.md")),
        format!(
            "---\nname: {name}\nbrief_description: Operator skills probe.\nbase: fleet-worker\n\
             agent: {agent}\nmodel: {model}\ncompact_at: 300000\nskills: [tdd]\n{deny}\
             operator_skills:\n  dir: ~/.agents/skills\n  names: [test-modernizer]\n---\nProbe.\n"
        ),
    )
    .unwrap();
    h
}

/// The operator skill reaches the Claude pane through the bundle plugin,
/// and the briefing names it as expected, with its description. The
/// operator's directory is left as it was.
#[test]
fn operator_skills_e2e_exposure_claude() {
    let h = operator_world("opskc", "op-claude", "claude", "sonnet");
    let check = h.run(&["teammates", "--check"]);
    assert!(!text(&check).contains("op-claude"), "{}", text(&check));
    let r = launch(&h, "op-claude", || {});
    assert_eq!(strings(&r["skills"]), ["tdd", "test-modernizer"], "{r}");
    dirs_in_bundle(&r);
    let prompts = serde_json::to_string(&h.calls_of("claude")).unwrap();
    assert!(
        prompts.contains("- horch:test-modernizer: Operator-local test modernizer."),
        "{prompts}"
    );
    assert!(h
        .home
        .join(".agents/skills/test-modernizer/SKILL.md")
        .is_file());
}

/// The spawn's ledger record lists the operator skill next to the catalog
/// skill, with the operator version and the digest the launch copied.
#[test]
fn operator_skills_e2e_ledger_record_lists_them() {
    let h = operator_world("opskr", "op-record", "claude", "sonnet");
    launch(&h, "op-record", || {});
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    let ledger = std::fs::read_to_string(h.state.join(format!("{slug}.json"))).unwrap();
    let records: Vec<Value> = serde_json::from_str(&ledger).unwrap();
    let r = records
        .iter()
        .find(|r| r["tier"] == "op-record")
        .unwrap_or_else(|| panic!("no op-record record in {ledger}"));
    let ids: Vec<&str> = r["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["tdd", "test-modernizer"], "{r}");
    let op = &r["skills"][1];
    assert!(
        op["version"].as_str().unwrap().starts_with("operator+"),
        "{op}"
    );
}

/// The same operator skill reaches a codex pane, linked in its private
/// `CODEX_HOME`.
#[test]
fn operator_skills_e2e_exposure_codex() {
    if cfg!(windows) {
        return;
    }
    let h = operator_world("opskx", "op-codex", "codex", "gpt-5.6-sol");
    let r = launch(&h, "op-codex", || {});
    assert_eq!(strings(&r["skills"]), ["tdd", "test-modernizer"], "{r}");
    let files = strings(&r["files"]);
    assert!(
        files.contains(&"test-modernizer/SKILL.md".to_string()),
        "{r}"
    );
    assert!(
        files.contains(&"test-modernizer/references/notes.md".to_string()),
        "{r}"
    );
}

/// `horch teammates --check` warns on an operator skill this host lacks
/// and still passes. The launch skips the skill and the briefing says so.
#[test]
fn operator_skills_e2e_check_warns_on_a_missing_name() {
    let h = operator_world("opskm", "op-missing", "claude", "sonnet");
    std::fs::remove_dir_all(h.home.join(".agents/skills/test-modernizer")).unwrap();
    let check = h.run(&["teammates", "--check"]);
    assert!(check.status.success(), "{}", text(&check));
    assert!(
        text(&check).contains(
            "warning: op-missing: operator skill test-modernizer is not installed on this host \
             (no ~/.agents/skills/test-modernizer)"
        ),
        "{}",
        text(&check)
    );
    let r = launch(&h, "op-missing", || {});
    assert_eq!(strings(&r["skills"]), ["tdd"], "{r}");
    let prompts = serde_json::to_string(&h.calls_of("claude")).unwrap();
    assert!(
        prompts.contains("Skipped: operator skill test-modernizer is not installed on this host"),
        "{prompts}"
    );
}

/// D20 item 5: `skl_06_e2e_marketplace_offline`, which runs `git init
/// --bare`, `symbolic-ref` and commit "demo", with `GIT_DIR` and
/// `GIT_WORK_TREE` aimed at a decoy repository leaves the decoy unchanged.
/// The child process, not this one, gets the variables: the other tests
/// run in parallel.
#[test]
fn git_env_cannot_reach_another_repository() {
    use horch_marketplace::git::{repo_state, GitRunner};
    let h = world("gitenv", "skl-market", "claude", "sonnet", &["tdd"]).with_git();
    let Some(git) = h.git_bin() else { return };
    let decoy = h.root.join("decoy");
    std::fs::create_dir(&decoy).unwrap();
    GitRunner::new(git)
        .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
        .run(&decoy, &["init", "--quiet", "-b", "trunk"])
        .unwrap();
    let before = repo_state(&decoy.join(".git"));
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "skl_06_e2e_marketplace_offline",
            "--test-threads=1",
        ])
        .env("GIT_DIR", decoy.join(".git"))
        .env("GIT_WORK_TREE", &decoy)
        .output()
        .unwrap();
    assert!(
        out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
        "{}",
        text(&out)
    );
    assert_eq!(repo_state(&decoy.join(".git")), before, "the decoy changed");
}

/// Spawn `name`, run `between`, then run its worker as the new pane would,
/// and return the worker's output, success or not.
fn worker_after(h: &Harness, name: &str, between: impl FnOnce()) -> Output {
    let out = h.run(&["spawn", name, "x", "--from-pane", "w1:p1", "--no-tile"]);
    assert!(out.status.success(), "spawn {name}: {}", text(&out));
    let pane = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();
    between();
    let mut worker = h.horch(&["worker", &format!("{name}-1")]);
    worker.env("HERDR_PANE_ID", &pane);
    worker.output().unwrap()
}

/// An operator skill edited between the spawn (which recorded its digest)
/// and the worker's start fails the launch: the agent never runs content
/// the ledger did not record.
#[test]
fn operator_skills_e2e_changed_since_spawn_fails_the_launch() {
    let h = operator_world("opskd", "op-drift", "claude", "sonnet");
    let skill_md = h.home.join(".agents/skills/test-modernizer/SKILL.md");
    let out = worker_after(&h, "op-drift", || {
        std::fs::write(
            &skill_md,
            "---\nname: test-modernizer\ndescription: Edited after the spawn.\n---\nnew body\n",
        )
        .unwrap();
    });
    assert!(!out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("the skills changed since"), "{t}");
    assert!(t.contains("'test-modernizer' was operator+"), "{t}");
    assert!(h.calls_of("claude").is_empty(), "the agent ran: {t}");
}

/// An operator skill deleted between the spawn and the worker's start fails
/// the launch, instead of the skip that a missing skill gets at spawn.
#[test]
fn operator_skills_e2e_deleted_since_spawn_fails_the_launch() {
    let h = operator_world("opskg", "op-gone", "claude", "sonnet");
    let skill = h.home.join(".agents/skills/test-modernizer");
    let out = worker_after(&h, "op-gone", || std::fs::remove_dir_all(&skill).unwrap());
    assert!(!out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("'test-modernizer' (operator+"), "{t}");
    assert!(t.contains("is missing now"), "{t}");
    assert!(h.calls_of("claude").is_empty(), "the agent ran: {t}");
}

/// A Claude plugin `code` (version 1.2.0, skills `review` and `lint`) at
/// `<home>/plugins/code`, and overlay teammate `name` that names `tdd` and,
/// through `plugin_dirs` and `plugin_skills`, only `code:review`.
fn plugin_world(test: &str, name: &str) -> Harness {
    let h = world(test, name, "claude", "sonnet", &["tdd"]);
    let root = h.home.join("plugins/code");
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"code","version":"1.2.0"}"#,
    )
    .unwrap();
    for skill in ["review", "lint"] {
        let dir = root.join("skills").join(skill);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill}\ndescription: Plugin {skill}.\n---\nbody\n"),
        )
        .unwrap();
    }
    std::fs::write(
        h.home.join(format!(".config/horch/teammates/{name}.md")),
        format!(
            "---\nname: {name}\nbrief_description: Plugin skills probe.\nbase: fleet-worker\n\
             agent: claude\nmodel: sonnet\nskills: [tdd]\ndisallowed_tools: [Agent]\n\
             plugin_dirs: [~/plugins/code]\nplugin_skills:\n  code: [review]\n---\nProbe.\n"
        ),
    )
    .unwrap();
    h
}

/// The record of the last spawn of teammate `tier` in the project ledger.
fn ledger_record(h: &Harness, tier: &str) -> Value {
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    let ledger = std::fs::read_to_string(h.state.join(format!("{slug}.json"))).unwrap();
    let records: Vec<Value> = serde_json::from_str(&ledger).unwrap();
    records
        .into_iter()
        .rfind(|r| r["tier"] == tier)
        .unwrap_or_else(|| panic!("no {tier} record in {ledger}"))
}

/// The spawn's ledger record lists the named plugin skill as
/// `code:review`, with the plugin's id, its version and the digest of the
/// skill directory; the unnamed `lint` is not in it. The launch accepts
/// the record and loads only `review` from the plugin.
#[test]
fn plugin_skills_e2e_ledger_record_lists_them() {
    let h = plugin_world("plgr", "plug-record");
    let r = launch(&h, "plug-record", || {});
    assert_eq!(strings(&r["skills"]), ["review", "tdd"], "{r}");
    let record = ledger_record(&h, "plug-record");
    let ids: Vec<&str> = record["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["code:review", "tdd"], "{record}");
    let review = &record["skills"][0];
    assert_eq!(review["source"], "plugin:code@inline", "{review}");
    assert_eq!(review["policy"], "explicit", "{review}");
    let digest =
        horch_marketplace::integrity::tree_digest(&h.home.join("plugins/code/skills/review"))
            .unwrap();
    assert_eq!(review["digest"], digest.as_str(), "{review}");
    assert!(
        review["version"].as_str().unwrap().starts_with("1.2.0+"),
        "{review}"
    );
}

/// A plugin skill edited between the spawn and the worker's start fails
/// the launch and names the skill; the agent never runs.
#[test]
fn plugin_skills_e2e_changed_since_spawn_fails_the_launch() {
    let h = plugin_world("plgd", "plug-drift");
    let skill_md = h.home.join("plugins/code/skills/review/SKILL.md");
    let out = worker_after(&h, "plug-drift", || {
        std::fs::write(
            &skill_md,
            "---\nname: review\ndescription: Edited after the spawn.\n---\nnew body\n",
        )
        .unwrap();
    });
    assert!(!out.status.success(), "{}", text(&out));
    let t = text(&out);
    assert!(t.contains("the skills changed since"), "{t}");
    assert!(t.contains("'code:review' was 1.2.0+"), "{t}");
    assert!(h.calls_of("claude").is_empty(), "the agent ran: {t}");
}
