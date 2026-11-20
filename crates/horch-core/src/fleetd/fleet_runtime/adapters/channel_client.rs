//! The client of the channel: send 1 request, check that the peer is the
//! fleetd of the `fleetd` row, ask for the result after a lost reply, or
//! write a request file when fleetd is down.
//!
//! Requirements: RR-8, SR-5, SR-8, FM-15. Ticket 15 of fleetd-p5.
//!
//! [`send`] makes 1 `request_id` for each call and keeps it for each retry
//! of that call. Before each send it checks that the process of the server
//! (its PID and its start time) is the process of the `fleetd` row. A
//! mismatch or an unknown fact sends nothing: a process of the same user
//! can remove the socket file and bind its own. After a lost reply, it
//! reads the `requests` row of its `request_id` before it sends again.
//!
//! The wrapper of a worker (`horch worker`) and each process of a live
//! attempt send no request and never read the operator token (T-899): the
//! facts of a wrapper go by the files of its attempt. This refusal comes
//! before the token read, but it is not the boundary. The peer process
//! check of fleetd and the worker sandbox stay the boundary.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::Stream;
use horch_platform::fs as platform;
use serde_json::{Map, Value};

use crate::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, recorded_processes, Fact, PlatformFacts,
};
use crate::fleetd::fleet_runtime::adapters::request_wire::{parse_reply, Outcome, Reply, Request};
use crate::fleetd::fleet_runtime::adapters::{config_file, requests_store, token_file};
use crate::fleetd::fleet_runtime::domain::requests::{check_size, Class, Kind, RequestId};
use crate::runtime::RuntimeContext;
use crate::taskledger::store::{self, Store};

/// The longest wait for the connect: the connect time of today
/// (`preservation-05-contracts-and-values.md`, row V-18).
const CONNECT_WAIT: Duration = Duration::from_secs(5);

/// The longest wait for the reply of 1 send, and for the write of the
/// request line. A send past it is a lost reply, so a fleetd that accepts
/// and never answers cannot hold a command with no end.
const REPLY_WAIT: Duration = Duration::from_secs(30);

/// The most sends of 1 request. A send after the first comes only after a
/// lost reply, when the `requests` row is missing or `pending`.
const MAX_SENDS: usize = 3;

/// This process is the wrapper of a worker ([`mark_wrapper_process`]).
static WRAPPER: AtomicBool = AtomicBool::new(false);

/// Mark this process as the wrapper of a worker (`horch worker`). From
/// then on, [`send`] refuses each request before it reads the operator
/// token, and [`is_wrapper_process`] is true.
pub fn mark_wrapper_process() {
    WRAPPER.store(true, Ordering::SeqCst);
}

/// True when this process is the wrapper of a worker.
pub fn is_wrapper_process() -> bool {
    WRAPPER.load(Ordering::SeqCst)
}

/// What became of 1 request.
#[derive(Debug)]
pub enum Sent {
    /// fleetd replied, or the `requests` row gave the stored result after a
    /// lost reply.
    Replied(Reply),
    /// fleetd is down: the request waits in the request file `path`, and
    /// fleetd applies it at start.
    Queued {
        request_id: RequestId,
        path: PathBuf,
    },
}

/// Send 1 `operator` request of `kind` with `payload` to fleetd, with the
/// operator token. It sends nothing to a server that is not the fleetd of
/// the `fleetd` row. When fleetd is down, it writes the request file. The
/// wrapper of a worker and a process of a live attempt send nothing and
/// do not read the token ([`refuse_fleet_caller`]).
pub fn send(ctx: &RuntimeContext, kind: Kind, payload: Map<String, Value>) -> Result<Sent> {
    if kind.class() != Class::Operator {
        bail!(
            "the kind {} is of class worker: a worker kind never uses the channel or the request folder",
            kind.as_str()
        );
    }
    refuse_fleet_caller(ctx, kind)?;
    let root = &ctx.paths.fleet_root;
    let max_bytes = config_file::load(root)?.request_max_bytes;
    let token = token_file::read(root).context("reading the operator token")?;
    let request = Request {
        request_id: new_request_id(),
        kind,
        payload,
        token: Some(token.text().to_string()),
    };
    let id = request.request_id.clone();
    let line = request.to_line();
    check_size(line.len(), max_bytes).map_err(|e| anyhow!("request {id}: {e}"))?;
    let channel = channel_path(root);
    for _ in 0..MAX_SENDS {
        let stream = match Stream::connect(&channel, Instant::now() + CONNECT_WAIT) {
            Ok(stream) => stream,
            Err(e) if is_down(&e) => return queue(root, &id, &line),
            Err(e) => {
                return Err(anyhow!(e))
                    .with_context(|| format!("request {id}: connecting to {}", channel.display()))
            }
        };
        check_server(ctx, &stream, &channel)?;
        if let Some(reply) = exchange(&stream, &line, max_bytes, &id)? {
            return Ok(Sent::Replied(reply));
        }
        // The connection ended with no reply: ask for the result first.
        if let Some(reply) = stored_result(ctx, &request)? {
            return Ok(Sent::Replied(reply));
        }
    }
    bail!(
        "request {id} got no reply after {MAX_SENDS} sends; `horch request {id}` prints its state"
    )
}

