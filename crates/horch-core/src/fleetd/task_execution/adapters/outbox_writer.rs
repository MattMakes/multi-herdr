use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::peer_process::{ProcessFacts, MAX_CHAIN};
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::domain::outbox::{
    question_name, question_number, LineKind, ProgressLine, Question, DIR_MODE, FILE_MODE,
    PROGRESS_JSONL, QUESTIONS, RESULT_JSON, TMP_SUFFIX,
};
use crate::fleetd::task_execution::domain::result::{validate, ResultFile};

pub const NO_LIVE_ATTEMPT: &str = "no live attempt of this process";

const QUESTION_TRIES: u32 = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveAttempt {
    pub attempt: AttemptRef,
    pub wrapper_pid: u32,
    pub wrapper_start: u64,
}

pub fn live_attempts(conn: &Connection) -> Result<Vec<LiveAttempt>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, pid, pid_start FROM attempts
         WHERE state = 'live' AND task_id IS NOT NULL
           AND pid IS NOT NULL AND pid_start IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut live = Vec::new();
    for row in rows {
        let (attempt_id, task_id, pid, start) = row.context("read the live attempts")?;
        let (Ok(wrapper_pid), Ok(wrapper_start)) = (u32::try_from(pid), u64::try_from(start))
        else {
            continue;
        };
        live.push(LiveAttempt {
            attempt: AttemptRef {
                task_id,
                attempt_id,
            },
            wrapper_pid,
            wrapper_start,
        });
    }
    Ok(live)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoLiveAttempt {
    pub detail: Option<String>,
}

impl std::fmt::Display for NoLiveAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            None => f.write_str(NO_LIVE_ATTEMPT),
            Some(detail) => write!(f, "{NO_LIVE_ATTEMPT}: {detail}"),
        }
    }
}

impl std::error::Error for NoLiveAttempt {}

fn unknown(detail: String) -> NoLiveAttempt {
    NoLiveAttempt {
        detail: Some(detail),
    }
}

pub fn own_attempt(
    pid: u32,
    live: &[LiveAttempt],
    facts: &dyn ProcessFacts,
) -> Result<AttemptRef, NoLiveAttempt> {
    let mut found: Vec<&LiveAttempt> = Vec::new();
    let mut p = pid;
    for _ in 0..MAX_CHAIN {
        for a in live.iter().filter(|a| a.wrapper_pid == p) {
            let start = facts
                .start_time(p)
                .ok_or_else(|| unknown(format!("the start time of process {p} cannot be read")))?;
            if a.wrapper_start == start {
                found.push(a);
            }
        }
        if p == 1 {
            return the_one(&found);
        }
        match facts.parent(p) {
            None => return Err(unknown(format!("the parent of process {p} cannot be read"))),
            Some(0) => return the_one(&found),
            Some(parent) => p = parent,
        }
    }
    Err(unknown(format!(
        "the parent chain is longer than {MAX_CHAIN} processes"
    )))
}

fn the_one(found: &[&LiveAttempt]) -> Result<AttemptRef, NoLiveAttempt> {
    match found {
        [] => Err(NoLiveAttempt { detail: None }),
        [one] => Ok(one.attempt.clone()),
        more => {
            let ids: Vec<&str> = more.iter().map(|a| a.attempt.attempt_id.as_str()).collect();
            Err(unknown(format!(
                "{} live attempts are in its parent chain: {}",
                ids.len(),
                ids.join(", ")
            )))
        }
    }
}

pub const ATTEMPT_WAIT: Duration = Duration::from_secs(START_TIMEOUT_S.unsigned_abs());

pub const ATTEMPT_POLL: Duration = Duration::from_millis(250);

fn wait_own_attempt(
    ledger: &Connection,
    facts: &dyn ProcessFacts,
    wait: Duration,
) -> Result<AttemptRef> {
    let until = Instant::now() + wait;
    loop {
        let was_starting = starting(ledger)?;
        let live = live_attempts(ledger)?;
        match own_attempt(std::process::id(), &live, facts) {
            Err(e) if e.detail.is_none() && was_starting && Instant::now() < until => {
                std::thread::sleep(ATTEMPT_POLL);
            }
            found => return Ok(found?),
        }
    }
}

