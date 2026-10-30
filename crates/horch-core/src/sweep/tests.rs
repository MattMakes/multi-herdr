use super::*;
use crate::execution::legacy::Record;
use crate::execution::model::{ExecutionStatus, FailureKind};
use crate::execution::records::Ledger;
use crate::workspace::testing::FakeWorkspace;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A state root and a temp root in one temp dir, and a fake herdr with 1
/// workspace (`w1`) and 1 pane (`p1`) in it.
struct World {
    tmp: tempfile::TempDir,
    herdr: FakeWorkspace,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("state")).unwrap();
        std::fs::create_dir_all(tmp.path().join("temp")).unwrap();
        let herdr = FakeWorkspace::new();
        let ws = herdr.workspace_create("fleet", None, false).unwrap();
        assert_eq!(
            (ws.workspace_id.as_str(), ws.root_pane_id.as_str()),
            ("w1", "p1")
        );
        World { tmp, herdr }
    }

    fn state(&self) -> PathBuf {
        self.tmp.path().join("state")
    }

    fn temp(&self) -> PathBuf {
        self.tmp.path().join("temp")
    }

    fn ledger(&self) -> Ledger {
        Ledger::for_project(self.state(), "/proj")
    }

    /// A working record of `role` in `ws`, in pane `pane`.
    fn record(&self, id: &str, role: &str, ws: &str, pane: &str) {
        self.ledger()
            .insert(Record {
                record_id: id.to_string(),
                role: role.to_string(),
                agent: "codex".to_string(),
                workspace_id: Some(ws.to_string()),
                pane_id: Some(pane.to_string()),
                ..Record::default()
            })
            .unwrap();
    }

    fn finish(&self, id: &str) {
        self.ledger()
            .store()
            .set_state(id, ExecutionStatus::Done)
            .unwrap();
    }

    fn status(&self, id: &str) -> ExecutionStatus {
        self.ledger().get(id).unwrap().execution_status()
    }

    /// Sweep with herdr's real answer, as if `ahead` had passed.
    fn sweep(&self, ahead: Duration) -> Applied {
        let listed = self.herdr.server_reachable().then(|| self.listed());
        self.sweep_with(listed, ahead)
    }

    fn sweep_with(&self, listed: Option<BTreeSet<String>>, ahead: Duration) -> Applied {
        let herd = Herd {
            listed,
            client: &self.herdr,
        };
        let state = self.state();
        let temp = self.temp();
        let scope = Scope {
            state_root: &state,
            temp_root: Some(&temp),
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        scan(scope, &herd, SystemTime::now() + ahead, deadline).apply(&self.herdr)
    }

    fn listed(&self) -> BTreeSet<String> {
        // The fake has no workspace list: its open workspaces are the ones
        // that still have a pane.
        let mut listed = BTreeSet::new();
        for ws in ["w1", "w2", "w3"] {
            if !self.herdr.pane_list(ws).unwrap().is_empty() {
                listed.insert(ws.to_string());
            }
        }
        listed
    }
}

fn touch(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
    path.to_path_buf()
}

fn set_age(path: &Path, age: Duration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

/// A pid that no process has: a child that ran and was reaped.
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!crate::procid::alive(pid, None), "pid {pid} was reused");
    pid
}

#[test]
fn a_spool_file_goes_after_7_days_unless_pending_mail_names_it() {
    let w = World::new();
    let dir = crate::messaging::message::spool_dir(&w.state(), "w1");
    let old = touch(&dir.join("old.md"));
    let new = touch(&dir.join("new.md"));
    let named = touch(&dir.join("named.md"));
    set_age(&old, 8 * DAY);
    set_age(&new, 6 * DAY);
    set_age(&named, 8 * DAY);
    Mail::under(&w.state(), "w1")
        .enqueue("opus-1", &format!("[opus-1] DONE: see {}", named.display()))
        .unwrap();

    let applied = w.sweep(Duration::ZERO);

    assert!(!old.exists(), "a spool file older than 7 days goes");
    assert!(new.exists(), "a spool file of 6 days stays");
    assert!(named.exists(), "a spool file that pending mail names stays");
    assert_eq!(applied.removed.get(&Kind::Spool), Some(&1));
}

#[cfg(unix)]
#[test]
fn a_socket_without_a_relay_goes_and_a_live_relay_keeps_its_socket() {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixListener;
    let w = World::new();
    let dead = crate::messaging::channel::socket_path(&w.state(), "w9");
    let live = crate::messaging::channel::socket_path(&w.state(), "w1");
    std::fs::create_dir_all(dead.parent().unwrap()).unwrap();
    drop(UnixListener::bind(&dead).unwrap());
    let listener = UnixListener::bind(&live).unwrap();
    let relay = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut line)
            .unwrap();
        (&stream).write_all(b"ok\n").unwrap();
        line
    });

    let applied = w.sweep(Duration::ZERO);

    assert!(!dead.exists(), "a socket that no relay answers goes");
    assert!(live.exists(), "the socket of a live relay stays");
    assert!(relay.join().unwrap().contains("ping"));
    assert_eq!(applied.removed.get(&Kind::Socket), Some(&1));
}

