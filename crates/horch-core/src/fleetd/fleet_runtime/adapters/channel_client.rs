use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::Stream;
use horch_platform::fs as platform;
use horch_platform::NotSupported;
use serde_json::{Map, Value};

use crate::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, recorded_processes, Fact, PlatformFacts,
};
use crate::fleetd::fleet_runtime::adapters::request_wire::{parse_reply, Outcome, Reply, Request};
use crate::fleetd::fleet_runtime::adapters::{config_file, requests_store, token_file};
use crate::fleetd::fleet_runtime::domain::requests::{check_size, Class, Kind, RequestId};
use crate::runtime::RuntimeContext;
use crate::taskledger::store::{self, Store};

const CONNECT_WAIT: Duration = Duration::from_secs(5);

const REPLY_WAIT: Duration = Duration::from_secs(30);

const MAX_SENDS: usize = 3;

static WRAPPER: AtomicBool = AtomicBool::new(false);

pub fn mark_wrapper_process() {
    WRAPPER.store(true, Ordering::SeqCst);
}

pub fn is_wrapper_process() -> bool {
    WRAPPER.load(Ordering::SeqCst)
}

#[derive(Debug)]
pub enum Sent {
    Replied(Reply),
    Queued {
        request_id: RequestId,
        path: PathBuf,
    },
}

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
        if let Some(reply) = stored_result(ctx, &request)? {
            return Ok(Sent::Replied(reply));
        }
    }
    bail!(
        "request {id} got no reply after {MAX_SENDS} sends; `horch request {id}` prints its state"
    )
}

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

fn own_fact(ctx: &RuntimeContext) -> Fact {
    let Ok(Some(store)) = crate::taskledger::open_existing_read_only(ctx) else {
        return Fact::No;
    };
    match recorded_processes(store.conn()) {
        Ok(Some((recorded, _))) => fleet_process(std::process::id(), &recorded, &PlatformFacts),
        Ok(None) | Err(_) => Fact::Unknown,
    }
}

fn new_request_id() -> RequestId {
    RequestId::parse(&uuid::Uuid::new_v4().to_string()).expect("a UUID meets the request_id rule")
}

fn channel_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.sock")
}

fn request_folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("requests")
}

fn is_down(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

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

fn ledger(ctx: &RuntimeContext) -> Result<Store> {
    crate::taskledger::open_existing_read_only(ctx)?.ok_or_else(|| {
        anyhow!(
            "the ledger {} does not exist",
            ctx.paths.ledger_db.display()
        )
    })
}

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

fn make_request_folder(folder: &Path) -> Result<()> {
    platform::create_dir_owner_only(folder)
        .with_context(|| format!("making {}", folder.display()))?;
    let meta = std::fs::symlink_metadata(folder)
        .with_context(|| format!("reading {}", folder.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a folder", folder.display());
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = platform::create_new_owner_only(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!(e));
    }
    if let Some(folder) = path.parent() {
        match platform::sync_dir(folder) {
            Err(e) if e.get_ref().is_some_and(|inner| inner.is::<NotSupported>()) => {}
            synced => synced.with_context(|| format!("syncing {}", folder.display()))?,
        }
    }
    Ok(())
}
