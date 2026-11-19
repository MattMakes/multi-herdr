//! The production conformance suite through the `horch` binary passes for
//! each exec CLI (ai_docs/plans/fleetd-build/tickets/53; design, "Docs sync
//! and adapter conformance", FR-8, SR-1, SR-8).
//!
//! A test fleetd finds the fake CLIs on its `PATH`, queues a `docs_sync`
//! run and then a `conformance` run for each, and each run is a `horch job
//! run` process. The suite launches each CLI with the reference profile of
//! the job, as fleetd launches a worker of that CLI. Each fake runs with the
//! scenario `conformance` only: it acts on the prompt of each check as a
//! real agent does.
//!
//! No test uses the network: `HORCH_TEST_DOCS_FIXTURE` makes the fetch port
//! of the docs read each page from a local folder.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::worker_profiles::application::conformance::read_conformance_json;
use horch_core::fleetd::worker_profiles::application::health::unhealthy_profiles;
use horch_core::fleetd::worker_profiles::domain::conformance::Verdict;
use horch_core::harness::HarnessKind;
use horch_core::roster::Roster;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, warm, Harness};
use rusqlite::Connection;

/// The `fleet.toml` of the test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The docs sources with an exec adapter, the fake of each, the version
/// text of the fake and the agent of its profiles.
const EXEC_CLIS: [(&str, &str, &str, HarnessKind); 3] = [
    ("claude", "fake-claude", "2.1.284 (Claude Code)", HarnessKind::Claude),
    // The lowest Codex on which the operator boundary is proven on macOS.
    ("codex", "fake-codex", "codex-cli 0.167.0", HarnessKind::Codex),
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

/// The text of a string value `"..."` of 1 line of `docs-sources.toml`.
fn quoted(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('"')
        .split('"')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The file of a URL in the fixture: its host and path, and `index.html`
/// for a path that ends with `/`.
fn fixture_path(fixture: &Path, url: &str) -> PathBuf {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let mut path = fixture.join(rest);
    if rest.ends_with('/') {
        path = path.join("index.html");
    }
    path
}

/// The pages of each docs source of `docs-sources.toml` under `fixture`:
/// the sitemap fixture of agent-docs-sync for herdr, an `llms.txt` with
/// each included page for the others, and a local repository for each
/// `also_git`. The file has 1 key on each line, so a line split is enough.
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
            put(&fixture_path(fixture, &url), &read(&herdr.join("sitemap.xml")));
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
                put(&fixture_path(fixture, &page), &format!("# {entry}\n\nA page.\n"));
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

/// The script `<bin>/<cli>` that starts `fake` with the version text
/// `version` and the scenario `conformance`.
fn install(h: &Harness, cli: &str, fake: &str, version: &str) {
    let built = horch_e2e::bin_dir().join(fake);
    // Codex answers `codex features list` with the `network_proxy` feature,
    // which denies the connect to the command channel.
    let features = if cli == "codex" {
        "case \"$1\" in features) echo 'network_proxy  experimental  true'; exit 0 ;; esac\n"
    } else {
        ""
    };
    let script = format!(
        "#!/bin/sh\n{features}HORCH_FAKE_SCENARIO='conformance,version:{version}' exec '{}' \"$@\"\n",
        built.display()
    );
    // `<bin>/<cli>` is a link to the built fake: remove the link, so the
    // write does not change the fake.
    let path = h.bin.join(cli);
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // macOS scans a new executable at its first exec: keep that scan out of
    // the waits of the test.
    let warmed = warm(std::process::Command::new(&path).arg("--version")).unwrap();
    assert!(warmed.status.success(), "{}", text(&warmed));
}

/// A test fleetd with the fake CLIs on its `PATH` and the docs fixture.
fn fleetd(name: &str) -> Harness {
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
    // The docs sync of Codex clones its `also_git` repository with the
    // `git` of the `PATH`.
    let git = h.git_bin().expect("the git of with_git").to_path_buf();
    std::os::unix::fs::symlink(git, h.bin.join("git")).unwrap();
    for (cli, fake, version, _) in EXEC_CLIS {
        install(&h, cli, fake, version);
    }
    seed_run(&h);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h.serve_fleetd();
    h
}

/// The run `r1` of the project with its idea in flight (FR-12), and the
/// seat of its orchestrator in the pane `w1:p1`.
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

/// The state of the first ended run of `kind` of `cli`, if one ended.
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

/// Each failed check of each `conformance.json` of `cli`, with its evidence.
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

/// The jobs, the output of fleetd and the launch log.
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

/// Each profile of the built-in roster whose agent is `agent`.
fn profiles_of(agent: HarnessKind) -> BTreeSet<String> {
    let roster = Roster::builtin().unwrap();
    roster
        .names()
        .into_iter()
        .filter(|n| roster.get(n).is_some_and(|t| t.agent == agent))
        .map(str::to_owned)
        .collect()
}

/// FR-8: fleetd runs the production conformance suite for each exec CLI
/// through the binary. Each run passes, no check fails, and no profile of
/// the CLI is unhealthy.
#[test]
fn conformance_passes_for_each_exec_cli_and_its_profiles_are_healthy() {
    let h = fleetd("p7-conformance-binary");
    let mut failed = Vec::new();
    for (cli, _, _, _) in EXEC_CLIS {
        let state = wait_for_or(
            &format!("an ended conformance run of {cli}"),
            || ended(&h, "conformance", cli),
            || diagnostics(&h),
        );
        if state != "passed" {
            failed.push(format!("{cli}: the run is {state}"));
        }
        failed.extend(failed_checks(&h, cli));
    }
    assert!(
        failed.is_empty(),
        "the conformance suite failed {} times:\n{}\n{}",
        failed.len(),
        failed.join("\n"),
        diagnostics(&h)
    );
    let unhealthy: BTreeSet<String> = unhealthy_profiles(&ledger(&h))
        .unwrap()
        .into_keys()
        .collect();
    for (cli, _, _, agent) in EXEC_CLIS {
        let profiles = profiles_of(agent);
        let sick: Vec<_> = unhealthy.intersection(&profiles).collect();
        assert!(sick.is_empty(), "unhealthy profiles of {cli}: {sick:?}");
    }
}