#[test]
fn a_gone_workspace_loses_its_registry_and_mail_unless_mail_is_pending() {
    let w = World::new();
    let registry = |root: &Path, ws: &str| root.join(format!("herdr-orchestration-{ws}"));
    for ws in ["w1", "w8", "w9"] {
        touch(&registry(&w.state(), ws).join(".seq-opus"));
        Mail::under(&w.state(), ws).ensure_dir().unwrap();
    }
    touch(&registry(&w.temp(), "w9").join("opus-1.id"));
    touch(&registry(&w.temp(), "w1").join("opus-1.id"));
    Mail::to_role(&w.state(), "w8", "opus-1")
        .enqueue("horch", "reviews are in")
        .unwrap();

    let applied = w.sweep(2 * SETTLE);

    assert!(
        !registry(&w.state(), "w9").exists(),
        "a gone workspace's registry goes"
    );
    assert!(
        !registry(&w.temp(), "w9").exists(),
        "and its old temp registry"
    );
    assert!(
        !Mail::under(&w.state(), "w9").dir().exists(),
        "and its mail"
    );
    assert!(
        registry(&w.state(), "w8").exists(),
        "pending mail keeps the registry"
    );
    assert!(Mail::under(&w.state(), "w8").dir().exists(), "and the mail");
    assert!(
        registry(&w.state(), "w1").exists(),
        "a listed workspace keeps its registry"
    );
    assert!(
        registry(&w.temp(), "w1").exists(),
        "and its old temp registry"
    );
    assert!(Mail::under(&w.state(), "w1").dir().exists(), "and its mail");
    assert_eq!(applied.removed.get(&Kind::Registry), Some(&2));
    assert_eq!(applied.removed.get(&Kind::Mail), Some(&1));
}

#[test]
fn a_new_dir_of_a_gone_workspace_stays_for_an_hour() {
    let w = World::new();
    let registry = touch(&w.state().join("herdr-orchestration-w9").join("x"));
    w.sweep(Duration::ZERO);
    assert!(
        registry.exists(),
        "a workspace may start while the sweep reads"
    );
}

#[test]
fn the_bundle_of_a_done_record_goes_and_a_live_or_unknown_one_stays() {
    let w = World::new();
    w.record("done-1", "opus-1", "w1", "p1");
    w.finish("done-1");
    w.record("live-1", "opus-2", "w1", "p1");
    let bundles = w.state().join("skill-bundles");
    for id in ["done-1", "live-1", "unknown-1"] {
        touch(&bundles.join(id).join("skills").join("SKILL.md"));
    }

    let applied = w.sweep(Duration::ZERO);

    assert!(
        !bundles.join("done-1").exists(),
        "a done record's bundle goes"
    );
    assert!(
        bundles.join("live-1").exists(),
        "a live record's bundle stays"
    );
    assert!(
        bundles.join("unknown-1").exists(),
        "a bundle of no record stays"
    );
    assert_eq!(applied.removed.get(&Kind::Bundle), Some(&1));
}

/// A private codex home as the codex harness leaves it: links, `rules/`,
/// and the cache codex writes at its top.
#[cfg(unix)]
fn codex_home(state: &Path, role: &str, target: &Path) -> PathBuf {
    let dir = state
        .join("codex-home")
        .join(format!("{role}-{}", crate::mint_uuid()));
    touch(&dir.join("rules").join("horch.rules"));
    std::os::unix::fs::symlink(target, dir.join("config.toml")).unwrap();
    touch(&dir.join("cloud-config-bundle-cache.json"));
    dir
}

#[cfg(unix)]
#[test]
fn a_codex_home_of_a_finished_role_goes_with_its_cache_but_never_its_target() {
    let w = World::new();
    let target = touch(&w.tmp.path().join("real-codex").join("config.toml"));
    w.record("done-1", "codex-1", "w1", "p1");
    w.finish("done-1");
    w.record("live-1", "codex-2", "w1", "p1");
    let done = codex_home(&w.state(), "codex-1", &target);
    let live = codex_home(&w.state(), "codex-2", &target);
    let unknown = codex_home(&w.state(), "codex-3", &target);
    let stranded = codex_home(&w.state(), "codex-1", &target);
    touch(&stranded.join("brand_new.sqlite"));

    let applied = w.sweep(2 * SETTLE);

    assert!(!done.exists(), "a finished role's home goes, cache and all");
    assert!(target.exists(), "the link target is never followed");
    assert!(live.exists(), "a live role's home stays");
    assert!(stranded.exists(), "a home with stranded state stays");
    assert!(unknown.exists(), "a home of a role no record names stays");
    assert_eq!(applied.removed.get(&Kind::CodexHome), Some(&1));
}