/// Refuse the `operator` request `kind` of the wrapper of a worker, or of
/// a process that the ledger of `ctx` shows in a live attempt
/// ([`fleet_process`]). An unknown fact does not refuse here: fleetd
/// refuses it at the channel.
fn refuse_fleet_caller(ctx: &RuntimeContext, kind: Kind) -> Result<()> {
    let kind = kind.as_str();
    if is_wrapper_process() {
        bail!(
            "the wrapper of a worker sends no operator request ({kind}): its facts go by the files of its attempt"
        );
    }
    if let Fact::Yes(attempts) = own_fact(ctx) {
        bail!(
            "refused inside a fleet attempt: this process is a fleet process of attempt {}, so it sends no operator request ({kind}). Run it yourself from a terminal outside the fleet.",
            attempts.join(", ")
        );
    }
    Ok(())
}

/// The fleet process fact of this process in the ledger of `ctx`. A
/// missing ledger or a fact that fails to read is [`Fact::Unknown`] or
/// [`Fact::No`], and neither refuses.
fn own_fact(ctx: &RuntimeContext) -> Fact {
    let Ok(Some(store)) = crate::taskledger::open_existing_read_only(ctx) else {
        return Fact::No;
    };
    match recorded_processes(store.conn()) {
        Ok(Some((recorded, _))) => fleet_process(std::process::id(), &recorded, &PlatformFacts),
        Ok(None) | Err(_) => Fact::Unknown,
    }
}

/// A new `request_id`: 1 UUID v4.
fn new_request_id() -> RequestId {
    RequestId::parse(&uuid::Uuid::new_v4().to_string()).expect("a UUID meets the request_id rule")
}

/// The command channel of the fleet at `fleet_root`.
fn channel_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

/// The request folder of the fleet at `fleet_root`.
fn request_folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("requests")
}

