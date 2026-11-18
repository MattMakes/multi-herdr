//! The command channel of fleetd: bind `<fleet root>/run/fleetd.sock` (a
//! pipe on Windows), check the peer user, read 1 request line with a
//! deadline, and write the reply of the recorded decision.
//!
//! Requirements: SR-5, SR-8, RR-8, FM-15. Ticket 13 of fleetd-p5 fills it.
//!
//! The server runs in 1 thread of fleetd with its own connection to the
//! ledger, and serves 1 connection at a time. Each connection gives 1
//! request:
//!
//! 1. It reads 1 line, at most `request_max_bytes` of `fleet.toml` and its
//!    `\n`, until [`READ_DEADLINE`]. A slow or silent client is closed at
//!    its deadline with no reply, so it holds the next client for that
//!    time at most (on Windows the channel has 1 pipe instance only).
//! 2. It parses the line. A line over the limit, or a line that does not
//!    parse, has no `request_id` that fleetd can trust, so fleetd records
//!    no row and replies `{"state": "refused", "reason"}` with no
//!    `request_id`.
//! 3. It collects the facts: the peer user (SR-5), the token (SR-8) and the
//!    peer process fact against the live attempts of the ledger (SR-8),
//!    and decides with [`authorize`].
//! 4. It records and applies the request with [`handle`] (RR-8, FM-15),
//!    and writes the reply line. Then it closes the connection.
//!
//! An error ends only that connection: a failed write of the ledger closes
//! it with no reply, and the client asks for the result of its
//! `request_id`. A lost generation ends the server, because each later
//! write of this fleetd is refused too.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::{self, AtName, Listener, Peer, Stream, User};
use horch_platform::NotSupported;
use rusqlite::{Connection, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::peer_process::{
    peer_in_live_attempt, AttemptProcess, Fact, PlatformFacts,
};
use crate::fleetd::fleet_runtime::adapters::request_wire::{parse_request, Request};
use crate::fleetd::fleet_runtime::adapters::{config_file, fleetd_row, token_file};
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use crate::fleetd::fleet_runtime::domain::authorize::{
    authorize, PeerProcess, RequestFacts, Source,
};
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::operator::AttemptFacts;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::domain::needs_you::Write as NeedsYouWrite;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::domain::task::Recorded;

/// The deadline of 1 connection: the read of its line, and on unix the
/// write of its reply. It is the connect time of today
/// (`preservation-05-contracts-and-values.md`, row V-18).
pub const READ_DEADLINE: Duration = Duration::from_secs(5);

/// The kind of the needs-you item of a file at the name of the channel
/// that is not a socket.
pub const NAME_TAKEN_KIND: &str = "channel_name_taken";

/// The time between 2 accepts after an accept fails, so that an error that
/// stays does not keep 1 core busy.
const ACCEPT_RETRY: Duration = Duration::from_millis(50);

/// The channel of the fleet at `fleet_root`.
pub fn socket_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

/// The options of the server. [`Options::default`] is the production
/// value; a test changes a field.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// The deadline of 1 connection.
    pub read_deadline: Duration,
    /// The peer user of a connection. The default reads [`Peer::user`]; a
    /// test gives another user (SR-5).
    pub peer_user: fn(&Peer) -> Result<User, NotSupported>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            read_deadline: READ_DEADLINE,
            peer_user: |peer| peer.user.clone(),
        }
    }
}

/// The running channel server. [`ChannelServer::stop`] ends it; a drop
/// also ends it.
#[derive(Debug)]
pub struct ChannelServer {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
}

impl ChannelServer {
    /// Bind the channel of `fleet_root` and serve it in 1 thread, with
    /// `ledger` as its own connection to the ledger. `guard` is the
    /// generation of this fleetd. The caller holds the fleetd lock.
    pub fn start(
        fleet_root: &Path,
        ledger: Connection,
        guard: Generation,
        handlers: Handlers,
    ) -> Result<ChannelServer> {
        ChannelServer::start_with(fleet_root, ledger, guard, handlers, Options::default())
    }