pub fn has_push_attempt(conn: &Connection, worker_id: &str) -> Result<bool> {
    let found: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM attempts WHERE worker_id = ?1 AND via = 'push')",
        [worker_id],
        |r| r.get(0),
    )?;
    Ok(found)
}

pub fn starting(conn: &Connection) -> Result<bool> {
    let found: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM attempts
           WHERE state = 'launching' OR (state = 'live' AND pid IS NULL))",
        [],
        |r| r.get(0),
    )?;
    Ok(found)
}

#[derive(Debug)]
pub struct OutboxWriter {
    attempt: AttemptRef,
    outbox: PathBuf,
    result_max_bytes: u64,
    question_max_bytes: u64,
    progress_line_max_bytes: u64,
}

impl OutboxWriter {
    pub fn of_this_process(
        fleet_root: &Path,
        ledger: &Connection,
        config: &FleetConfig,
        facts: &dyn ProcessFacts,
    ) -> Result<OutboxWriter> {
        let attempt = wait_own_attempt(ledger, facts, ATTEMPT_WAIT)?;
        Ok(OutboxWriter {
            outbox: attempt.outbox(fleet_root),
            attempt,
            result_max_bytes: config.result_max_bytes,
            question_max_bytes: config.question_max_bytes,
            progress_line_max_bytes: config.progress_line_max_bytes,
        })
    }

    pub fn attempt(&self) -> &AttemptRef {
        &self.attempt
    }

    pub fn outbox(&self) -> &Path {
        &self.outbox
    }

    pub fn append(&self, kind: LineKind, at: &str, text: &str) -> Result<()> {
        let line = ProgressLine {
            task: self.attempt.task_id.clone(),
            attempt: self.attempt.attempt().to_string(),
            at: at.to_string(),
            kind,
            text: text.to_string(),
        }
        .to_line(self.progress_line_max_bytes)?;
        make_dir(&self.outbox)?;
        let path = self.outbox.join(PROGRESS_JSONL);
        refuse_link(&path)?;
        let mut options = OpenOptions::new();
        options.append(true).create(true);
        let _ = horch_platform::fs::create_mode(&mut options, FILE_MODE);
        let mut file = options
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        let written = file
            .write(line.as_bytes())
            .with_context(|| format!("append to {}", path.display()))?;
        if written != line.len() {
            bail!(
                "{}: wrote {written} of {} bytes of 1 line",
                path.display(),
                line.len()
            );
        }
        Ok(())
    }

    pub fn write_result(&self, file: &ResultFile) -> Result<PathBuf> {
        let bytes = file.to_json().into_bytes();
        validate(
            &bytes,
            &self.attempt.task_id,
            self.attempt.attempt(),
            self.result_max_bytes,
        )?;
        make_dir(&self.outbox)?;
        let path = self.outbox.join(RESULT_JSON);
        match create_message(&self.outbox, RESULT_JSON, &bytes) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => bail!(
                "{} exists; result.json of an attempt is written 1 time",
                path.display()
            ),
            r => r.with_context(|| format!("write {}", path.display())),
        }
    }

    pub fn write_question(&self, at: &str, text: &str, mail_id: Option<&str>) -> Result<PathBuf> {
        let bytes = Question {
            task: self.attempt.task_id.clone(),
            attempt: self.attempt.attempt().to_string(),
            at: at.to_string(),
            text: text.to_string(),
            mail_id: mail_id.map(str::to_string),
        }
        .to_json(self.question_max_bytes)?
        .into_bytes();
        make_dir(&self.outbox)?;
        let dir = self.outbox.join(QUESTIONS);
        make_dir(&dir)?;
        let first = highest_question(&dir)?
            .checked_add(1)
            .context("no question number is free")?;
        for n in first..first.saturating_add(QUESTION_TRIES) {
            match create_message(&dir, &question_name(n), &bytes) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                r => {
                    return r.with_context(|| format!("write a question file in {}", dir.display()))
                }
            }
        }
        bail!(
            "{}: question numbers {first} to {} are taken",
            dir.display(),
            first.saturating_add(QUESTION_TRIES - 1)
        )
    }
}

