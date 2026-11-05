//! The report socket of a `horch_access: report` pane
//! (`docs/security/sandboxed-pane-horch-access.md`, C1).
//!
//! The sandboxed agent in such a pane reaches no horch state: no state root,
//! no ledger folder, no relay or herdr socket. Its `horch worker` parent,
//! which runs outside the sandbox, serves 1 Unix socket for the pane, and
//! the `horch` CLI in the pane talks only to it ([`ENV`]).
//!
//! Protocol: 1 request per connection. The client writes 1 JSON line of at
//! most [`MAX_REQUEST`] bytes, then reads 1 JSON [`Reply`] line.
//! - `{"verb":"progress","text":..}`: the parent records ledger progress
//!   for its own worker row.
//! - `{"verb":"tell","to":..,"text":..}`: the parent tells the brief's
//!   report target, tagged with the brief's role. Any other `to` is refused.
//! - `{"verb":"done","text":..}`: the parent runs `horch done` for its own
//!   record.
//!
//! Every other verb or key, a line over the limit and an empty text are
//! refused. The role and record come from the parent, never from the pane.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::thread::JoinHandle;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use anyhow::Context;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// The variable that puts the `horch` CLI in report mode. Its value is the
/// socket path.
pub const ENV: &str = "HORCH_REPORT_SOCKET";

/// The largest request line, in bytes, without its newline: 16 KiB. A
/// `horch done` summary is 1 paragraph, and a longer `tell` is spooled to a
/// file by the parent.
pub const MAX_REQUEST: usize = 16 * 1024;

/// How long the server waits for 1 request line.
#[cfg(unix)]
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// The socket of the pane whose ledger record is `record_id`.
pub fn socket_path(state_root: &Path, record_id: &str) -> PathBuf {
    state_root
        .join("report")
        .join(format!("{}.sock", super::file_safe(record_id)))
}

/// 1 request from the pane.
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

/// The server's answer to 1 request.
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

/// Carries out a checked request in the parent.
pub type Executor = dyn Fn(&Request) -> Result<()> + Send + Sync;

/// Check 1 request line and run it. `report_to` is the only role `tell`
/// may name; `None` refuses every `tell`.
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

#[cfg(unix)]
/// Read 1 request line of at most [`MAX_REQUEST`] bytes plus its newline.
/// A longer line comes back longer than the limit, so [`handle`] refuses it.
fn read_request(stream: &UnixStream) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    BufReader::new(stream.take(MAX_REQUEST as u64 + 2)).read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    Ok(line)
}

#[cfg(unix)]
fn serve_one(stream: UnixStream, report_to: Option<&str>, exec: &Executor) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let reply = match read_request(&stream) {
        Ok(line) => handle(&line, report_to, exec),
        Err(e) => Reply::refused(format!("reading the request: {e}")),
    };
    let mut text = serde_json::to_string(&reply).unwrap_or_default();
    text.push('\n');
    let _ = (&stream).write_all(text.as_bytes());
}

/// The parent's socket server. It stops and removes the socket on drop.
#[cfg(unix)]
pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(unix)]
impl Server {
    /// Bind `path` (mode 0600, in a 0700 folder) and serve it on a thread,
    /// 1 connection at a time.
    pub fn start(path: PathBuf, report_to: Option<String>, exec: Box<Executor>) -> Result<Server> {
        let dir = path.parent().context("the report socket has no folder")?;
        crate::fsx::ensure_private_dir(dir)?;
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing the old socket {}", path.display()))?;
        }
        let listener = UnixListener::bind(&path)
            .with_context(|| format!("binding the report socket {}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    serve_one(stream, report_to.as_deref(), exec.as_ref());
                }
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

#[cfg(unix)]
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked accept, so the thread sees the flag.
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Send 1 request to the socket at `path` and return the server's answer
/// as a result.
#[cfg(not(unix))]
pub fn send(_path: &Path, _request: &Request) -> Result<()> {
    bail!("the report socket needs a Unix host")
}

/// Send 1 request to the socket at `path` and return the server's answer
/// as a result.
#[cfg(unix)]
pub fn send(path: &Path, request: &Request) -> Result<()> {
    let mut stream = UnixStream::connect(path)
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

    /// An executor that records each request it runs.
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

    #[test]
    fn each_verb_runs_once() {
        let (seen, exec) = recorder();
        for v in [
            serde_json::json!({"verb": "progress", "text": "step 1"}),
            serde_json::json!({"verb": "tell", "to": "orchestrator", "text": "hi"}),
            serde_json::json!({"verb": "done", "text": "finished"}),
        ] {
            let reply = handle(&line(v), Some("orchestrator"), exec.as_ref());
            assert_eq!(
                reply,
                Reply {
                    ok: true,
                    error: None
                }
            );
        }
        assert_eq!(
            *seen.lock().unwrap(),
            [
                Request::Progress {
                    text: "step 1".into()
                },
                Request::Tell {
                    to: "orchestrator".into(),
                    text: "hi".into()
                },
                Request::Done {
                    text: "finished".into()
                },
            ]
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
            &line(serde_json::json!({"verb": "progress", "text": "x"})),
            None,
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

    /// A Unix socket path has a short length limit, so the test folder is
    /// under `/tmp`, not the long macOS temp folder.
    #[cfg(unix)]
    #[test]
    fn the_server_answers_over_the_socket_and_removes_it_on_drop() {
        let tmp = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let path = socket_path(tmp.path(), "r1");
        let (seen, exec) = recorder();
        let server = Server::start(path.clone(), Some("orchestrator".into()), exec).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        send(
            &path,
            &Request::Progress {
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
            &Request::Done {
                text: "x".repeat(MAX_REQUEST),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("over 16384 bytes"), "{err}");
        // A raw oversize line is refused by the server too.
        let mut raw = UnixStream::connect(&path).unwrap();
        let mut long = vec![b'x'; MAX_REQUEST + 10];
        long.push(b'\n');
        // The server stops reading at the limit, so the tail can fail.
        let _ = raw.write_all(&long);
        let mut reply = String::new();
        BufReader::new(raw).read_line(&mut reply).unwrap();
        assert!(reply.contains("over 16384 bytes"), "{reply}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        drop(server);
        assert!(!path.exists());
    }
}