    /// [`ChannelServer::start`] with `options`.
    ///
    /// An old socket file at the name is removed first. Any other file at
    /// the name is kept: the start fails and opens 1 needs-you item.
    pub fn start_with(
        fleet_root: &Path,
        ledger: Connection,
        guard: Generation,
        handlers: Handlers,
        options: Options,
    ) -> Result<ChannelServer> {
        let config = config_file::load(fleet_root)?;
        let ledger_user = User::current().context("reading the user of fleetd")?;
        let socket = socket_path(fleet_root);
        free_the_name(&ledger, guard, &socket)?;
        let listener = Listener::bind(&socket)
            .with_context(|| format!("binding the channel {}", socket.display()))?;
        let server = Server {
            fleet_root: fleet_root.to_path_buf(),
            conn: ledger,
            guard,
            handlers,
            options,
            max_bytes: usize::try_from(config.request_max_bytes).unwrap_or(usize::MAX),
            seats: config.orchestrator_seats,
            ledger_user: ledger_user.to_string(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("fleetd-channel".into())
                .spawn(move || server.run(&listener, &stop))
                .context("starting the thread of the channel")?
        };
        Ok(ChannelServer {
            socket,
            stop,
            thread: Some(thread),
        })
    }

    /// End the server: it serves no new connection. The error is the end
    /// of the server before the stop, for example a lost generation.
    pub fn stop(mut self) -> Result<()> {
        self.end()
    }

    fn end(&mut self) -> Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept. A server that already ended refuses the connect.
        drop(Stream::connect(
            &self.socket,
            Instant::now() + READ_DEADLINE,
        ));
        thread
            .join()
            .map_err(|_| anyhow!("the thread of the channel panicked"))?
    }
}

impl Drop for ChannelServer {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// Remove the socket file of an old fleetd at `socket`. Refuse any other
/// file there, with 1 needs-you item.
fn free_the_name(conn: &Connection, guard: Generation, socket: &Path) -> Result<()> {
    let found = channel::at_name(socket)
        .with_context(|| format!("reading the channel name {}", socket.display()))?;
    match found {
        AtName::Free => Ok(()),
        AtName::OldSocket => fs::remove_file(socket)
            .with_context(|| format!("removing the old socket {}", socket.display())),
        AtName::Other => {
            let path = socket.display().to_string();
            let text = format!(
                "{path} is not a socket, so fleetd cannot open its command channel. \
                 Move the file away, then restart fleetd."
            );
            open_name_taken_item(conn, guard, &path, &text)?;
            bail!("{text}")
        }
    }
}

fn open_name_taken_item(
    conn: &Connection,
    guard: Generation,
    path: &str,
    text: &str,
) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    fleetd_row::check_generation(&tx, guard)?;
    let key = format!("{NAME_TAKEN_KIND}:{path}");
    needs_you::open_item(
        &tx,
        &NeedsYouWrite {
            key: &key,
            kind: NAME_TAKEN_KIND,
            entity: "fleetd",
            entity_id: path,
            text,
        },
    )?;
    tx.commit()?;
    Ok(())
}

/// The state of the thread of the channel.
struct Server {
    fleet_root: PathBuf,
    conn: Connection,
    guard: Generation,
    handlers: Handlers,
    options: Options,
    max_bytes: usize,
    seats: Vec<String>,
    ledger_user: String,
}

impl Server {
    fn run(mut self, listener: &Listener, stop: &AtomicBool) -> Result<()> {
        loop {
            let accepted = listener.accept();
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let Ok((stream, peer)) = accepted else {
                thread::sleep(ACCEPT_RETRY);
                continue;
            };
            // Each other error ends only this connection.
            if let Err(lost @ DoorError::GenerationLost { .. }) = self.serve(&stream, &peer) {
                return Err(lost.into());
            }
        }
    }

    /// Serve 1 connection: 1 line, the decision, 1 reply.
    fn serve(&mut self, stream: &Stream, peer: &Peer) -> Result<(), DoorError> {
        let deadline = Instant::now() + self.options.read_deadline;
        let line = match read_line(stream, self.max_bytes, deadline) {
            Ok(Line::Read(line)) if line.is_empty() => return Ok(()),
            Ok(Line::Read(line)) => line,
            Ok(Line::OverLimit) => {
                let reason = format!(
                    "the request line is over the limit of {} bytes",
                    self.max_bytes
                );
                write_reply(stream, &refusal_line(&reason), self.options.read_deadline);
                return Ok(());
            }
            // A silent or slow client, or a client that went away.
            Err(_) => return Ok(()),
        };
        let request = match parse_request(&line, self.max_bytes as u64) {
            Ok(request) => request,
            Err(e) => {
                write_reply(
                    stream,
                    &refusal_line(&e.to_string()),
                    self.options.read_deadline,
                );
                return Ok(());
            }
        };
        let facts = self.facts(&request, peer);
        let decision = authorize(&facts);
        let reply = handle(
            &mut self.conn,
            self.guard,
            &request,
            facts.caller.as_deref(),
            &decision,
            &self.handlers,
        )?;
        write_reply(stream, &reply.to_line(), self.options.read_deadline);
        Ok(())
    }

