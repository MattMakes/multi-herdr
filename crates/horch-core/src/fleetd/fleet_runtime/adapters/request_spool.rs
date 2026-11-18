//! The request files in `<fleet root>/run/requests/`: the folder and file
//! modes, the checks of each file, its `requests` row, then its archive
//! after the row is committed.
//!
//! Requirements: SR-5, SR-6, RR-8, FM-15, FM-23. Ticket 14 of fleetd-p5 fills it.
//!
//! When fleetd is down, a `horch` command writes its request as the file
//! `<request_id>.json` (mode 0600) in the request folder (mode 0700).
//! [`scan`] reads each file 1 time:
//!
//! 1. It makes the request folder and `archive/` with mode 0700 when they
//!    are missing. A folder with another owner or a wider mode is not read,
//!    and 1 needs-you item names it until you correct it.
//! 2. It skips `*.tmp` (a write that is not complete) and each path that
//!    `ingested` records.
//! 3. It reads the file through the protected file reader, and parses it.
//!    A file that fails is recorded in `ingested` as rejected, with its
//!    reason, and stays where it is.
//! 4. It authorizes the request as a request file of the owner of the
//!    file, and gives it to [`handle`], which commits its `requests` row.
//! 5. Only then it moves the file to `archive/`.
//!
//! A crash before the commit leaves the file, and the next scan reads it
//! again. A crash after the commit leaves the file too: the next scan finds
//! the row by its `request_id`, gets the stored result with no second
//! effect, and only archives the file.

use std::fs::{self, DirBuilder};
use std::io;
use std::path::Path;

use anyhow::anyhow;
use horch_platform::channel::User;
use horch_platform::fs as platform;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::fleetd::fleet_runtime::adapters::fleetd_row;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::fleet_runtime::adapters::request_wire::{parse_request, Outcome, Request};
use crate::fleetd::fleet_runtime::adapters::scans::{requests_dir, Scan, ScanClock};
use crate::fleetd::fleet_runtime::adapters::token_file::Token;
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use crate::fleetd::fleet_runtime::domain::authorize::{authorize, RequestFacts, Source};
use crate::fleetd::fleet_runtime::domain::budget::Cursor;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::ingested_store;
use crate::taskledger::util;

/// The kind of the needs-you item of a request folder that fleetd does not
/// read.
pub const FOLDER_REFUSED: &str = "request_folder_refused";

/// The folder of the request files whose row is committed, inside the
/// request folder.
const ARCHIVE: &str = "archive";

/// The fault point after the commit of the `requests` row of a request
/// file, before its archive (`p5_requests.rs`).
pub const ABORT_BEFORE_REQUEST_ARCHIVE: &str = "abort-before-request-archive";

/// The end of the name of a request file that its writer has not renamed.
const TMP: &str = ".tmp";

/// The permission bits of group and others. The request folder and
/// `archive/` are 0700 or narrower.
const GROUP_AND_OTHERS: u32 = 0o077;

/// What a scan of the request folder needs to know.
#[derive(Clone, Copy, Debug)]
pub struct Spool<'a> {
    /// The fleet root that holds `run/requests/`.
    pub fleet_root: &'a Path,
    /// The user that owns the ledger. Each request file has this owner.
    /// A test gives another user, because it cannot make a file of
    /// another user (Q13).
    pub owner: &'a User,
    /// The operator token, or `None` when the token file failed its checks:
    /// then no operator request is authorized.
    pub token: Option<&'a Token>,
    /// The size limit of a request file (`request_max_bytes`).
    pub max_bytes: u64,
    /// The generation of this fleetd. Each write checks it first.
    pub guard: Generation,
}

/// What 1 scan of the request folder did.
#[derive(Debug, PartialEq)]
pub enum ScanOutcome {
    /// The request folder or `archive/` failed its owner or mode check, or
    /// could not be made. No file was read.
    FolderRefused { reason: String },
    /// The files that the scan read, in the order read.
    Read(Scan<FileFact>),
}

/// 1 request file that a scan read.
#[derive(Debug, PartialEq)]
pub struct FileFact {
    /// The name of the file in the request folder.
    pub name: String,
    /// What fleetd did with it.
    pub handled: Handled,
}

