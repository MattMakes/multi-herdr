//! End-to-end: `multi-herdr-dataset fleet sync [--all-projects]`, and the
//! sync that `export` and `readiness` run first
//! (`docs/specs/fleet-dataset.md` §4.2, §6, FDS-15, FDS-17, FDS-18).
//!
//! Each test writes ledgers into a fake state root and a fixture Claude
//! transcript into the sealed home, then runs the real binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_e2e::bin_dir;
use horch_e2e::harness::{repo_root, Harness};
use serde_json::{json, Value};

const SESSION: &str = "11111111-2222-4333-8444-555555555555";

fn dataset_bin() -> PathBuf {
    let path = bin_dir().join(format!(
        "multi-herdr-dataset{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        path.is_file(),
        "{} is not built; run `cargo build --workspace --bins` first",
        path.display()
    );
    path
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `multi-herdr-dataset <args>`; its stdout, which must succeed.
fn dataset(h: &Harness, args: &[&str]) -> String {
    let mut cmd = Command::new(dataset_bin());
    h.seal(&mut cmd);
    cmd.args(args);
    let out = cmd.output().expect("running multi-herdr-dataset");
    assert!(out.status.success(), "{}", text(&out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn slug(project: &Path) -> String {
    project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect()
}

fn fleet_dir(h: &Harness, project: &Path) -> PathBuf {
    h.state
        .join("multi-herdr")
        .join(slug(project))
        .join("fleet")
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// A finished worker record of `project`; `session` gets a transcript.
fn finished(id: &str, project: &Path, session: Option<&str>) -> Value {
    json!({
        "record_id": id, "session_id": session, "agent": "claude", "tier": "sonnet",
        "model": "sonnet", "effort": "high", "role": format!("{id}-role"),
        "status": "done", "task": format!("task {id}"),
        "history": [{"at": "2026-10-07T10:00:00Z", "event": "spawned", "text": "t"},
                    {"at": "2026-10-07T10:30:00Z", "event": "done", "text": "did it"}],
        "created_at": "2026-10-07T10:00:00Z", "updated_at": "2026-10-07T10:30:00Z",
        "state": {"state": "done"}, "finished_at": "2026-10-07T10:30:00Z",
        "project": project.to_string_lossy(),
    })
}

fn working(id: &str, project: &Path) -> Value {
    let mut r = finished(id, project, None);
    r["status"] = json!("working");
    r["state"] = json!({"state": "running"});
    r["finished_at"] = Value::Null;
    r
}

fn write_ledger(h: &Harness, project: &Path, records: &[Value]) {
    std::fs::write(
        h.state.join(format!("{}.json", slug(project))),
        serde_json::to_string_pretty(records).unwrap(),
    )
    .unwrap();
}

/// The Claude usage fixture as the transcript of [`SESSION`].
fn transcript(h: &Harness) {
    let dir = h.home.join(".claude/projects/-work-alpha");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        repo_root().join("crates/horch-core/tests/fixtures/usage/claude.jsonl"),
        dir.join(format!("{SESSION}.jsonl")),
    )
    .unwrap();
}

/// FDS-15: `fleet sync` writes a row for each finished record of this
/// project, prints 1 line for it and 1 total line, and a second sync
/// writes nothing.
#[test]
fn fds_15_fleet_sync_cli() {
    let h = Harness::new("fds15-sync");
    transcript(&h);
    let p = h.project.clone();
    write_ledger(
        &h,
        &p,
        &[
            finished("r-1", &p, Some(SESSION)),
            finished("r-2", &p, None),
            working("r-3", &p),
        ],
    );
    let out = dataset(&h, &["fleet", "sync"]);
    assert_eq!(
        out,
        format!(
            "{}: written 2 (tokens 1, unpriced 1), already 0, deferred 0, skipped 1\n\
             total: written 2 (tokens 1, unpriced 1), already 0, deferred 0, skipped 1\n",
            p.display()
        )
    );
    let runs = lines(&fleet_dir(&h, &p).join("runs.jsonl"));
    let ids: Vec<&str> = runs
        .iter()
        .map(|r| r["record_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["r-1", "r-2"]);
    assert!(runs.iter().all(|r| r["written_by"] == "sync"));
    assert!(runs[0]["tokens"].is_object(), "{}", runs[0]);

    let again = dataset(&h, &["fleet", "sync"]);
    assert!(
        again.ends_with(
            "total: written 0 (tokens 0, unpriced 0), already 2, deferred 0, skipped 1\n"
        ),
        "{again}"
    );
    assert_eq!(lines(&fleet_dir(&h, &p).join("runs.jsonl")).len(), 2);
}

/// FDS-15: `--all-projects` syncs every ledger of the state root into its
/// own project's dataset directory.
#[test]
fn fds_15_fleet_sync_all_projects() {
    let h = Harness::new("fds15-all");
    let a = h.project.clone();
    let b = h.root.join("work/beta");
    std::fs::create_dir_all(&b).unwrap();
    write_ledger(&h, &a, &[finished("a-1", &a, None)]);
    write_ledger(
        &h,
        &b,
        &[
            finished("b-1", &b, None),
            finished("b-2", &b, None),
            working("b-3", &b),
        ],
    );
    std::fs::write(h.state.join("policy.json"), "{}").unwrap();

    let out = dataset(&h, &["fleet", "sync", "--all-projects"]);
    let mut want = vec![
        format!(
            "{}: written 1 (tokens 0, unpriced 1), already 0, deferred 0, skipped 0",
            a.display()
        ),
        format!(
            "{}: written 2 (tokens 0, unpriced 2), already 0, deferred 0, skipped 1",
            b.display()
        ),
    ];
    // Ledgers go in file name order.
    want.sort_by_key(|line| slug(Path::new(line.split(':').next().unwrap())));
    want.push(
        "total: written 3 (tokens 0, unpriced 3), already 0, deferred 0, skipped 1".to_string(),
    );
    assert_eq!(out.lines().collect::<Vec<_>>(), want, "{out}");
    assert_eq!(lines(&fleet_dir(&h, &a).join("runs.jsonl")).len(), 1);
    let beta = lines(&fleet_dir(&h, &b).join("runs.jsonl"));
    assert_eq!(beta.len(), 2);
    assert_eq!(beta[0]["project"], b.to_string_lossy().as_ref());
}

/// FDS-18: `readiness` and `export` sync first. FDS-17: the readiness
/// output before the fleet section is the same with and without fleet
/// rows.
#[test]
fn fds_18_export_and_readiness_sync_first() {
    let h = Harness::new("fds18-first");
    let p = h.project.clone();
    write_ledger(&h, &p, &[]);
    let empty = dataset(&h, &["readiness"]);
    let (rounds_before, fleet_before) = empty.split_at(empty.find("fleet observed:").unwrap());
    assert!(fleet_before.contains("  rows: 0\n"), "{empty}");

    write_ledger(&h, &p, &[finished("r-1", &p, None)]);
    let out = dataset(&h, &["readiness"]);
    let (rounds_after, fleet_after) = out.split_at(out.find("fleet observed:").unwrap());
    assert_eq!(rounds_after, rounds_before);
    assert!(rounds_after.starts_with("verdict: not_ready\n"), "{out}");
    assert!(fleet_after.contains("  rows: 1\n"), "{out}");
    let runs = fleet_dir(&h, &p).join("runs.jsonl");
    assert_eq!(lines(&runs).len(), 1);

    write_ledger(
        &h,
        &p,
        &[finished("r-1", &p, None), finished("r-2", &p, None)],
    );
    let out = dataset(&h, &["export"]);
    assert_eq!(lines(&runs).len(), 2, "{out}");
    let line = out
        .lines()
        .find(|l| l.starts_with("exported 2 fleet rows to "))
        .unwrap_or_else(|| panic!("{out}"));
    let file = PathBuf::from(line.trim_start_matches("exported 2 fleet rows to "));
    assert!(
        file.parent().unwrap().ends_with("exports/fleet-observed-1"),
        "{}",
        file.display()
    );
    let rows = lines(&file);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["schema"] == "mh.export-fleet/1.0.0"));
}