#[cfg(unix)]
#[test]
fn a_new_codex_home_stays_for_an_hour() {
    let w = World::new();
    let target = touch(&w.tmp.path().join("real-codex").join("config.toml"));
    w.record("done-1", "codex-1", "w1", "p1");
    w.finish("done-1");
    let home = codex_home(&w.state(), "codex-1", &target);
    w.sweep(Duration::ZERO);
    assert!(home.exists(), "its launch may not have a record yet");
}

#[test]
fn a_working_record_whose_pane_is_gone_ends_and_loses_its_bundle() {
    let w = World::new();
    let p2 = w
        .herdr
        .pane_split("p1", crate::workspace::model::Direction::Right);
    let p2 = p2.unwrap();
    w.record("open-1", "opus-1", "w1", "p1");
    w.record("closed-1", "opus-2", "w1", &p2);
    w.record("gone-ws-1", "opus-3", "w7", "p9");
    w.herdr.pane_close(&p2).unwrap();
    let bundles = w.state().join("skill-bundles");
    touch(&bundles.join("closed-1").join("x"));
    touch(&bundles.join("open-1").join("x"));

    let applied = w.sweep(Duration::ZERO);

    let vanished = ExecutionStatus::Failed {
        failure: FailureKind::PaneVanished,
    };
    assert_eq!(w.status("closed-1"), vanished);
    assert_eq!(w.status("gone-ws-1"), vanished);
    assert!(
        w.status("open-1").is_live(),
        "an open pane keeps its record"
    );
    assert!(
        !bundles.join("closed-1").exists(),
        "the ended record's bundle goes"
    );
    assert!(bundles.join("open-1").exists());
    assert_eq!(applied.removed.get(&Kind::Record), Some(&2));
}

#[cfg(unix)]
#[test]
fn herdr_unreachable_removes_nothing_and_ends_nothing() {
    let w = World::new();
    w.record("gone-ws-1", "codex-1", "w7", "p9");
    w.record("done-1", "codex-2", "w1", "p1");
    w.finish("done-1");
    let target = touch(&w.tmp.path().join("real-codex").join("config.toml"));
    let mut kept = vec![
        codex_home(&w.state(), "codex-2", &target),
        touch(&w.state().join("skill-bundles").join("done-1").join("x")),
        touch(&w.state().join("herdr-orchestration-w9").join(".seq-opus")),
        touch(&w.temp().join("herdr-orchestration-w9").join(".seq-opus")),
        touch(&w.state().join(format!(".a.json.tmp.{}", dead_pid()))),
    ];
    let spool = touch(&w.state().join("messages").join("w9").join("old.md"));
    set_age(&spool, 8 * DAY);
    kept.push(spool);
    let socket = crate::messaging::channel::socket_path(&w.state(), "w9");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    kept.push(socket);
    w.herdr.set_reachable(false);

    let applied = w.sweep(2 * SETTLE);

    for path in &kept {
        assert!(path.exists(), "{} stays", path.display());
    }
    assert!(w.status("gone-ws-1").is_live(), "the record stays working");
    assert_eq!(applied.total(), 0);
    assert!(applied.errors.is_empty(), "{:?}", applied.errors);
}

#[test]
fn a_temp_file_of_a_dead_pid_goes_and_one_of_a_live_pid_stays() {
    let w = World::new();
    let me = std::process::id();
    let dead = dead_pid();
    let mail = w.state().join("mail").join("w1");
    let files = [
        touch(&w.state().join(format!(".a.json.tmp-{dead}-0123456789ab"))),
        touch(&mail.join(format!("b.json.tmp.{dead}"))),
        touch(&w.state().join(format!(".c.json.tmp-{me}-0123456789ab"))),
        touch(&w.state().join(format!("d.json.tmp.{me}"))),
        touch(&w.state().join(format!(".e.json.tmp-{dead}-not-a-nonce"))),
    ];

    let applied = w.sweep(2 * SETTLE);

    let exists: Vec<bool> = files.iter().map(|f| f.exists()).collect();
    assert_eq!(exists, [false, false, true, true, true]);
    assert_eq!(applied.removed.get(&Kind::TempFile), Some(&2));
}

#[test]
fn a_new_temp_file_of_a_dead_pid_stays_for_an_hour() {
    let w = World::new();
    let file = touch(
        &w.state()
            .join(format!(".a.json.tmp-{}-0123456789ab", dead_pid())),
    );
    w.sweep(Duration::ZERO);
    assert!(file.exists());
}

