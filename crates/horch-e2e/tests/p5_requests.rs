#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::fleet_runtime::adapters::request_spool::ABORT_BEFORE_REQUEST_ARCHIVE;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{
    parse_reply, Outcome, Reply, Request,
};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::requests::ABORT_BEFORE_REQUEST_COMMIT;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::runtime::fault::ABORT_EXIT_CODE;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, Harness, DELIVERY_PROBE_FAULT};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};

const MAX_BYTES: u64 = 65_536;

const FLEET_TOML: &str = "pass_period_s = 1\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("FLEET_ROOT", fleet_root(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_TEST_ASSERT_READ_ONLY", "1");
    std::fs::create_dir_all(fleet_root(&h)).unwrap();
    std::fs::write(fleet_root(&h).join("fleet.toml"), FLEET_TOML).unwrap();
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn fleet_root(h: &Harness) -> PathBuf {
    h.root.join("fleet")
}

fn channel(h: &Harness) -> PathBuf {
    fleet_root(h).join("run").join("fleetd.sock")
}

fn requests_dir(h: &Harness) -> PathBuf {
    fleet_root(h).join("run").join("requests")
}

fn start_fleetd(h: &mut Harness, fleetds: &mut Fleetds, name: &str, faults: &[&str]) -> u32 {
    if faults.is_empty() {
        h.unset("HORCH_FAULT");
    } else {
        let mut all = vec![DELIVERY_PROBE_FAULT];
        all.extend_from_slice(faults);
        h.set("HORCH_FAULT", all.join(","));
    }
    let pid = fleetds.start(h, name);
    h.unset("HORCH_FAULT");
    let root = fleet_root(h);
    wait_for("the operator token file", || {
        token_file::read(&root).ok().map(|_| ())
    });
    pid
}

fn note_request(h: &Harness, id: &str, note: &str) -> Request {
    let mut payload = Map::new();
    payload.insert("text".into(), json!(format!("spawn gate: {note}")));
    Request {
        request_id: RequestId::parse(id).unwrap(),
        kind: Kind::SpawnGateNote,
        payload,
        token: Some(token_file::read(&fleet_root(h)).unwrap().text().to_string()),
    }
}

fn connect(h: &Harness) -> UnixStream {
    let path = channel(h);
    wait_for("fleetd to listen on its channel", || {
        UnixStream::connect(&path).ok()
    })
}

fn send(h: &Harness, request: &Request) -> Reply {
    let mut stream = connect(h);
    stream.write_all(request.to_line().as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut line = Vec::new();
    BufReader::new(&stream)
        .read_until(b'\n', &mut line)
        .unwrap();
    assert_eq!(line.last(), Some(&b'\n'), "1 whole reply line");
    parse_reply(&line, MAX_BYTES).unwrap()
}

fn write_request_file(h: &Harness, request: &Request) -> PathBuf {
    let dir = requests_dir(h);
    wait_for("the request folder", || dir.is_dir().then_some(()));
    let name = format!("{}.json", request.request_id);
    let tmp = h.tmp.join(format!("{name}.tmp"));
    std::fs::write(&tmp, request.to_line()).unwrap();
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).unwrap();
    let path = dir.join(&name);
    std::fs::rename(&tmp, &path).unwrap();
    path
}

fn archived(h: &Harness, request: &Request) -> PathBuf {
    requests_dir(h)
        .join("archive")
        .join(format!("{}.json", request.request_id))
}

