use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::{self, AtName, Listener, Peer, Stream, User};
use horch_platform::NotSupported;
use rusqlite::{Connection, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, recorded_processes, Fact, PlatformFacts,
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

pub const READ_DEADLINE: Duration = Duration::from_secs(5);

pub const NAME_TAKEN_KIND: &str = "channel_name_taken";

const ACCEPT_RETRY: Duration = Duration::from_millis(50);

const STOP_POLL: Duration = Duration::from_millis(50);

const WAKE_WAIT: Duration = Duration::from_millis(100);

const WAKE_POLL: Duration = Duration::from_millis(10);

const STOP_MARGIN: Duration = Duration::from_secs(5);

const DRAIN_QUIET: Duration = Duration::from_millis(100);

pub fn socket_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub read_deadline: Duration,
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

#[derive(Debug)]
pub struct ChannelServer {
    socket: PathBuf,
    read_deadline: Duration,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
    acceptor: Option<JoinHandle<()>>,
}

impl ChannelServer {
    pub fn start(
        fleet_root: &Path,
        ledger: Connection,
        guard: Generation,
        handlers: Handlers,
    ) -> Result<ChannelServer> {
        ChannelServer::start_with(fleet_root, ledger, guard, handlers, Options::default())
    }

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
            ledger_user: ledger_user.to_string(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (accepted, connections) = mpsc::sync_channel(0);
        let acceptor = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("fleetd-channel-accept".into())
                .spawn(move || accept_loop(&listener, &stop, &accepted))
                .context("starting the accept thread of the channel")?
        };
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("fleetd-channel".into())
                .spawn(move || server.run(&connections, &stop))
        };
        let mut server = ChannelServer {
            socket,
            read_deadline: options.read_deadline,
            stop,
            thread: None,
            acceptor: Some(acceptor),
        };
        match thread {
            Ok(thread) => server.thread = Some(thread),
            Err(e) => {
                server.stop.store(true, Ordering::SeqCst);
                server.wake_acceptor();
                return Err(e).context("starting the thread of the channel");
            }
        }
        Ok(server)
    }

    pub fn stop(mut self) -> Result<()> {
        self.end()
    }

    fn end(&mut self) -> Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        self.stop.store(true, Ordering::SeqCst);
        let limit = Instant::now() + self.read_deadline * 2 + STOP_MARGIN;
        while !thread.is_finished() {
            if Instant::now() >= limit {
                self.wake_acceptor();
                bail!("the thread of the channel did not end at the stop");
            }
            thread::sleep(WAKE_POLL);
        }
        let ended = thread
            .join()
            .map_err(|_| anyhow!("the thread of the channel panicked"));
        self.wake_acceptor();
        ended?
    }

    fn wake_acceptor(&mut self) {
        let Some(acceptor) = self.acceptor.take() else {
            return;
        };
        let limit = Instant::now() + STOP_MARGIN;
        while !acceptor.is_finished() {
            if Instant::now() >= limit {
                return;
            }
            match Stream::connect(&self.socket, Instant::now() + WAKE_WAIT) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return,
                other => drop(other),
            }
            let next = Instant::now() + WAKE_WAIT;
            while !acceptor.is_finished() && Instant::now() < next {
                thread::sleep(WAKE_POLL);
            }
        }
        let _ = acceptor.join();
    }
}

impl Drop for ChannelServer {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

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

fn accept_loop(listener: &Listener, stop: &AtomicBool, accepted: &SyncSender<(Stream, Peer)>) {
    loop {
        let connection = listener.accept();
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match connection {
            Ok(connection) => {
                if accepted.send(connection).is_err() {
                    return;
                }
            }
            Err(_) => thread::sleep(ACCEPT_RETRY),
        }
    }
}

struct Server {
    fleet_root: PathBuf,
    conn: Connection,
    guard: Generation,
    handlers: Handlers,
    options: Options,
    max_bytes: usize,
    ledger_user: String,
}

impl Server {
    fn run(mut self, connections: &Receiver<(Stream, Peer)>, stop: &AtomicBool) -> Result<()> {
        loop {
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let (stream, peer) = match connections.recv_timeout(STOP_POLL) {
                Ok(connection) => connection,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("the accept thread of the channel ended")
                }
            };
            if let Err(lost @ DoorError::GenerationLost { .. }) = self.serve(&stream, &peer) {
                return Err(lost.into());
            }
        }
    }

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
                drain_input(stream, self.options.read_deadline);
                return Ok(());
            }
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
        }
    }
}

enum Line {
    Read(Vec<u8>),
    OverLimit,
}

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

fn refusal_line(reason: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({ "state": "refused", "reason": reason })
    )
}

fn write_reply(stream: &Stream, line: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut rest = line.as_bytes();
    let mut writer = stream;
    while !rest.is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_write_timeout(left).is_err() {
            return;
        }
        match writer.write(rest) {
            Ok(0) => return,
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
    let _ = writer.flush();
}

fn drain_input(stream: &Stream, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut chunk = [0u8; 4096];
    let mut reader = stream;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(left.min(DRAIN_QUIET)).is_err() {
            return;
        }
        match reader.read(&mut chunk) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

fn peer_process(conn: &Connection, peer: &Peer) -> PeerProcess {
    let Ok(pid) = peer.pid else {
        return PeerProcess::Unknown;
    };
    let Ok(Some((recorded, rows))) = recorded_processes(conn) else {
        return PeerProcess::Unknown;
    };
    match fleet_process(pid, &recorded, &PlatformFacts) {
        Fact::No => PeerProcess::Outside,
        Fact::Unknown => PeerProcess::Unknown,
        Fact::Yes(ids) => PeerProcess::Inside(
            ids.iter()
                .filter_map(|id| rows.iter().find(|r| &r.attempt == id))
                .map(|r| AttemptFacts {
                    attempt: r.attempt.clone(),
                    task: r.task.clone(),
                    persona: r.persona.clone().unwrap_or_default(),
                })
                .collect(),
        ),
    }
}