#[test]
fn scan_counts_without_removing() {
    let w = World::new();
    w.record("done-1", "opus-1", "w1", "p1");
    w.finish("done-1");
    let bundle = touch(&w.state().join("skill-bundles").join("done-1").join("x"));
    let registry = touch(&w.state().join("herdr-orchestration-w9").join("x"));
    let state = w.state();
    let herd = Herd {
        listed: Some(w.listed()),
        client: &w.herdr,
    };
    let scope = Scope {
        state_root: &state,
        temp_root: None,
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    let plan = scan(scope, &herd, SystemTime::now() + 2 * SETTLE, deadline);

    let counts = plan.counts();
    assert_eq!(counts.get(&Kind::Bundle), Some(&1));
    assert_eq!(counts.get(&Kind::Registry), Some(&1));
    assert!(
        bundle.exists() && registry.exists(),
        "a scan removes nothing"
    );
}

#[test]
fn a_spent_deadline_reads_nothing_more() {
    let w = World::new();
    w.record("gone-ws-1", "opus-1", "w7", "p9");
    let state = w.state();
    let herd = Herd {
        listed: Some(w.listed()),
        client: &w.herdr,
    };
    let scope = Scope {
        state_root: &state,
        temp_root: None,
    };
    let plan = scan(scope, &herd, SystemTime::now(), Instant::now());
    assert!(!plan.counts().contains_key(&Kind::Record));
}

#[test]
fn the_sweep_is_due_when_the_stamp_is_missing_or_an_hour_old() {
    let tmp = tempfile::tempdir().unwrap();
    let stamp = tmp.path().join(STAMP);
    let now = SystemTime::now();
    assert!(due(&stamp, now), "no stamp yet");
    touch(&stamp);
    assert!(!due(&stamp, now + INTERVAL / 2), "swept half an hour ago");
    assert!(due(&stamp, now + INTERVAL * 2), "swept 2 hours ago");
}

#[test]
fn temp_names_give_their_pid() {
    assert_eq!(temp_pid(".ledger.json.tmp-123-0123456789ab"), Some(123));
    assert_eq!(temp_pid("ledger.json.tmp.456"), Some(456));
    assert_eq!(temp_pid(".ledger.json.tmp-123-xyz"), None);
    assert_eq!(temp_pid("ledger.json"), None);
    assert_eq!(
        temp_pid(".1.json.tmp"),
        None,
        "mail's own temp name is not ours"
    );
}

/// A context whose state root is `<dir>/state` and whose herdr is a warmed
/// script that lists workspace `w1` with pane `p1`.
#[cfg(unix)]
fn script_ctx(dir: &Path) -> RuntimeContext {
    use std::os::unix::fs::PermissionsExt;
    let herdr = dir.join("herdr");
    std::fs::write(
        &herdr,
        "#!/bin/sh\ncase \"$1 $2\" in\n\
         'workspace list') echo '{\"result\":{\"workspaces\":[{\"workspace_id\":\"w1\"}]}}' ;;\n\
         'pane list') echo '{\"result\":{\"panes\":[{\"pane_id\":\"p1\"}]}}' ;;\n\
         warm*) ;;\n\
         *) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
    // macOS scans a new executable at its first exec: warm it.
    let warm = std::process::Command::new(&herdr).arg("warm").status();
    assert!(warm.unwrap().success());
    let env = crate::runtime::MapEnv::new(dir)
        .with("HOME", &dir.to_string_lossy())
        .with("HORCH_STATE_DIR", &dir.join("state").to_string_lossy())
        .with("HORCH_HERDR_BIN", &herdr.to_string_lossy());
    RuntimeContext::from_env(&env).unwrap()
}

/// What `horch fleet` and `horch spawn` run: on old state, 1 run leaves
/// bundles only for live records, and a 2nd run within the hour reads
/// nothing.
#[cfg(unix)]
#[test]
fn run_due_leaves_bundles_only_for_live_records_once_an_hour() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = script_ctx(tmp.path());
    let state = tmp.path().join("state");
    let ledger = Ledger::for_project(&state, "/proj");
    for (id, pane) in [("live-1", "p1"), ("closed-1", "p7"), ("done-1", "p1")] {
        ledger
            .insert(Record {
                record_id: id.into(),
                role: format!("opus-{id}"),
                workspace_id: Some("w1".into()),
                pane_id: Some(pane.into()),
                ..Record::default()
            })
            .unwrap();
        touch(&state.join("skill-bundles").join(id).join("x"));
    }
    ledger.done("done-1", "finished").unwrap();

    run_due(&ctx);

    let left: Vec<String> = entries(&state.join("skill-bundles"))
        .iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["live-1"]);

    touch(&state.join("skill-bundles").join("done-1").join("x"));
    run_due(&ctx);
    assert!(
        state.join("skill-bundles").join("done-1").exists(),
        "the stamp holds the next sweep for an hour"
    );
}