fn ledger(h: &Harness) -> Connection {
    Connection::open_with_flags(db(h), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn row(h: &Harness, id: &str) -> Option<(String, String, Option<String>)> {
    ledger(h)
        .query_row(
            "SELECT state, payload_sha256, result_json FROM requests WHERE request_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap()
}

fn state(h: &Harness, id: &str) -> Option<String> {
    row(h, id).map(|(state, _, _)| state)
}

fn rows_of(h: &Harness, id: &str) -> i64 {
    ledger(h)
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE request_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

fn effects(h: &Harness, note: &str) -> i64 {
    ledger(h)
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind = 'note' AND text = ?1",
            [format!("spawn gate: {note}")],
            |r| r.get(0),
        )
        .unwrap()
}

fn applied(reply: &Reply) -> &Value {
    match &reply.outcome {
        Outcome::Applied(result) => result,
        Outcome::Refused(reason) => panic!("request {} refused: {reason}", reply.request_id),
    }
}

#[test]
fn request_sent_2_times_with_1_request_id_has_1_effect() {
    let mut h = world("p5-req-twice");
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&mut h, &mut fleetds, "fleetd", &[]);
    let id = "5e0c2b1a-7d41-4f3e-9a6b-0c1d2e3f4a51";
    let request = note_request(&h, id, "twice");

    let first = send(&h, &request);
    let second = send(&h, &request);

    assert_eq!(first.request_id.as_str(), id);
    assert_eq!(second, first, "the second send gets the stored result");
    applied(&first);
    assert_eq!(effects(&h, "twice"), 1, "1 request_id has 1 effect");
    assert_eq!(rows_of(&h, id), 1);
    assert_eq!(state(&h, id).as_deref(), Some("applied"));
}

#[test]
fn fleetd_killed_before_the_commit_applies_the_request_file_once_after_restart() {
    let mut h = world("p5-req-precommit");
    let mut fleetds = Fleetds::new(&h);
    let pid = start_fleetd(
        &mut h,
        &mut fleetds,
        "fleetd-1",
        &[ABORT_BEFORE_REQUEST_COMMIT],
    );
    let id = "2a7f0e61-3c58-4b9d-8e12-6f4a0b7c9d22";
    let request = note_request(&h, id, "before the commit");
    let file = write_request_file(&h, &request);

    let status = fleetds.wait_exit(pid);
    assert_eq!(
        status.code(),
        Some(ABORT_EXIT_CODE),
        "fleetd stops at {ABORT_BEFORE_REQUEST_COMMIT}: {}",
        fleetds.output("fleetd-1", "stderr")
    );
    assert_eq!(
        state(&h, id).as_deref(),
        Some("pending"),
        "the pending row is committed, the decision is not"
    );
    assert_eq!(
        effects(&h, "before the commit"),
        0,
        "no effect before the commit"
    );
    assert!(file.is_file(), "the request file stays for the next fleetd");
    assert!(!archived(&h, &request).exists());

    start_fleetd(&mut h, &mut fleetds, "fleetd-2", &[]);
    wait_for("the request file in archive/", || {
        archived(&h, &request).is_file().then_some(())
    });

    assert!(!file.exists());
    assert_eq!(state(&h, id).as_deref(), Some("applied"));
    assert_eq!(rows_of(&h, id), 1);
    assert_eq!(effects(&h, "before the commit"), 1, "applied 1 time");
}

#[test]
fn fleetd_killed_after_the_commit_only_archives_the_file() {
    let mut h = world("p5-req-archive");
    let mut fleetds = Fleetds::new(&h);
    let pid = start_fleetd(
        &mut h,
        &mut fleetds,
        "fleetd-1",
        &[ABORT_BEFORE_REQUEST_ARCHIVE],
    );
    let id = "9c3d5e7f-1a2b-4c6d-8e0f-a1b2c3d4e5f6";
    let request = note_request(&h, id, "after the commit");
    let file = write_request_file(&h, &request);

    let status = fleetds.wait_exit(pid);
    assert_eq!(
        status.code(),
        Some(ABORT_EXIT_CODE),
        "fleetd stops at {ABORT_BEFORE_REQUEST_ARCHIVE}: {}",
        fleetds.output("fleetd-1", "stderr")
    );
    let committed = row(&h, id).expect("the requests row");
    assert_eq!(committed.0, "applied", "the decision is committed");
    assert_eq!(effects(&h, "after the commit"), 1);
    assert!(file.is_file(), "the file is not archived yet");

    start_fleetd(&mut h, &mut fleetds, "fleetd-2", &[]);
    wait_for("the request file in archive/", || {
        archived(&h, &request).is_file().then_some(())
    });

    assert!(!file.exists());
    assert_eq!(
        row(&h, id),
        Some(committed),
        "the row keeps its stored result"
    );
    assert_eq!(rows_of(&h, id), 1);
    assert_eq!(effects(&h, "after the commit"), 1, "no second effect");
}

#[test]
fn lost_reply_is_found_by_horch_request_and_not_applied_again() {
    let mut h = world("p5-req-lost");
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&mut h, &mut fleetds, "fleetd", &[]);
    let id = "7b1e9d3c-5a24-4f60-b8c7-d2e1f0a9b834";
    let request = note_request(&h, id, "lost reply");

    let mut stream = connect(&h);
    stream.write_all(request.to_line().as_bytes()).unwrap();
    stream.flush().unwrap();
    wait_for("fleetd to decide the request", || {
        (state(&h, id).as_deref() == Some("applied")).then_some(())
    });
    drop(stream);

    let o = h.run(&["request", id]);
    assert!(o.status.success(), "horch request {id}: {}", text(&o));
    let said = String::from_utf8_lossy(&o.stdout);
    assert!(
        said.contains(&format!("request {id}: applied")),
        "horch request prints the state: {said}"
    );
    let (_, _, result) = row(&h, id).unwrap();
    let result = result.expect("the stored result");
    assert!(
        said.contains(&format!("result: {result}")),
        "horch request prints the stored result: {said}"
    );

    let again = send(&h, &request);
    let stored: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(applied(&again), &stored);
    assert_eq!(rows_of(&h, id), 1);
    assert_eq!(effects(&h, "lost reply"), 1, "not applied again");
}

#[test]
fn known_request_id_with_another_payload_is_refused() {
    let mut h = world("p5-req-payload");
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&mut h, &mut fleetds, "fleetd", &[]);
    let id = "c4d5e6f7-0819-4a2b-9c3d-4e5f60718293";
    let first = note_request(&h, id, "first payload");
    applied(&send(&h, &first));
    let before = row(&h, id).unwrap();

    let other = note_request(&h, id, "other payload");
    let reply = send(&h, &other);

    match &reply.outcome {
        Outcome::Refused(reason) => assert!(
            reason.contains("known with another payload"),
            "the reason names the payload: {reason}"
        ),
        Outcome::Applied(result) => panic!("another payload was applied: {result}"),
    }
    assert_eq!(
        row(&h, id),
        Some(before.clone()),
        "the row keeps its first payload"
    );
    assert_eq!(rows_of(&h, id), 1);
    assert_eq!(effects(&h, "first payload"), 1);
    assert_eq!(
        effects(&h, "other payload"),
        0,
        "no effect of the other payload"
    );

    let file = write_request_file(&h, &other);
    wait_for("the request file in archive/", || {
        archived(&h, &other).is_file().then_some(())
    });
    assert!(!file.exists());
    assert_eq!(row(&h, id), Some(before));
    assert_eq!(effects(&h, "other payload"), 0);
}
