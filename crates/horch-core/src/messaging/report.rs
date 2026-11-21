use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use horch_platform::ipc::{LocalListener, LocalStream};
use serde::{Deserialize, Serialize};

use super::brief::Brief;
use crate::execution::ReportTarget;
use crate::fleetd::fleet_runtime::adapters::config_file;
use crate::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts;
use crate::fleetd::task_execution::adapters::outbox_writer::OutboxWriter;
use crate::fleetd::task_execution::domain::outbox::LineKind;
use crate::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use crate::taskledger::store::Store;

pub const ENV: &str = "HORCH_REPORT_SOCKET";

pub const MAX_REQUEST: usize = 16 * 1024;

const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub fn socket_path(state_root: &Path, record_id: &str) -> PathBuf {
    state_root
        .join("report")
        .join(format!("{}.sock", super::file_safe(record_id)))
}

pub const OUTBOX_VERBS: &str =
    "progress and done write the outbox of the attempt; this socket takes only tell";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Progress { text: String },
    Tell { to: String, text: String },
    Done { text: String },
}

impl Request {
    fn text(&self) -> &str {
        match self {
            Request::Progress { text } | Request::Tell { text, .. } | Request::Done { text } => {
                text
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    fn refused(error: impl Into<String>) -> Reply {
        Reply {
            ok: false,
            error: Some(error.into()),
        }
    }
}

pub fn target(brief: &Brief) -> Option<String> {
    if let Some(controller) = &brief.controller {
        return Some(controller.clone());
    }
    (brief.report_to == ReportTarget::Orchestrator).then(|| "orchestrator".to_string())
}

pub type Executor = dyn Fn(&Request) -> Result<()> + Send + Sync;

pub fn handle(line: &[u8], report_to: Option<&str>, exec: &Executor) -> Reply {
    if line.len() > MAX_REQUEST {
        return Reply::refused(format!(
            "the request is over {MAX_REQUEST} bytes; send a shorter text"
        ));
    }
    let request: Request = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(e) => {
            return Reply::refused(format!(
                "not a report request ({e}); only progress, tell and done work in this pane"
            ))
        }
    };
    if request.text().trim().is_empty() {
        return Reply::refused("the text is empty");
    }
    if !matches!(request, Request::Tell { .. }) {
        return Reply::refused(OUTBOX_VERBS);
    }
    if let Request::Tell { to, .. } = &request {
        match report_to {
            Some(target) if target == to => {}
            Some(target) => {
                return Reply::refused(format!("this pane reports only to {target}, not to {to}"))
            }
            None => return Reply::refused("this pane has no report target"),
        }
    }
    match exec(&request) {
        Ok(()) => Reply {
            ok: true,
            error: None,
        },
        Err(e) => Reply::refused(format!("{e:#}")),
    }
}

fn read_request(stream: &LocalStream) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    BufReader::new(stream.take(MAX_REQUEST as u64 + 2)).read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    Ok(line)
}

fn serve_one(stream: LocalStream, report_to: Option<&str>, exec: &Executor) {
    let _ = stream.set_read_timeout(READ_TIMEOUT);
    let reply = match read_request(&stream) {
        Ok(line) => handle(&line, report_to, exec),
        Err(e) => Reply::refused(format!("reading the request: {e}")),
    };
    let mut text = serde_json::to_string(&reply).unwrap_or_default();
    text.push('\n');
    let _ = (&stream).write_all(text.as_bytes());
}

pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(path: PathBuf, report_to: Option<String>, exec: Box<Executor>) -> Result<Server> {
        if !horch_platform::ipc::is_supported() {
            bail!("the report socket needs a Unix host");
        }
        let dir = path.parent().context("the report socket has no folder")?;
        crate::fsx::ensure_private_dir(dir)?;
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing the old socket {}", path.display()))?;
        }
        let listener = LocalListener::bind(&path)
            .with_context(|| format!("binding the report socket {}", path.display()))?;
        horch_platform::fs::set_mode(&path, 0o600)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || loop {
            let stream = listener.accept();
            if flag.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(stream) = stream {
                serve_one(stream, report_to.as_deref(), exec.as_ref());
            }
        });
        Ok(Server {
            path,
            stop,
            thread: Some(thread),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = LocalStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn report(
    socket: &Path,
    request: &Request,
    outbox: impl FnOnce() -> Result<OutboxWriter>,
    at: &str,
) -> Result<()> {
    if request.text().trim().is_empty() {
        bail!("the text is empty");
    }
    match request {
        Request::Progress { text } => outbox()?.append(LineKind::Progress, at, text),
        Request::Done { text } => {
            let writer = outbox()?;
            writer.write_result(&completed(&writer, text))?;
            Ok(())
        }
        Request::Tell { .. } => send(socket, request),
    }
}

fn completed(writer: &OutboxWriter, summary: &str) -> ResultFile {
    ResultFile {
        task: writer.attempt().task_id.clone(),
        attempt: writer.attempt().attempt().to_string(),
        status: ResultStatus::Completed,
        summary: summary.to_string(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
        unlanded: None,
    }
}

pub fn own_outbox(ledger_db: &Path, fleet_root: &Path) -> Result<OutboxWriter> {
    let ledger = Store::open_read_only(ledger_db)?;
    let config = config_file::load(fleet_root)?;
    OutboxWriter::of_this_process(fleet_root, ledger.conn(), &config, &PlatformFacts)
}

pub fn send(path: &Path, request: &Request) -> Result<()> {
    if !horch_platform::ipc::is_supported() {
        bail!("the report socket needs a Unix host");
    }
    let mut stream = LocalStream::connect(path)
        .with_context(|| format!("connecting to the report socket {}", path.display()))?;
    let mut line = serde_json::to_vec(request)?;
    if line.len() > MAX_REQUEST {
        bail!("the text is over {MAX_REQUEST} bytes; send a shorter text");
    }
    line.push(b'\n');
    stream.write_all(&line)?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.is_empty() {
        bail!("the report socket closed without an answer");
    }
    let reply: Reply = serde_json::from_str(&reply).context("reading the report answer")?;
    match reply.error {
        None if reply.ok => Ok(()),
        error => bail!("{}", error.unwrap_or_else(|| "refused".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::{BufRead, BufReader, Write};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};

    fn recorder() -> (Arc<Mutex<Vec<Request>>>, Box<Executor>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let exec: Box<Executor> = Box::new(move |r: &Request| {
            log.lock().unwrap().push(r.clone());
            Ok(())
        });
        (seen, exec)
    }

    fn line(v: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&v).unwrap()
    }

    fn brief(report_to: crate::execution::ReportTarget, controller: Option<&str>) -> Brief {
        let raw = serde_json::json!({
            "role": "intake-auditor-1", "teammate": "intake-auditor", "agent": "claude",
            "model": "opus", "record_id": "r1", "session_id": "", "resume": false,
            "project_dir": "/p", "report_to": report_to, "controller": controller,
        });
        Brief::from_json(&raw.to_string()).unwrap()
    }

    #[test]
    fn the_controller_is_the_only_tell_target_of_its_roles() {
        use crate::execution::ReportTarget;
        let spawned = brief(ReportTarget::Orchestrator, Some("skill-importer-1"));
        assert_eq!(target(&spawned).as_deref(), Some("skill-importer-1"));
        let tell = |to: &str| line(serde_json::json!({"verb": "tell", "to": to, "text": "x"}));
        let (seen, exec) = recorder();
        let to = target(&spawned);
        assert!(handle(&tell("skill-importer-1"), to.as_deref(), exec.as_ref()).ok);
        let reply = handle(&tell("orchestrator"), to.as_deref(), exec.as_ref());
        assert_eq!(
            reply.error.as_deref(),
            Some("this pane reports only to skill-importer-1, not to orchestrator")
        );
        assert_eq!(seen.lock().unwrap().len(), 1);

        let plain = brief(ReportTarget::Orchestrator, None);
        assert_eq!(target(&plain).as_deref(), Some("orchestrator"));
        assert_eq!(target(&brief(ReportTarget::None, None)), None);
    }

    #[test]
    fn tell_runs_once() {
        let (seen, exec) = recorder();
        let tell = serde_json::json!({"verb": "tell", "to": "orchestrator", "text": "hi"});
        let reply = handle(&line(tell), Some("orchestrator"), exec.as_ref());
        assert_eq!(
            reply,
            Reply {
                ok: true,
                error: None
            }
        );
        assert_eq!(
            *seen.lock().unwrap(),
            [Request::Tell {
                to: "orchestrator".into(),
                text: "hi".into()
            }]
        );
    }

    #[test]
    fn an_unknown_verb_or_key_is_refused() {
        let (seen, exec) = recorder();
        for v in [
            serde_json::json!({"verb": "spawn", "text": "opus"}),
            serde_json::json!({"verb": "progress", "text": "x", "role": "orchestrator"}),
            serde_json::json!({"text": "x"}),
        ] {
            let reply = handle(&line(v), Some("orchestrator"), exec.as_ref());
            assert!(!reply.ok);
            assert!(
                reply.error.unwrap().contains("not a report request"),
                "refusal names the cause"
            );
        }
        let reply = handle(b"progress hi", Some("orchestrator"), exec.as_ref());
        assert!(!reply.ok);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_oversize_request_or_an_empty_text_is_refused() {
        let (seen, exec) = recorder();
        let text = "x".repeat(MAX_REQUEST);
        let reply = handle(
            &line(serde_json::json!({"verb": "progress", "text": text})),
            Some("orchestrator"),
            exec.as_ref(),
        );
        assert!(reply.error.unwrap().contains("over 16384 bytes"));
        let reply = handle(
            &line(serde_json::json!({"verb": "done", "text": "  "})),
            Some("orchestrator"),
            exec.as_ref(),
        );
        assert_eq!(reply.error.as_deref(), Some("the text is empty"));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn tell_reaches_only_the_report_target() {
        let (seen, exec) = recorder();
        let tell = line(serde_json::json!({"verb": "tell", "to": "intake-writer", "text": "x"}));
        let reply = handle(&tell, Some("orchestrator"), exec.as_ref());
        assert_eq!(
            reply.error.as_deref(),
            Some("this pane reports only to orchestrator, not to intake-writer")
        );
        let tell = line(serde_json::json!({"verb": "tell", "to": "orchestrator", "text": "x"}));
        let reply = handle(&tell, None, exec.as_ref());
        assert_eq!(
            reply.error.as_deref(),
            Some("this pane has no report target")
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_executor_error_reaches_the_client() {
        let exec: Box<Executor> = Box::new(|_: &Request| bail!("the ledger is locked"));
        let reply = handle(
            &line(serde_json::json!({"verb": "tell", "to": "orchestrator", "text": "x"})),
            Some("orchestrator"),
            exec.as_ref(),
        );
        assert_eq!(reply.error.as_deref(), Some("the ledger is locked"));
    }

    #[test]
    fn the_socket_path_is_per_record_under_the_state_root() {
        assert_eq!(
            socket_path(Path::new("/s"), "r1"),
            PathBuf::from("/s/report/r1.sock")
        );
        assert_eq!(
            socket_path(Path::new("/s"), "../x"),
            PathBuf::from("/s/report/___x.sock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_closed_socket_is_an_error() {
        let tmp = tempfile::Builder::new()
            .tempdir_in("/tmp")
            .or_else(|_| tempfile::tempdir())
            .unwrap();
        let path = tmp.path().join("r.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mute = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
        });
        let tell = Request::Tell {
            to: "orchestrator".into(),
            text: "x".into(),
        };
        let err = send(&path, &tell).unwrap_err();
        assert!(
            err.to_string().contains("closed without an answer"),
            "{err}"
        );
        mute.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_server_answers_over_the_socket_and_removes_it_on_drop() {
        let tmp = tempfile::Builder::new()
            .tempdir_in("/tmp")
            .or_else(|_| tempfile::tempdir())
            .unwrap();
        let path = socket_path(tmp.path(), "r1");
        let (seen, exec) = recorder();
        let server = Server::start(path.clone(), Some("orchestrator".into()), exec).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        send(
            &path,
            &Request::Tell {
                to: "orchestrator".into(),
                text: "step 1".into(),
            },
        )
        .unwrap();
        let err = send(
            &path,
            &Request::Tell {
                to: "sonnet-1".into(),
                text: "x".into(),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("reports only to orchestrator"),
            "{err}"
        );
        let err = send(
            &path,
            &Request::Tell {
                to: "orchestrator".into(),
                text: "x".repeat(MAX_REQUEST),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("over 16384 bytes"), "{err}");
        let mut raw = UnixStream::connect(&path).unwrap();
        let mut long = vec![b'x'; MAX_REQUEST + 10];
        long.push(b'\n');
        let _ = raw.write_all(&long);
        let mut reply = String::new();
        BufReader::new(raw).read_line(&mut reply).unwrap();
        assert!(reply.contains("over 16384 bytes"), "{reply}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        drop(server);
        assert!(!path.exists());
    }
}
