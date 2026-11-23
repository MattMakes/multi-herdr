#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::Connection;
use serde_json::Value;

const PERSONA: &str = "opus";

const FAKE_FILE: &str = "fake-claude.txt";

fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct Spec<'a> {
    check: &'a str,
    kind: WorkKind,
    own: &'a [&'a str],
    writes: Option<(&'a str, &'a str)>,
}

struct World {
    h: Harness,
    task_id: String,
}

impl World {
    fn new(name: &str, spec: &Spec) -> World {
        let mut h = Harness::new(name).with_git();
        link_bwrap(&h);
        let db = h.root.join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_FAKE_SCENARIO", "exec,commit,result");
        let fleet = h.fleet_root();
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(
            fleet.join("fleet.toml"),
            "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
        )
        .unwrap();
        std::fs::write(
            h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        World::git(&h, &h.project, &["add", "fleet.toml"]);
        World::git(
            &h,
            &h.project,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
        );
        if let Some((path, body)) = spec.writes {
            World::docs_author(&mut h, path, body);
        }
        let workspace = h.fleet_workspace();
        assert_eq!(workspace, "w1");
        h.set("HORCH_WORKSPACE_ID", workspace);
        let task_id = World::queue(&h, spec);
        World { h, task_id }
    }

    fn docs_author(h: &mut Harness, path: &str, body: &str) {
        let fake = h.bin.join("claude");
        let script = h.root.join("docs-author.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 if [ -n \"${{HORCH_TASK_YAML:-}}\" ] && [ -f \"$HORCH_TASK_YAML\" ]; then\n\
                 while IFS= read -r line; do\n\
                 case \"$line\" in worktree:*) tree=${{line#worktree: }};; esac\n\
                 done < \"$HORCH_TASK_YAML\"\n\
                 tree=${{tree#\\\"}}; tree=${{tree%\\\"}}\n\
                 cd \"$tree\" || exit 1\n\
                 printf '%s\\n' '{body}' > '{path}' || exit 1\n\
                 \"$HORCH_GIT_BIN\" add -- '{path}' || exit 1\n\
                 \"$HORCH_GIT_BIN\" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
                 -c user.name=author -c user.email=author@horch.invalid \
                 commit -q -m 'the work' -- '{path}' || exit 1\n\
                 fi\n\
                 exec '{fake}' \"$@\"\n",
                fake = fake.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        h.set("HORCH_CLAUDE_BIN", script.to_string_lossy());
    }

    fn git(h: &Harness, dir: &Path, args: &[&str]) -> String {
        let mut c = h.git_cmd(args);
        c.current_dir(dir);
        let o = c.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn queue(h: &Harness, spec: &Spec) -> String {
        let dir = h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        let ticket = dir.join("t0.md");
        std::fs::write(&ticket, "# t0\n").unwrap();
        World::git(h, &h.project, &["branch", "fleet/r1", "main"]);
        let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
        let conn = store.conn();
        let run = runs::create(
            conn,
            &h.project,
            &NewRun {
                slug: "r1".into(),
                workspace_id: Some("w1".into()),
                base_branch: Some("main".into()),
                run_branch: Some("fleet/r1".into()),
                finish_steps: VerifyProfile::Minimal.steps(),
                ..Default::default()
            },
        )
        .unwrap();
        schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
            .unwrap()
            .expect("the idea of r1");
        let orchestrator = workers::insert(
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
        runs::set_orchestrator(conn, &run.run_id, &orchestrator).unwrap();
        let mut new = NewTask::work("t0", &ticket.to_string_lossy(), PERSONA);
        new.ticket_snapshot = Some("# t0\n".into());
        new.land_check = Some(spec.check.into());
        new.work_kind = Some(spec.kind);
        new.own = spec.own.iter().map(|s| s.to_string()).collect();
        tasks::add(conn, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.root.join("ledger.sqlite3")).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn attempt_folder(&self) -> PathBuf {
        attempt_dir(&self.h.fleet_root(), &self.task_id, "A1")
    }

    fn start_fleetd(&self) {
        self.h.serve_fleetd();
    }

    fn worktree(&self) -> PathBuf {
        let yaml = self.attempt_folder().join("task.yaml");
        let found = wait_for_or(
            "fleetd to write the task.yaml of attempt A1",
            || std::fs::read_to_string(&yaml).ok(),
            || self.evidence(),
        );
        let path = found
            .lines()
            .find_map(|l| l.trim().strip_prefix("worktree:"))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_else(|| panic!("no worktree in {found}"));
        PathBuf::from(path)
    }

    fn task_status(&self) -> String {
        self.conn()
            .query_row(
                "SELECT status FROM tasks WHERE task_id = ?1",
                [&self.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn attempt(&self) -> Option<(String, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [format!("{}/A1", self.task_id)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    fn moves(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT to_state, COALESCE(cause, '') FROM events
                 WHERE entity = 'task' AND entity_id = ?1 AND to_state IS NOT NULL
                 ORDER BY event_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([&self.task_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    fn verdict_path(&self, n: u32) -> PathBuf {
        self.h
            .fleet_root()
            .join("tasks")
            .join(&self.task_id)
            .join(format!("verdict-{n}.json"))
    }

    fn verdict(&self, n: u32) -> Value {
        let path = self.verdict_path(n);
        wait_for_or(
            &format!("verdict-{n}.json of the task"),
            || std::fs::read(&path).ok(),
            || self.evidence(),
        );
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
    }

    fn check_job_ended(&self) -> bool {
        self.conn()
            .query_row(
                "SELECT count(*) FROM jobs
                 WHERE kind = 'task_check' AND state NOT IN ('queued', 'running')",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }

    fn evidence(&self) -> String {
        let conn = self.conn();
        let jobs: Vec<String> = conn
            .prepare("SELECT kind || ' ' || state || ' ' || COALESCE(result_json, '') FROM jobs")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        format!(
            "task status: {}\nattempt: {:?}\nmoves: {:?}\njobs: {jobs:?}\nclaude calls: {:?}\nfleetd stderr: {}",
            self.task_status(),
            self.attempt(),
            self.moves(),
            self.h.calls_of("claude"),
            std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default()
        )
    }
}

fn assert_not_done(w: &World) {
    assert_ne!(w.task_status(), "done", "{}", w.evidence());
    assert!(
        !w.moves().iter().any(|(to, _)| to == "done"),
        "{}",
        w.evidence()
    );
}

#[test]
fn passing_code_task_reaches_done_only_through_the_verifier() {
    let w = World::new(
        "p4v-pass",
        &Spec {
            check: "test -s fake-claude.txt",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "passed", "{verdict}");
    wait_for_or(
        "the task to be done",
        || (w.task_status() == "done").then_some(()),
        || w.evidence(),
    );
    let to_done: Vec<_> = w
        .moves()
        .into_iter()
        .filter(|(to, _)| to == "done")
        .collect();
    assert_eq!(to_done.len(), 1, "{}", w.evidence());
    assert!(
        to_done[0].1.contains("the verifier: verdict-1.json judged"),
        "{}",
        w.evidence()
    );
    assert!(w.worktree().join(FAKE_FILE).is_file());
}

#[test]
fn push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier() {
    let w = World::new(
        "p4v-chain",
        &Spec {
            check: "test -s fake-claude.txt",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    assert!(w.attempt().is_none(), "{}", w.evidence());
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "passed", "{verdict}");
    wait_for_or(
        "the task to be done",
        || (w.task_status() == "done").then_some(()),
        || w.evidence(),
    );

    let moves = w.moves();
    assert!(
        moves
            .iter()
            .any(|(to, why)| to == "leased" && why.contains("dispatch binds T-1 to attempt")),
        "{}",
        w.evidence()
    );
    let (state, outcome) = w.attempt().expect("attempt A1");
    assert_eq!(state, "ended", "{}", w.evidence());
    assert_eq!(outcome.as_deref(), Some("verified"), "{}", w.evidence());

    let jobs: Vec<String> = w
        .conn()
        .prepare("SELECT state FROM jobs WHERE kind = 'worktree_add'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(jobs, ["passed"], "{}", w.evidence());
    let tree = w.worktree();
    assert!(tree.join(".git").exists(), "{}", w.evidence());
    assert!(tree.join(FAKE_FILE).is_file(), "{}", w.evidence());

    let dir = w.attempt_folder();
    let yaml = std::fs::read_to_string(dir.join("task.yaml")).unwrap();
    assert!(yaml.lines().any(|l| l.trim() == "attempt: A1"), "{yaml}");
    assert!(yaml.lines().any(|l| l.trim() == "kind: feature"), "{yaml}");
    let start = std::fs::read_to_string(dir.join("inbox").join("START.md")).unwrap();
    assert!(start.starts_with("# t0\n"), "{start}");
    assert!(start.contains("horch result"), "{start}");

    let result: Value =
        serde_json::from_slice(&std::fs::read(dir.join("outbox").join("result.json")).unwrap())
            .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    let agent =
        w.h.calls_of("claude")
            .into_iter()
            .find(|c| c["commit"]["head"].is_string())
            .unwrap_or_else(|| panic!("no launch of the fake agent\n{}", w.evidence()));
    assert_eq!(agent["result"]["exit"], 0, "{agent}");

    assert!(verdict["job"].is_number(), "{verdict}");
    let to_done: Vec<_> = moves.into_iter().filter(|(to, _)| to == "done").collect();
    assert_eq!(to_done.len(), 1, "{}", w.evidence());
    assert!(
        to_done[0].1.contains("the verifier: verdict-1.json judged"),
        "{}",
        w.evidence()
    );
}

#[test]
fn fm9_code_task_with_a_failing_check_stays_not_done() {
    let w = World::new(
        "p4v-fm9-code",
        &Spec {
            check: "test -f the-file-that-the-worker-never-wrote",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().and_then(|a| a.1).as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    assert_not_done(&w);
}

#[test]
fn fm9_docs_task_without_its_section_stays_not_done() {
    let w = World::new(
        "p4v-fm9-docs",
        &Spec {
            check: "true",
            kind: WorkKind::Docs,
            own: &["guide.md"],
            writes: Some(("guide.md", "The guide has text and no section.")),
        },
    );
    w.start_fleetd();
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().and_then(|a| a.1).as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    let checked = w.verdict(1);
    assert_eq!(checked["decision"], "passed", "{checked}");
    let verdict = w.verdict(2);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    assert!(
        verdict["reason"].as_str().unwrap().contains("guide.md"),
        "{verdict}"
    );
    assert_not_done(&w);
}

#[test]
fn sr4_check_that_writes_outside_its_paths_is_refused() {
    let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let escaped = outside.path().join("escaped.txt");
    let check = format!(
        "echo inside > written-inside.txt && echo outside > {}",
        escaped.display()
    );
    let w = World::new(
        "p4v-sr4",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        assert!(
            !escaped.exists() && !w.worktree().join("written-inside.txt").exists(),
            "SR-4 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    assert!(
        w.worktree().join("written-inside.txt").is_file(),
        "the check did not run, so its refusal proves nothing\n{verdict}\n{}",
        w.evidence()
    );
    assert!(
        !escaped.exists(),
        "SR-4: the check wrote {} outside its paths\n{}",
        escaped.display(),
        w.evidence()
    );
    let reason = verdict["reason"].as_str().unwrap();
    assert!(
        reason.lines().any(|l| l.starts_with("horch: SR-4")),
        "the verdict names no SR-4 rule\n{verdict}"
    );
    assert!(
        reason.contains("Operation not permitted") || reason.contains("Read-only file system"),
        "the verdict holds no refusal of the sandbox\n{verdict}"
    );
    assert_not_done(&w);
}

const OFF_HOST_URL: &str = "http://192.0.2.1:9/";
const SANDBOX_HAS_ITS_OWN_LOOPBACK: bool = cfg!(target_os = "linux");

fn host_lan_address() -> Option<std::net::Ipv4Addr> {
    const LISTS: [(&str, &[&str]); 4] = [
        ("/sbin/ifconfig", &[]),
        ("/usr/sbin/ip", &["-4", "-o", "addr"]),
        ("/sbin/ip", &["-4", "-o", "addr"]),
        ("/usr/bin/ip", &["-4", "-o", "addr"]),
    ];
    let (program, args) = LISTS.into_iter().find(|(p, _)| Path::new(p).is_file())?;
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(2)
        .filter(|pair| pair[0] == "inet")
        .filter_map(|pair| pair[1].split('/').next()?.parse().ok())
        .find(|ip: &std::net::Ipv4Addr| !ip.is_loopback())
}

const LAN_LISTENER: &str = "$| = 1; alarm 600; \
    my $l = IO::Socket::INET->new(Listen => 8, LocalAddr => $ARGV[0], LocalPort => 0) \
      or die \"listen: $!\"; \
    print $l->sockport, \"\\n\"; \
    while (my $c = $l->accept) { \
      my $r = \"\"; while ($r !~ /hello|control/ && sysread($c, my $b, 1024)) { $r .= $b } \
      print \"$&\\n\" if $r =~ /hello|control/; \
      print $c \"HTTP/1.1 200 OK\\r\\nContent-Length: 0\\r\\nConnection: close\\r\\n\\r\\n\"; \
      close $c }";

struct LanListener {
    child: std::process::Child,
    host: std::net::Ipv4Addr,
    port: u16,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LanListener {
    fn start(perl: &str, host: std::net::Ipv4Addr) -> LanListener {
        use std::io::BufRead as _;
        let mut child = std::process::Command::new(perl)
            .args(["-MIO::Socket::INET", "-e", LAN_LISTENER, &host.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let port = lines.next().unwrap().unwrap().parse().unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&seen);
        std::thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                log.lock().unwrap().push(line);
            }
        });
        LanListener {
            child,
            host,
            port,
            seen,
        }
    }

    fn answers_outside_the_sandbox(&self) -> bool {
        use std::io::{Read as _, Write as _};
        let limit = std::time::Duration::from_secs(5);
        let address = std::net::SocketAddr::from((self.host, self.port));
        let Ok(mut stream) = std::net::TcpStream::connect_timeout(&address, limit) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(limit));
        if stream.write_all(b"control").is_err() {
            return false;
        }
        let mut reply = String::new();
        let _ = stream.read_to_string(&mut reply);
        reply.starts_with("HTTP/1.1 200")
    }

    fn check_requests(&self) -> usize {
        let seen = self.seen.lock().unwrap();
        seen.iter().filter(|line| *line == "hello").count()
    }
}

impl Drop for LanListener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn loopback_server(
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read as _, Write as _};
    use std::sync::atomic::Ordering;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (port, seen)
}

fn sandbox_loopback_check(perl: &str, curl: &str) -> String {
    format!(
        "{perl} -MIO::Socket::INET -e '\
         alarm 20; \
         my $l = IO::Socket::INET->new(Listen => 1, LocalAddr => \"127.0.0.1\", LocalPort => 0) \
           or die \"listen: $!\"; \
         my $p = $l->sockport; \
         my $pid = fork; defined $pid or die \"fork: $!\"; \
         if (!$pid) {{ exec \"{curl}\", \"--silent\", \"--show-error\", \"--max-time\", \"5\", \
           \"--data\", \"hello\", \"http://127.0.0.1:$p/\"; die \"exec: $!\" }} \
         my $c = $l->accept or die \"accept: $!\"; \
         my $r = \"\"; while ($r !~ /hello/ && sysread($c, my $b, 1024)) {{ $r .= $b }} \
         print $c \"HTTP/1.1 200 OK\\r\\nContent-Length: 0\\r\\nConnection: close\\r\\n\\r\\n\"; \
         close $c; waitpid $pid, 0; exit($? >> 8 || $r !~ /hello/)'"
    )
}

#[test]
fn sr3_check_that_connects_off_host_is_refused() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let (port, loopback_requests) = loopback_server(std::sync::Arc::clone(&stop));
    let curl = ["/usr/bin/curl", "/bin/curl"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("curl on this host");
    let perl = ["/usr/bin/perl", "/bin/perl"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("perl on this host");
    let get = |url: &str| format!("{curl} --silent --show-error --max-time 5 --data hello {url}");
    let check = format!(
        "{} && echo reached > loopback-ok.txt; {}; {}",
        sandbox_loopback_check(perl, curl),
        get(&format!("http://127.0.0.1:{port}/")),
        get(OFF_HOST_URL)
    );
    let w = World::new(
        "p4v-sr3",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        stop.store(true, Ordering::SeqCst);
        assert!(
            loopback_requests.lock().unwrap().is_empty(),
            "SR-3 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    stop.store(true, Ordering::SeqCst);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    assert!(
        w.worktree().join("loopback-ok.txt").is_file(),
        "the check did not reach loopback, so its refusal proves nothing\n{verdict}\n{}",
        w.evidence()
    );
    let reason = verdict["reason"].as_str().unwrap();
    let refused = |host: &str| {
        reason
            .lines()
            .any(|l| l.starts_with("check| curl: (7)") && l.contains(host))
    };
    let host_requests = loopback_requests.lock().unwrap().clone();
    if SANDBOX_HAS_ITS_OWN_LOOPBACK {
        assert!(host_requests.is_empty(), "{host_requests:?}");
        assert!(refused("127.0.0.1"), "{verdict}");
    } else {
        assert!(
            host_requests.iter().any(|r| r.contains("hello")),
            "{}",
            w.evidence()
        );
    }
    assert!(
        reason
            .lines()
            .any(|l| l.starts_with("horch: SR-3") && l.contains("off-host network")),
        "the verdict names no SR-3 network rule\n{verdict}"
    );
    assert!(
        refused("192.0.2.1"),
        "SR-3: the connect to {OFF_HOST_URL} was not refused\n{verdict}"
    );
    assert_not_done(&w);
}

#[test]
fn sr3_check_that_connects_to_a_host_lan_address_is_refused() {
    let Some(lan) = host_lan_address() else {
        eprintln!("skip: this host has no non-loopback IPv4 address");
        return;
    };
    let curl = ["/usr/bin/curl", "/bin/curl"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("curl on this host");
    let perl = ["/usr/bin/perl", "/bin/perl"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("perl on this host");
    let listener = LanListener::start(perl, lan);
    assert!(
        listener.answers_outside_the_sandbox(),
        "the listener on {lan} does not answer outside the sandbox, so a refusal proves nothing"
    );
    let url = format!("http://{lan}:{}/", listener.port);
    let check = format!("{curl} --silent --show-error --max-time 5 --data hello {url}");
    let w = World::new(
        "p4v-sr3-lan",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        assert_eq!(
            listener.check_requests(),
            0,
            "SR-3 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    assert_eq!(
        listener.check_requests(),
        0,
        "SR-3: the check reached the listener on the host LAN address {url}\n{verdict}\n{}",
        w.evidence()
    );
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    let reason = verdict["reason"].as_str().unwrap();
    assert!(
        reason
            .lines()
            .any(|l| l.starts_with("check| curl: (7)") && l.contains(&lan.to_string())),
        "SR-3: the connect to {url} was not refused\n{verdict}"
    );
    assert_not_done(&w);
}

#[cfg(target_os = "macos")]
#[test]
fn sr3_check_that_connects_to_the_dns_daemon_is_refused() {
    const DNS_DAEMON: &str = "/var/run/mDNSResponder";
    assert!(
        std::os::unix::net::UnixStream::connect(DNS_DAEMON).is_ok(),
        "{DNS_DAEMON} does not answer outside the sandbox, so a refusal proves nothing"
    );
    let check = format!(
        "/usr/bin/perl -MIO::Socket::UNIX -e \
         'IO::Socket::UNIX->new(Peer => \"{DNS_DAEMON}\") or die \"connect: $!\\n\"'"
    );
    let w = World::new(
        "p4v-sr3-dns",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(
        verdict["decision"], "failed",
        "SR-3: the check reached {DNS_DAEMON}\n{verdict}"
    );
    let reason = verdict["reason"].as_str().unwrap();
    assert!(
        reason
            .lines()
            .any(|l| l == "check| connect: Operation not permitted"),
        "the verdict holds no refusal of the sandbox\n{verdict}"
    );
    assert_not_done(&w);
}
