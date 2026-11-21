use std::fs;
use std::io;
use std::path::Path;

use anyhow::anyhow;
use horch_platform::channel::User;
use horch_platform::fs as platform;
use horch_platform::NotSupported;
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

pub const FOLDER_REFUSED: &str = "request_folder_refused";

pub const FILE_REJECTED: &str = "request_file_rejected";

pub const REQUEST_REFUSED: &str = "request_file_refused";

const ARCHIVE: &str = "archive";

pub const ABORT_BEFORE_REQUEST_ARCHIVE: &str = "abort-before-request-archive";

const TMP: &str = ".tmp";

#[derive(Clone, Copy, Debug)]
pub struct Spool<'a> {
    pub fleet_root: &'a Path,
    pub owner: &'a User,
    pub token: Option<&'a Token>,
    pub max_bytes: u64,
    pub guard: Generation,
}

#[derive(Debug, PartialEq)]
pub enum ScanOutcome {
    FolderRefused { reason: String },
    Read(Scan<FileFact>),
}

#[derive(Debug, PartialEq)]
pub struct FileFact {
    pub name: String,
    pub handled: Handled,
}

#[derive(Debug, PartialEq)]
pub enum Handled {
    Archived(Outcome),
    Rejected(String),
}

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
            let text = format!(
                "fleetd did not read the request file {path_text}: {reason}. The file stays \
                 where it is. Correct it and write it again with a new request_id, or remove it."
            );
            write(conn, spool.guard, |tx| {
                ingested_store::record_message(
                    tx,
                    &path_text,
                    None,
                    ingested_store::REJECTED,
                    Some(&reason),
                    &util::now(),
                )?;
                needs_you::open_item(
                    tx,
                    &Write {
                        key: &format!("{FILE_REJECTED}:{path_text}"),
                        kind: FILE_REJECTED,
                        entity: "request_file",
                        entity_id: &path_text,
                        text: &text,
                    },
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
    });
    let reply = handle(
        conn,
        spool.guard,
        &request,
        Some(&caller),
        &decision,
        handlers,
    )?;
    let archived = dir.join(ARCHIVE).join(name);
    if let Outcome::Refused(reason) = &reply.outcome {
        write(conn, spool.guard, |tx| {
            open_refused_item(tx, &request, &archived, reason)
        })?;
    }
    handlers.faults().abort_if(ABORT_BEFORE_REQUEST_ARCHIVE);
    archive(&path, &archived).map_err(|e| anyhow!(e).context(format!("archiving {path_text}")))?;
    Ok(Some(Handled::Archived(reply.outcome)))
}

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

fn token_valid(token: Option<&Token>, given: Option<&str>) -> bool {
    match (token, given) {
        (Some(token), Some(given)) => token.matches(given),
        _ => false,
    }
}

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

fn make_folder(folder: &Path) -> io::Result<()> {
    platform::create_dir_owner_only(folder)
}

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
    platform::check_owner_only(&handle, &meta, 0o700)
}

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

fn open_refused_item(
    tx: &Transaction<'_>,
    request: &Request,
    archived: &Path,
    reason: &str,
) -> anyhow::Result<()> {
    let id = request.request_id.as_str();
    let text = format!(
        "fleetd refused the queued request {id} of kind {}: {reason}. Its file is {}. \
         Read the request in that file and send it again.",
        request.kind.as_str(),
        archived.display()
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &format!("{REQUEST_REFUSED}:{id}"),
            kind: REQUEST_REFUSED,
            entity: "request",
            entity_id: id,
            text: &text,
        },
    )
}

fn archive(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    for folder in [to.parent(), from.parent()].into_iter().flatten() {
        match platform::sync_dir(folder) {
            Err(e) if e.get_ref().is_some_and(|inner| inner.is::<NotSupported>()) => {}
            synced => synced?,
        }
    }
    Ok(())
}

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

fn user_text(user: &User) -> String {
    match user {
        User::Uid(uid) => format!("uid {uid}"),
        User::Sid(sid) => format!("sid {sid}"),
    }
}