/// What fleetd did with 1 request file.
#[derive(Debug, PartialEq)]
pub enum Handled {
    /// Its `requests` row is committed with this outcome, and the file is
    /// in `archive/`.
    Archived(Outcome),
    /// The file failed a check, with this reason. `ingested` records it,
    /// and the file stays.
    Rejected(String),
}

/// Read the request files of `spool` from `cursor` until the deadline of
/// `clock`, and apply each with `handlers`. The cursor steps past each file
/// read. An error of the ledger, a lost generation or a failed archive
/// stops the scan; the file of that error stays for the next scan.
pub fn scan(
    conn: &mut Connection,
    spool: &Spool<'_>,
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
    handlers: &Handlers,
) -> Result<ScanOutcome, DoorError> {
    let dir = requests_dir(spool.fleet_root);
    let key = format!("{FOLDER_REFUSED}:{}", dir.display());
    if let Err(reason) = ready_folders(&dir) {
        let text = format!(
            "fleetd does not read the request files: {reason}. Give the folder to the user of \
             fleetd with mode 0700."
        );
        write(conn, spool.guard, |tx| {
            open_folder_item(tx, &key, &dir, &text)
        })?;
        return Ok(ScanOutcome::FolderRefused { reason });
    }
    if needs_you::open_keys(conn, FOLDER_REFUSED)?.contains(&key) {
        write(conn, spool.guard, |tx| needs_you::resolve(tx, &key))?;
    }
    let names = request_names(&dir).map_err(|e| anyhow!(e).context("listing the request files"))?;
    let mut facts = Vec::new();
    for name in cursor.turns(&names, String::as_str) {
        if clock.deadline.reached((clock.now)()) {
            return Ok(ScanOutcome::Read(Scan {
                facts,
                stopped_at_budget: true,
            }));
        }
        if let Some(handled) = read_file(conn, spool, &dir, name, handlers)? {
            facts.push(FileFact {
                name: name.clone(),
                handled,
            });
        }
        cursor.step(name, clock.stamp);
    }
    Ok(ScanOutcome::Read(Scan {
        facts,
        stopped_at_budget: false,
    }))
}

/// Read, check, apply and archive the request file `name` of `dir`.
/// `None` when the scan does not read it: `ingested` records its path, or
/// it was removed after the listing.
fn read_file(
    conn: &mut Connection,
    spool: &Spool<'_>,
    dir: &Path,
    name: &str,
    handlers: &Handlers,
) -> Result<Option<Handled>, DoorError> {
    let path = dir.join(name);
    let path_text = path.display().to_string();
    if ingested_store::has(conn, &path_text)? {
        return Ok(None);
    }
    if let Err(e) = fs::symlink_metadata(&path) {
        if e.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
    }
    let rule = Rule {
        root: dir,
        owner: spool.owner,
        limit: spool.max_bytes,
    };
    let parsed = read_protected(&path, &rule)
        .map_err(|r| r.reason)
        .and_then(|bytes| parse(name, &bytes, spool.max_bytes));
    let request = match parsed {
        Ok(request) => request,
        Err(reason) => {
            write(conn, spool.guard, |tx| {
                ingested_store::record_message(
                    tx,
                    &path_text,
                    None,
                    ingested_store::REJECTED,
                    Some(&reason),
                    &util::now(),
                )
            })?;
            return Ok(Some(Handled::Rejected(reason)));
        }
    };
    let caller = user_text(spool.owner);
    let decision = authorize(&RequestFacts {
        kind: request.kind.as_str().to_string(),
        source: Source::RequestFile,
        caller: Some(caller.clone()),
        ledger_user: Some(caller.clone()),
        token_valid: token_valid(spool.token, request.token.as_deref()),
        orchestrator_seats: Vec::new(),
    });
    let reply = handle(
        conn,
        spool.guard,
        &request,
        Some(&caller),
        &decision,
        handlers,
    )?;
    handlers.faults().abort_if(ABORT_BEFORE_REQUEST_ARCHIVE);
    archive(&path, &dir.join(ARCHIVE).join(name))
        .map_err(|e| anyhow!(e).context(format!("archiving {path_text}")))?;
    Ok(Some(Handled::Archived(reply.outcome)))
}

