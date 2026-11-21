#![cfg(unix)]

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::worker_profiles::application::conformance::read_conformance_json;
use horch_core::fleetd::worker_profiles::application::health::unhealthy_profiles;
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Verdict, CHECKS, FIRST_EVENT_LIMIT,
};
use horch_core::harness::HarnessKind;
use horch_core::roster::Roster;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, warm, Harness, DEADLINE};
use rusqlite::Connection;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const EXEC_CLIS: [(&str, &str, &str, HarnessKind); 3] = [
    (
        "claude",
        "fake-claude",
        "2.1.284 (Claude Code)",
        HarnessKind::Claude,
    ),
    (
        "codex",
        "fake-codex",
        "codex-cli 0.167.0",
        HarnessKind::Codex,
    ),
    ("agy", "fake-antigravity", "1.1.3", HarnessKind::Antigravity),
];

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn quoted(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('"')
        .split('"')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn fixture_path(fixture: &Path, url: &str) -> PathBuf {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let mut path = fixture.join(rest);
    if rest.ends_with('/') {
        path = path.join("index.html");
    }
    path
}

fn docs_fixture(h: &Harness, fixture: &Path) {
    let herdr = repo_root().join("crates/agent-docs-sync/tests/fixtures/herdr");
    let file = repo_root().join("crates/agent-docs-sync/docs-sources.toml");
    let mut sources: Vec<BTreeSet<(String, String)>> = Vec::new();
    for line in read(&file).lines() {
        let line = line.trim();
        if line.starts_with('[') {
            sources.push(BTreeSet::new());
        } else if let (Some(s), Some((k, v))) = (sources.last_mut(), line.split_once('=')) {
            s.insert((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    assert!(sources.len() >= 4, "the docs sources of {}", file.display());
    for source in sources {
        let key = |name: &str| {
            source
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        let url = quoted(&key("url").unwrap_or_default());
        if key("kind").is_some_and(|k| quoted(&k) == "sitemap") {
            put(
                &fixture_path(fixture, &url),
                &read(&herdr.join("sitemap.xml")),
            );
            for (page, file) in [
                ("https://herdr.dev/docs/", "index.html"),
                ("https://herdr.dev/docs/install/", "install.html"),
            ] {
                put(&fixture_path(fixture, page), &read(&herdr.join(file)));
            }
        } else {
            let base = url.trim_end_matches("llms.txt");
            let suffix = quoted(&key("md_suffix").unwrap_or_default());
            let include = key("include").unwrap_or_default();
            let include = include.trim_start_matches('[').trim_end_matches(']');
            let mut index = String::from("# docs\n\n");
            for entry in include.split(',').map(quoted) {
                let page = format!("{base}{entry}{suffix}");
                index.push_str(&format!("- [{entry}]({page}): {entry}.\n"));
                put(
                    &fixture_path(fixture, &page),
                    &format!("# {entry}\n\nA page.\n"),
                );
            }
            put(&fixture_path(fixture, &url), &index);
        }
        if let Some(git) = key("also_git") {
            let repo = fixture_path(fixture, &quoted(&git));
            put(&repo.join("docs/guide.md"), "# Guide\n");
            for args in [
                &["init", "-q", "-b", "main"][..],
                &["add", "."],
                &["commit", "-q", "--no-gpg-sign", "-m", "docs"],
            ] {
                let o = h.git_cmd(args).current_dir(&repo).output().unwrap();
                assert!(o.status.success(), "git {args:?}: {}", text(&o));
            }
        }
    }
}

fn install(dir: &Path, cli: &str, fake: &str, version: &str) {
    let built = horch_e2e::bin_dir().join(fake);
    let features = if cli == "codex" {
        "case \"$1\" in features) echo 'network_proxy  experimental  true'; exit 0 ;; esac\n"
    } else {
        ""
    };
    let script = format!(
        "#!/bin/sh\n{features}HORCH_FAKE_SCENARIO='conformance,version:{version}' exec '{}' \"$@\"\n",
        built.display()
    );
    let path = dir.join(cli);
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let warmed = warm(std::process::Command::new(&path).arg("--version")).unwrap();
    assert!(warmed.status.success(), "{}", text(&warmed));
}

fn fleetd(name: &str) -> Harness {
    let h = world(name);
    h.serve_fleetd();
    h
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    let docs = h.root.join("docs-fixture");
    docs_fixture(&h, &docs);
    h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
    let git = h.git_bin().expect("the git of with_git").to_path_buf();
    std::os::unix::fs::symlink(git, h.bin.join("git")).unwrap();
    for (cli, fake, version, _) in EXEC_CLIS {
        install(&h.bin, cli, fake, version);
    }
    seed_run(&h);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

fn seed_run(h: &Harness) {
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            workspace_id: Some("w1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let seat = workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            pane_id: Some("w1:p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    runs::set_orchestrator(conn, &run.run_id, &seat).unwrap();
}

fn ledger(h: &Harness) -> Connection {
    let conn = Connection::open(h.ledger_db()).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    conn
}

fn ended(h: &Harness, kind: &str, cli: &str) -> Option<String> {
    ledger(h)
        .query_row(
            "SELECT state FROM jobs WHERE kind = ?1 AND subject LIKE ?2
             AND state IN ('passed', 'failed', 'abandoned') ORDER BY id LIMIT 1",
            [kind, &format!("{cli} %")],
            |r| r.get(0),
        )
        .ok()
}

fn failed_checks(h: &Harness, cli: &str) -> Vec<String> {
    let docs = h.fleet_root().join("docs").join(cli);
    let mut failed = Vec::new();
    for dir in std::fs::read_dir(docs).into_iter().flatten() {
        let path = dir.unwrap().path().join("conformance.json");
        let Ok(report) = read_conformance_json(&read(&path)) else {
            continue;
        };
        failed.extend(
            report
                .checks
                .iter()
                .filter(|r| r.verdict == Verdict::Fail)
                .map(|r| format!("{cli}: {}: {}", r.check.name(), r.evidence)),
        );
    }
    failed
}

fn diagnostics(h: &Harness) -> String {
    let conn = ledger(h);
    let mut stmt = conn
        .prepare(
            "SELECT id || ' ' || kind || ' ' || subject || ' run ' || run || ' '
             || state || ' ' || COALESCE(result_json, '') FROM jobs ORDER BY id",
        )
        .unwrap();
    let jobs: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    format!(
        "jobs:\n{}\nfleetd stderr:\n{}\nlaunch log:\n{}",
        jobs.join("\n"),
        read(&h.root.join("fleetd.stderr")),
        read(&h.fleet_root().join("run/launch.log")),
    )
}

fn profiles_of(agent: HarnessKind) -> BTreeSet<String> {
    let roster = Roster::builtin().unwrap();
    roster
        .names()
        .into_iter()
        .filter(|n| roster.get(n).is_some_and(|t| t.agent == agent))
        .map(str::to_owned)
        .collect()
}

const AGY_FAILS: [&str; 3] = ["Private home honoured", "Exact MCP set", "Sandbox boundary"];

#[test]
fn conformance_passes_for_claude_and_codex_and_agy_fails_only_by_design() {
    let h = fleetd("p7-conformance-binary");
    let mut wrong = Vec::new();
    for (cli, _, _, _) in EXEC_CLIS {
        let state = wait_for_or(
            &format!("an ended conformance run of {cli}"),
            || ended(&h, "conformance", cli),
            || diagnostics(&h),
        );
        let failed = failed_checks(&h, cli);
        let (want_state, want_failed): (&str, Vec<String>) = match cli {
            "agy" => ("failed", AGY_FAILS.map(|c| format!("{cli}: {c}")).into()),
            _ => ("passed", Vec::new()),
        };
        let names: Vec<String> = failed
            .iter()
            .map(|f| f.split(": ").take(2).collect::<Vec<_>>().join(": "))
            .collect();
        if state != want_state || names != want_failed {
            wrong.push(format!(
                "{cli}: the run is {state}; failed checks: {failed:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the conformance suite gave {} wrong runs:\n{}\n{}",
        wrong.len(),
        wrong.join("\n"),
        diagnostics(&h)
    );
    let unhealthy = || -> BTreeSet<String> {
        unhealthy_profiles(&ledger(&h))
            .unwrap()
            .into_keys()
            .collect()
    };
    let agy = profiles_of(HarnessKind::Antigravity);
    wait_for_or(
        "the unhealthy marks of the agy profiles",
        || agy.is_subset(&unhealthy()).then_some(()),
        || diagnostics(&h),
    );
    for (cli, _, _, agent) in &EXEC_CLIS[..2] {
        let profiles = profiles_of(*agent);
        let sick: Vec<_> = unhealthy().intersection(&profiles).cloned().collect();
        assert!(sick.is_empty(), "unhealthy profiles of {cli}: {sick:?}");
    }
}

fn count(h: &Harness, sql: &str) -> i64 {
    ledger(h).query_row(sql, [], |r| r.get(0)).unwrap()
}

fn each_conformance_acted_on(h: &Harness) -> bool {
    let acted = count(
        h,
        "SELECT COUNT(DISTINCT substr(j.subject, 1, instr(j.subject, ' ') - 1)) FROM jobs j
         JOIN events e ON e.entity = 'job' AND e.entity_id = CAST(j.id AS TEXT)
         WHERE j.kind = 'conformance' AND e.cause = 'conformance result acted on'",
    );
    acted == EXEC_CLIS.len() as i64
}

fn operator_boundary_verdicts(h: &Harness, cli: &str) -> Vec<Verdict> {
    let docs = h.fleet_root().join("docs").join(cli);
    let mut verdicts = Vec::new();
    for dir in std::fs::read_dir(docs).into_iter().flatten() {
        let path = dir.unwrap().path().join("conformance.json");
        let Ok(report) = read_conformance_json(&read(&path)) else {
            continue;
        };
        verdicts.extend(
            report
                .checks
                .iter()
                .filter(|r| r.check.name() == "Operator boundary")
                .map(|r| r.verdict),
        );
    }
    verdicts
}

fn ended_again(h: &Harness, kind: &str, cli: &str) -> Option<String> {
    ledger(h)
        .query_row(
            "SELECT state FROM jobs WHERE kind = ?1 AND subject LIKE ?2 AND run >= 2
             AND state IN ('passed', 'failed', 'abandoned') ORDER BY id LIMIT 1",
            [kind, &format!("{cli} %")],
            |r| r.get(0),
        )
        .ok()
}

fn until_suite_ends<T>(
    what: &str,
    mut f: impl FnMut() -> Option<T>,
    context: impl Fn() -> String,
) -> T {
    let launches = u32::try_from(CHECKS.len()).unwrap();
    let wait = DEADLINE + FIRST_EVENT_LIMIT * launches;
    let end = std::time::Instant::now() + wait;
    while std::time::Instant::now() < end {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!(
        "timed out after {} s waiting for {what}\n{}",
        wait.as_secs(),
        context()
    );
}

#[test]
fn passes_with_no_fleetd_mark_no_conformant_profile_and_add_no_drift_feature() {
    let h = world("p7-conformance-no-fleetd");
    let features = count(&h, "SELECT COUNT(*) FROM features");
    let mut passes = Vec::new();
    until_suite_ends(
        "a pass that acted on a conformance run of each exec CLI",
        || {
            let o = h.run(&["ledger", "watch", "--once", "--force", "--json"]);
            assert!(o.status.success(), "horch ledger watch: {}", text(&o));
            passes.push(text(&o));
            each_conformance_acted_on(&h).then_some(())
        },
        || diagnostics(&h),
    );
    let unhealthy: BTreeSet<String> = unhealthy_profiles(&ledger(&h))
        .unwrap()
        .into_keys()
        .collect();
    for (cli, _, _, agent) in &EXEC_CLIS[..2] {
        let sick: Vec<_> = unhealthy
            .intersection(&profiles_of(*agent))
            .cloned()
            .collect();
        assert!(
            sick.is_empty(),
            "unhealthy profiles of {cli}: {sick:?}\nfailed checks: {:?}\n{}",
            failed_checks(&h, cli),
            diagnostics(&h)
        );
    }
    for (cli, _, _, _) in &EXEC_CLIS[..2] {
        assert_eq!(
            operator_boundary_verdicts(&h, cli),
            [Verdict::NotProved],
            "{cli}\n{}",
            diagnostics(&h)
        );
    }
    let drift = count(
        &h,
        "SELECT COUNT(*) FROM needs_you WHERE kind = 'adapter-conformance-failed'",
    );
    let new_features = count(&h, "SELECT COUNT(*) FROM features") - features;
    assert!(
        drift == 0 && new_features == 0,
        "{drift} drift items and {new_features} new features after {} passes\n{}",
        passes.len(),
        diagnostics(&h)
    );
    let agy_items = count(
        &h,
        "SELECT COUNT(*) FROM needs_you WHERE key LIKE '%agy%' OR entity_id LIKE '%agy%'
         OR text LIKE '%agy%'",
    );
    assert_eq!(agy_items, 0, "needs-you items of agy\n{}", diagnostics(&h));

    h.serve_fleetd();
    for (cli, _, _, _) in &EXEC_CLIS[..2] {
        let state = until_suite_ends(
            &format!("a second ended conformance run of {cli}"),
            || ended_again(&h, "conformance", cli),
            || diagnostics(&h),
        );
        assert_eq!(
            (state.as_str(), failed_checks(&h, cli)),
            ("passed", Vec::new()),
            "{cli}\n{}",
            diagnostics(&h)
        );
        assert_eq!(
            operator_boundary_verdicts(&h, cli),
            [Verdict::Pass],
            "{cli}"
        );
    }
}

const GATE: &str = "scripts/live/p7-real-conformance.sh";

fn fake_bin(h: &Harness, name: &str, clis: &[&str]) -> PathBuf {
    let dir = h.root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for (cli, fake, version, _) in EXEC_CLIS.iter().filter(|c| clis.contains(&c.0)) {
        install(&dir, cli, fake, version);
    }
    let git = h.git_bin().expect("the git of with_git").to_path_buf();
    std::os::unix::fs::symlink(git, dir.join("git")).unwrap();
    assert!(
        h.has_sqlite3(),
        "the gate script reads the ledger with sqlite3"
    );
    let sqlite3 = std::fs::canonicalize(h.bin.join("sqlite3")).unwrap();
    std::os::unix::fs::symlink(sqlite3, dir.join("sqlite3")).unwrap();
    dir
}

fn printed(out: &str, key: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("no line {key:?} in:\n{out}"))
        .trim()
        .to_string()
}

fn run_gate(h: &Harness, bin: &Path, args: &[&str]) -> (Output, String) {
    let mut cmd = std::process::Command::new(repo_root().join(GATE));
    cmd.env_clear()
        .current_dir(&h.project)
        .args(args)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &h.home)
        .env("TMPDIR", &h.tmp)
        .env("HORCH_BIN", Harness::horch_bin())
        .env("HORCH_TEST_DOCS_FIXTURE", h.root.join("docs-fixture"))
        .env("P7_CONFORMANCE_TIMEOUT_S", "300");
    for key in [
        "FLEET_ROOT",
        "HORCH_LEDGER_DB",
        "HORCH_STATE_DIR",
        "HORCH_DATA_DIR",
    ] {
        cmd.env(key, h.root.join("outer").join(key));
    }
    let o = cmd.output().unwrap();
    let out = text(&o);
    let stdout = String::from_utf8_lossy(&o.stdout);
    let fleet = PathBuf::from(printed(&stdout, "fleet root"));
    assert!(!fleet.starts_with(h.root.join("outer")), "{out}");
    assert!(!fleet.exists(), "the temp fleet stays: {out}");
    let pid: u32 = printed(&stdout, "fleetd pid").parse().unwrap();
    assert!(!horch_core::procid::alive(pid, None), "fleetd {pid} runs");
    assert!(
        !h.root.join("outer").exists(),
        "the gate used the outer fleet"
    );
    (o, out)
}

#[test]
fn real_conformance_gate_script_runs_isolated_and_retains_reports() {
    let h = Harness::new("p7-gate").with_git();
    docs_fixture(&h, &h.root.join("docs-fixture"));
    let evidence = h.root.join("evidence");
    let out_arg = evidence.to_string_lossy().into_owned();

    let bin = fake_bin(&h, "fakes", &["claude", "codex"]);
    let (o, out) = run_gate(&h, &bin, &["--out", &out_arg]);
    assert!(o.status.success(), "{out}");
    for (cli, _, version, _) in &EXEC_CLIS[..2] {
        let path = evidence.join(cli).join(version).join("conformance.json");
        let report = read_conformance_json(&read(&path))
            .unwrap_or_else(|e| panic!("{}: {e}\n{out}", path.display()));
        assert!(!report.checks.is_empty(), "{out}");
        let names: Vec<&str> = report.checks.iter().map(|r| r.check.name()).collect();
        assert!(names.contains(&"Operator boundary"), "{cli}: {names:?}");
        let failed: Vec<_> = report
            .checks
            .iter()
            .filter(|r| r.verdict != Verdict::Pass)
            .map(|r| r.check.name())
            .collect();
        assert!(failed.is_empty(), "{cli}: {failed:?}");
        assert!(out.contains(&path.display().to_string()), "{out}");
    }

    let bin = fake_bin(&h, "fakes-agy", &["agy"]);
    let (o, out) = run_gate(&h, &bin, &["--out", &out_arg, "--clis", "agy"]);
    assert!(!o.status.success(), "{out}");
    let stderr = String::from_utf8_lossy(&o.stderr);
    let path = evidence
        .join("agy")
        .join(EXEC_CLIS[2].2)
        .join("conformance.json");
    let report = read_conformance_json(&read(&path))
        .unwrap_or_else(|e| panic!("{}: {e}\n{out}", path.display()));
    let named: Vec<String> = report
        .checks
        .iter()
        .filter(|r| r.verdict != Verdict::Pass)
        .map(|r| format!("{:<28} {}", r.check.name(), r.verdict.word()))
        .collect();
    for check in AGY_FAILS {
        assert!(named.iter().any(|n| n.starts_with(check)), "{named:?}");
    }
    for line in &named {
        assert!(
            stderr.contains(line.as_str()),
            "the gate does not name {line:?}: {out}"
        );
    }
}