/// True when the error of the connect says that no fleetd listens: no
/// channel, or a channel that refuses the connection.
fn is_down(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// Refuse the server of `stream` unless its process is the process of the
/// `fleetd` row: the same PID and the same start time. An unknown fact
/// refuses it.
fn check_server(ctx: &RuntimeContext, stream: &Stream, channel: &Path) -> Result<()> {
    let refused = |reason: String| {
        anyhow!(
            "the server of the channel {} is refused, and nothing is sent: {reason}",
            channel.display()
        )
    };
    let pid = stream
        .server_pid()
        .map_err(|e| refused(format!("the process of the server is unknown ({e})")))?;
    let (row_pid, row_start) =
        fleetd_process(ctx)?.ok_or_else(|| refused("the ledger has no fleetd row".into()))?;
    let Some(start) = horch_platform::process::start_time(pid) else {
        return Err(refused(format!(
            "the start time of the server process {pid} is unknown"
        )));
    };
    if pid != row_pid || start != row_start {
        return Err(refused(format!(
            "the server process {pid} (start {start}) is not the fleetd of the fleetd row (process {row_pid}, start {row_start})"
        )));
    }
    Ok(())
}

/// The ledger, opened read-only. A missing ledger is an error.
fn ledger(ctx: &RuntimeContext) -> Result<Store> {
    crate::taskledger::open_existing_read_only(ctx)?.ok_or_else(|| {
        anyhow!(
            "the ledger {} does not exist",
            ctx.paths.ledger_db.display()
        )
    })
}

/// The PID and the start time of the `fleetd` row, or `None` with no row.
fn fleetd_process(ctx: &RuntimeContext) -> Result<Option<(u32, u64)>> {
    let store = ledger(ctx)?;
    let row = store::optional(store.conn().query_row(
        "SELECT pid, pid_start FROM fleetd WHERE id = 1",
        [],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ))
    .context("reading the fleetd row")?;
    let Some((pid, pid_start)) = row else {
        return Ok(None);
    };
    Ok(Some((
        u32::try_from(pid).context("the PID of the fleetd row")?,
        u64::try_from(pid_start).context("the start time of the fleetd row")?,
    )))
}

/// Write `line` to the server, then read 1 reply line. `None` when the
/// connection ends before a whole reply line: the reply is lost.
fn exchange(stream: &Stream, line: &str, max_bytes: u64, id: &RequestId) -> Result<Option<Reply>> {
    stream
        .set_read_timeout(REPLY_WAIT)
        .and_then(|()| stream.set_write_timeout(REPLY_WAIT))
        .with_context(|| format!("request {id}: setting the reply deadline"))?;
    let mut writer = stream;
    if writer
        .write_all(line.as_bytes())
        .and_then(|()| writer.flush())
        .is_err()
    {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    let read = BufReader::new(stream)
        .take(max_bytes.saturating_add(1))
        .read_until(b'\n', &mut bytes);
    if u64::try_from(bytes.len()).map_or(true, |n| n > max_bytes) {
        bail!("request {id}: the reply of fleetd is over the limit of {max_bytes} bytes; `horch request {id}` prints its state");
    }
    if read.is_err() || bytes.last() != Some(&b'\n') {
        return Ok(None);
    }
    let reply = parse_reply(&bytes, max_bytes)
        .map_err(|e| anyhow!("request {id}: the reply of fleetd is refused: {e}"))?;
    if reply.request_id != *id {
        bail!(
            "request {id}: the reply of fleetd is for the request {}",
            reply.request_id
        );
    }
    Ok(Some(reply))
}

/// The stored result of `request` in its `requests` row: `None` when the
/// row is missing or `pending`.
fn stored_result(ctx: &RuntimeContext, request: &Request) -> Result<Option<Reply>> {
    let id = &request.request_id;
    let store = ledger(ctx)?;
    let Some(row) = requests_store::get(store.conn(), id.as_str())? else {
        return Ok(None);
    };
    if row.payload_sha256 != request.payload_sha256() {
        bail!("request {id}: its requests row has another payload");
    }
    let outcome = match row.stored {
        requests_store::Stored::Pending => return Ok(None),
        requests_store::Stored::Applied(result) => Outcome::Applied(result),
        requests_store::Stored::Refused(reason) => Outcome::Refused(reason),
    };
    Ok(Some(Reply {
        request_id: id.clone(),
        outcome,
    }))
}

/// Write `line` to the request file of `id`, and print where it waits.
fn queue(fleet_root: &Path, id: &RequestId, line: &str) -> Result<Sent> {
    let folder = request_folder(fleet_root);
    make_request_folder(&folder)?;
    let path = folder.join(format!("{id}.json"));
    write_atomic(&path, line.as_bytes())
        .with_context(|| format!("request {id}: writing {}", path.display()))?;
    eprintln!(
        "fleetd is down: request {id} waits in {}; fleetd applies it at start",
        path.display()
    );
    Ok(Sent::Queued {
        request_id: id.clone(),
        path,
    })
}

/// Make the request folder with mode 0700 when it is missing. A link or a
/// file at that path is an error.
fn make_request_folder(folder: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    platform::create_dir_mode(&mut builder, 0o700)
        .map_err(|e| anyhow!("making {} with mode 0700: {e}", folder.display()))?;
    builder
        .create(folder)
        .with_context(|| format!("making {}", folder.display()))?;
    let meta = std::fs::symlink_metadata(folder)
        .with_context(|| format!("reading {}", folder.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a folder", folder.display());
    }
    Ok(())
}

/// Write `bytes` to `path` atomically: write `<path>.tmp` with mode 0600,
/// fsync it, rename it to `path`, then fsync the folder. A reader never
/// sees a part of the file at `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    platform::create_mode(&mut options, 0o600).map_err(|e| anyhow!("mode 0600: {e}"))?;
    let mut file = options
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!(e));
    }
    if let Some(folder) = path.parent() {
        platform::sync_dir(folder).with_context(|| format!("syncing {}", folder.display()))?;
    }
    Ok(())
}