/// The request of the bytes of the file `name`. The name is
/// `<request_id>.json` of its own request.
fn parse(name: &str, bytes: &[u8], max_bytes: u64) -> Result<Request, String> {
    let request = parse_request(bytes, max_bytes).map_err(|e| e.to_string())?;
    let id = request.request_id.as_str();
    if name != format!("{id}.json") {
        return Err(format!(
            "the name {name} is not <request_id>.json of the request_id {id}"
        ));
    }
    Ok(request)
}

/// True when `given` is the operator token. No token file, or no token in
/// the request, is not valid.
fn token_valid(token: Option<&Token>, given: Option<&str>) -> bool {
    match (token, given) {
        (Some(token), Some(given)) => token.matches(given),
        _ => false,
    }
}

/// Make the request folder and `archive/` when they are missing, then check
/// each. The error is the reason, as the user reads it.
fn ready_folders(dir: &Path) -> Result<(), String> {
    let me = User::current().map_err(|e| format!("the user of fleetd is unknown: {e}"))?;
    let archive = dir.join(ARCHIVE);
    for folder in [dir, archive.as_path()] {
        let in_folder = |text: String| format!("folder {}: {text}", folder.display());
        make_folder(folder).map_err(|e| in_folder(format!("the make failed: {e}")))?;
        check_folder(folder, &me).map_err(in_folder)?;
    }
    Ok(())
}

/// Make `folder` and each missing parent with mode 0700. A folder that
/// exists keeps its mode: the check refuses a wide one.
fn make_folder(folder: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    platform::create_dir_mode(&mut builder, 0o700)
        .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.to_string()))?;
    builder.create(folder)
}

/// Check that `folder` is a folder and not a link, that `owner` owns it,
/// and that its mode is 0700 or narrower. The owner is the user of fleetd,
/// which makes the folder.
fn check_folder(folder: &Path, owner: &User) -> Result<(), String> {
    let handle = platform::open_no_follow(folder)
        .map_err(|e| format!("the open without a link failed: {e}"))?;
    let meta = handle.metadata().map_err(|e| format!("stat failed: {e}"))?;
    if !meta.is_dir() {
        return Err("not a folder".into());
    }
    let found = platform::owner_of(&handle).map_err(|_| "owner unknown".to_string())?;
    if found != *owner {
        return Err(format!(
            "owner {} is not the user of fleetd {}",
            user_text(&found),
            user_text(owner)
        ));
    }
    let mode = platform::mode_of(&meta).map_err(|_| "mode unknown".to_string())? & 0o7777;
    if mode & GROUP_AND_OTHERS != 0 {
        return Err(format!("mode {mode:04o} is wider than 0700"));
    }
    Ok(())
}

/// The names in `dir` that can be request files: not `archive/` and not a
/// `*.tmp` file, in order.
fn request_names(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name != ARCHIVE && !name.ends_with(TMP) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Open the needs-you item `key` of the request folder `dir`, with `text`.
/// An item that is open with the same text stays as it is.
fn open_folder_item(tx: &Transaction<'_>, key: &str, dir: &Path, text: &str) -> anyhow::Result<()> {
    let id = dir.display().to_string();
    needs_you::open_item(
        tx,
        &Write {
            key,
            kind: FOLDER_REFUSED,
            entity: "request_folder",
            entity_id: &id,
            text,
        },
    )
}

/// Move the request file `from` to `to` in `archive/`, and make the move
/// durable in both folders.
fn archive(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    for folder in [to.parent(), from.parent()].into_iter().flatten() {
        platform::sync_dir(folder)?;
    }
    Ok(())
}

/// Run `f` in 1 immediate transaction that first checks `guard`. It
/// commits when `f` returns `Ok`, and rolls back otherwise.
fn write<T>(
    conn: &mut Connection,
    guard: Generation,
    f: impl FnOnce(&Transaction<'_>) -> anyhow::Result<T>,
) -> Result<T, DoorError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| DoorError::Ledger(e.into()))?;
    fleetd_row::check_generation(&tx, guard)?;
    let out = f(&tx)?;
    tx.commit().map_err(|e| DoorError::Ledger(e.into()))?;
    Ok(out)
}

/// The text of `user`: the `caller` of the `requests` row of a request
/// file, and the user in a reason.
fn user_text(user: &User) -> String {
    match user {
        User::Uid(uid) => format!("uid {uid}"),
        User::Sid(sid) => format!("sid {sid}"),
    }
}