fn highest_question(dir: &Path) -> Result<u32> {
    let mut highest = 0;
    for entry in std::fs::read_dir(dir).with_context(|| format!("list {}", dir.display()))? {
        let entry = entry.with_context(|| format!("list {}", dir.display()))?;
        if let Some(n) = question_number(&entry.file_name().to_string_lossy()) {
            highest = highest.max(n);
        }
    }
    Ok(highest)
}

fn make_dir(dir: &Path) -> Result<()> {
    if is_dir(dir)? {
        return Ok(());
    }
    let mut builder = DirBuilder::new();
    let _ = horch_platform::fs::create_dir_mode(&mut builder, DIR_MODE);
    match builder.create(dir) {
        Ok(()) => {
            ok_if_not_supported(horch_platform::fs::set_mode(dir, DIR_MODE))
                .with_context(|| format!("set the mode of {}", dir.display()))?;
            if let Some(parent) = dir.parent() {
                ok_if_not_supported(horch_platform::fs::sync_dir(parent))
                    .with_context(|| format!("sync {}", parent.display()))?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && is_dir(dir)? => Ok(()),
        Err(e) => Err(e).with_context(|| format!("make {}", dir.display())),
    }
}

fn is_dir(dir: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => bail!("{} is not a folder", dir.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("read {}", dir.display())),
    }
}

fn refuse_link(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => bail!("{} is a link", path.display()),
        _ => Ok(()),
    }
}

fn create_message(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let path = dir.join(name);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = dir.join(format!(
        "{name}.{}-{}{TMP_SUFFIX}",
        std::process::id(),
        &nonce[..12]
    ));
    let linked = write_temp(&tmp, bytes).and_then(|()| std::fs::hard_link(&tmp, &path));
    let _ = std::fs::remove_file(&tmp);
    linked?;
    ok_if_not_supported(horch_platform::fs::sync_dir(dir))?;
    Ok(path)
}

fn write_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let _ = horch_platform::fs::create_mode(&mut options, FILE_MODE);
    let mut file = options.open(tmp)?;
    ok_if_not_supported(horch_platform::fs::set_mode(tmp, FILE_MODE))?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn ok_if_not_supported(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e)
            if e.get_ref()
                .is_some_and(|inner| inner.is::<horch_platform::NotSupported>()) =>
        {
            Ok(())
        }
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use rusqlite::{params, OpenFlags};

    use super::*;
    use crate::taskledger::schema;

    const START: u64 = 77;

    struct LiveDuringTheWalk {
        ledger: PathBuf,
        moved: Cell<bool>,
    }

    impl ProcessFacts for LiveDuringTheWalk {
        fn parent(&self, _pid: u32) -> Option<u32> {
            if !self.moved.replace(true) {
                Connection::open(&self.ledger)
                    .unwrap()
                    .execute(
                        "UPDATE attempts SET state = 'live', pid = ?1, pid_start = ?2",
                        params![i64::from(std::process::id()), START as i64],
                    )
                    .unwrap();
            }
            Some(0)
        }

        fn start_time(&self, _pid: u32) -> Option<u64> {
            Some(START)
        }

        fn in_group(&self, _pid: u32, _leader: u32, _leader_start: u64) -> Option<bool> {
            None
        }
    }

    #[test]
    fn attempt_that_goes_live_between_the_2_reads_of_the_wait_is_found() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = temp.path().join("ledger.sqlite3");
        let mut conn = Connection::open(&ledger).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        conn.execute(
            "INSERT INTO attempts (id, task_id, worker_id, state, started_at)
             VALUES ('T-1/A1', 'T-1', 'w-1', 'launching', '2026-11-23T00:00:00Z')",
            [],
        )
        .unwrap();
        drop(conn);
        let read = Connection::open_with_flags(&ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let facts = LiveDuringTheWalk {
            ledger: ledger.clone(),
            moved: Cell::new(false),
        };
        let own = wait_own_attempt(&read, &facts, Duration::from_secs(5)).unwrap();
        assert_eq!(own.attempt_id, "T-1/A1");
    }
}