    fn facts(&self, request: &Request, peer: &Peer) -> RequestFacts {
        let token_valid = request.token.as_deref().is_some_and(|text| {
            token_file::read(&self.fleet_root).is_ok_and(|token| token.matches(text))
        });
        RequestFacts {
            kind: request.kind.as_str().to_string(),
            source: Source::Channel {
                peer: peer_process(&self.conn, peer),
            },
            caller: (self.options.peer_user)(peer).ok().map(|u| u.to_string()),
            ledger_user: Some(self.ledger_user.clone()),
            token_valid,
            orchestrator_seats: self.seats.clone(),
        }
    }
}

/// The line of a connection.
enum Line {
    /// The bytes before the `\n`, or before the end of the stream.
    Read(Vec<u8>),
    /// More than `max_bytes` bytes and no `\n` in them.
    OverLimit,
}

/// Read 1 line of `stream`: at most `max_bytes` and its `\n`, so never more
/// than `max_bytes + 1` bytes, until `deadline`. A read after `deadline`
/// fails with `TimedOut`.
fn read_line(stream: &Stream, max_bytes: usize, deadline: Instant) -> io::Result<Line> {
    let limit = max_bytes.saturating_add(1);
    let mut line = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut reader = stream;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(left)?;
        let room = (limit - line.len()).min(chunk.len());
        let n = match reader.read(&mut chunk[..room]) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            return Ok(Line::Read(line));
        }
        if let Some(end) = chunk[..n].iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&chunk[..end]);
            return Ok(Line::Read(line));
        }
        line.extend_from_slice(&chunk[..n]);
        if line.len() >= limit {
            return Ok(Line::OverLimit);
        }
    }
}

/// The reply to a line with no `request_id` that fleetd can trust.
fn refusal_line(reason: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({ "state": "refused", "reason": reason })
    )
}

/// Write `line` to `stream` and wait at most `timeout` for each write. A
/// failed write ends only this connection. Windows has no write timeout
/// on a pipe, so there the write waits until the client reads.
fn write_reply(stream: &Stream, line: &str, timeout: Duration) {
    if let Err(e) = stream.set_write_timeout(timeout) {
        if e.kind() != io::ErrorKind::Unsupported {
            return;
        }
    }
    let mut writer = stream;
    let _ = writer
        .write_all(line.as_bytes())
        .and_then(|()| writer.flush());
}

/// Is the process of `peer` in a live attempt. A fact that is not
/// supported or that fails to read is [`PeerProcess::Unknown`].
fn peer_process(conn: &Connection, peer: &Peer) -> PeerProcess {
    let Ok(pid) = peer.pid else {
        return PeerProcess::Unknown;
    };
    let Ok(Some(live)) = live_attempts(conn) else {
        return PeerProcess::Unknown;
    };
    let processes: Vec<AttemptProcess> = live.iter().map(|(p, _)| p.clone()).collect();
    match peer_in_live_attempt(pid, &processes, &PlatformFacts) {
        Fact::No => PeerProcess::Outside,
        Fact::Unknown => PeerProcess::Unknown,
        Fact::Yes(ids) => PeerProcess::Inside(
            ids.iter()
                .filter_map(|id| live.iter().find(|(p, _)| &p.attempt == id))
                .map(|(_, facts)| facts.clone())
                .collect(),
        ),
    }
}

/// The process and the facts of each attempt that is not ended and has a
/// pid. `None` when such an attempt has no valid pid or start time: its
/// process fact is unknown.
fn live_attempts(conn: &Connection) -> Result<Option<Vec<(AttemptProcess, AttemptFacts)>>> {
    let mut live = Vec::new();
    for (row, persona) in attempts_store::open_attempts_with_persona(conn)? {
        let Some(Recorded {
            pid,
            pid_start: Some(start),
        }) = row.process
        else {
            return Ok(None);
        };
        live.push((
            AttemptProcess {
                attempt: row.id.clone(),
                wrapper_pid: pid,
                wrapper_start: start,
            },
            AttemptFacts {
                attempt: row.id,
                task: row.task_id,
                persona: persona.unwrap_or_default(),
            },
        ));
    }
    Ok(Some(live))
}
