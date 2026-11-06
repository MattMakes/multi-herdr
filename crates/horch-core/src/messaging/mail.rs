//! Durable mail: worker-to-orchestrator, and horch's own notices to a
//! worker role (`roles/<role>/` under the workspace's mail dir).

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::messaging::{file_safe, private};

const ACK_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The idempotency key that [`keyed`] gives the next enqueue.
    static KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Run `f` with `key` as the idempotency key of the first enqueue on this
/// thread inside it (C3, D7). That enqueue returns the first mail of `key`
/// when it is pending or acknowledged, and writes no new file. A caller
/// that cannot reach the enqueue, such as an outbox drain that sends
/// through `horch tell`, passes the outbox row id here.
pub fn keyed<T>(key: &str, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            KEY.with(|k| *k.borrow_mut() = previous);
        }
    }
    let _restore = Restore(KEY.with(|k| k.replace(Some(key.to_string()))));
    f()
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct Id(String);

impl Id {
    /// The full id: the mail file's name without `.json`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id a mail line shows: 8 hex characters, a 32-bit FNV-1a hash of
    /// the full id. Every process computes the same value from the file name.
    pub fn short(&self) -> String {
        let hash = self.0.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
        format!("{hash:08x}")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub id: Id,
    pub from: String,
    pub text: String,
    pub created_at: SystemTime,
    /// The ledger actions that close a question horch raised, such as
    /// `requeue T-7` (C2). Empty for other mail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closes_on: Vec<String>,
}

impl Message {
    /// The line a delivery types: the text, with the short mail id in the
    /// sender tag (C2). Every delivery of this message shows the same id.
    pub fn line(&self) -> String {
        crate::messaging::message::with_mail_id(&self.text, &self.from, &self.id.short())
    }
}

#[derive(Clone, Debug)]
pub struct Mail {
    dir: PathBuf,
}

impl Mail {
    pub fn under(state_root: &Path, workspace: &str) -> Self {
        Self {
            dir: state_root.join("mail").join(file_safe(workspace)),
        }
    }

    /// Mail to a worker role: the reviews-in notice. The workspace's
    /// courier delivers it too.
    pub fn to_role(state_root: &Path, workspace: &str, role: &str) -> Self {
        Self {
            dir: Self::under(state_root, workspace)
                .dir
                .join("roles")
                .join(file_safe(role)),
        }
    }

    /// The mail of `role`: the workspace mail for the orchestrator, else
    /// [`Mail::to_role`].
    pub fn for_role(state_root: &Path, workspace: &str, role: &str) -> Self {
        if role == "orchestrator" {
            Self::under(state_root, workspace)
        } else {
            Self::to_role(state_root, workspace, role)
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Create this mail dir mode 0700, or check the existing one (#77).
    pub fn ensure_dir(&self) -> Result<()> {
        private::ensure_dir(&self.dir)
    }

    /// The worker roles that have a mail dir under this workspace mail.
    pub fn roles(&self) -> Result<Vec<(String, Mail)>> {
        let Some(entries) = read_dir_if_exists(&self.dir.join("roles"))? else {
            return Ok(Vec::new());
        };
        let mut roles = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(role) = entry.file_name().to_str() {
                roles.push((role.to_string(), Mail { dir: entry.path() }));
            }
        }
        roles.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(roles)
    }

    /// Acknowledge every pending message. Returns how many there were.
    pub fn ack_all(&self) -> Result<usize> {
        let pending = self.pending()?;
        for message in &pending {
            self.ack(&message.id)?;
        }
        Ok(pending.len())
    }

    pub fn enqueue(&self, from: &str, text: &str) -> Result<Id> {
        self.enqueue_with_questions(from, text, text)
    }

    /// Keep question lines from the original body before transport truncation.
    /// horch's own QUESTION or BLOCKED that names no closing ledger action
    /// is queued as a NOTE ([`horch_line`]).
    pub fn enqueue_with_questions(&self, from: &str, text: &str, original: &str) -> Result<Id> {
        let key = KEY.with(|k| k.borrow_mut().take());
        self.enqueue_inner(key.as_deref(), from, text, original)
    }

    /// [`Mail::enqueue_with_questions`] with an idempotency key: the first
    /// mail of `key`, pending or acknowledged, is returned and not written
    /// again. A replay after a crash keeps 1 file and 1 mail id.
    pub fn enqueue_with_key(
        &self,
        key: &str,
        from: &str,
        text: &str,
        original: &str,
    ) -> Result<Id> {
        self.enqueue_inner(Some(key), from, text, original)
    }

    fn enqueue_inner(
        &self,
        key: Option<&str>,
        from: &str,
        text: &str,
        original: &str,
    ) -> Result<Id> {
        let (text, original) = if from == HORCH {
            (horch_line(text), horch_line(original))
        } else {
            (text.to_string(), original.to_string())
        };
        let (text, original) = (text.as_str(), original.as_str());
        let pending = self.dir.join("pending");
        self.ensure_dir()?;
        private::ensure_dir(&pending)?;
        // Cleanup is best effort: a prune error must never block a send.
        if let Err(error) = self.prune_acked().and_then(|()| self.prune_keys()) {
            eprintln!("horch: pruning acked mail: {error:#}");
        }

        let created_at = crate::clock::now().into();
        let id = match key {
            None => next_id(created_at),
            Some(key) => match self.claim_key(key, created_at)? {
                Claim::Published(id) => return Ok(id),
                Claim::Publish(id) => id,
            },
        };
        let message = Message {
            id: id.clone(),
            from: from.to_string(),
            text: text.to_string(),
            created_at,
            closes_on: Vec::new(),
        };
        let final_path = pending.join(format!("{}.json", id.0));
        let temp_path = pending.join(format!(".{}.json.tmp", id.0));
        write_temp(
            &temp_path,
            &serde_json::to_vec(&message).context("serializing mail")?,
        )?;
        let mut questions = question_lines(original, from);
        if from == HORCH && closing_actions(&questions).is_empty() {
            // A horch question with no closing ledger action is mail only:
            // no answer by id could reach horch, so it would never close.
            questions.clear();
        }
        let question_path = (!questions.is_empty())
            .then(|| {
                let closes_on = if from == HORCH {
                    closing_actions(&questions)
                } else {
                    Vec::new()
                };
                let question = Message {
                    text: questions,
                    closes_on,
                    ..message
                };
                self.publish_question(&id, &question)
            })
            .transpose()?;
        // Publish questions first: a courier/inbox can observe pending mail as
        // soon as this rename completes, and acknowledgement must not erase them.
        publish_pending(&temp_path, &final_path, question_path.as_deref())?;
        Ok(id)
    }

    /// The mail id of `key`. The key file names the id before the mail is
    /// published, so a crash at any point leaves 1 id for the replay.
    fn claim_key(&self, key: &str, created_at: SystemTime) -> Result<Claim> {
        let dir = self.dir.join("keys");
        private::ensure_dir(&dir)?;
        let name = key_file(key);
        let path = dir.join(&name);
        if let Some(named) = if_exists(std::fs::read_to_string(&path))
            .with_context(|| format!("reading {}", path.display()))?
        {
            let id = Id(named.trim().to_string());
            if id.0.is_empty() || !valid_id(&id.0) {
                bail!("invalid mail id in {}", path.display());
            }
            if self.published(&id) {
                return Ok(Claim::Published(id));
            }
            // The crash came before the publish: drop its temp files.
            let pending = self.dir.join("pending").join(format!(".{}.json.tmp", id.0));
            let question = self.dir.join("questions").join(format!(".{}.tmp", id.0));
            for temp in [pending, question] {
                if_exists(std::fs::remove_file(&temp))
                    .with_context(|| format!("removing {}", temp.display()))?;
            }
            return Ok(Claim::Publish(id));
        }
        let id = next_id(created_at);
        let temp = dir.join(format!(".{name}.tmp"));
        if_exists(std::fs::remove_file(&temp))
            .with_context(|| format!("removing {}", temp.display()))?;
        write_temp(&temp, id.0.as_bytes())?;
        std::fs::rename(&temp, &path)
            .with_context(|| format!("moving {} to {}", temp.display(), path.display()))?;
        Ok(Claim::Publish(id))
    }

    /// Whether mail `id` is pending or acknowledged.
    fn published(&self, id: &Id) -> bool {
        ["pending", "acked"]
            .iter()
            .any(|dir| self.dir.join(dir).join(format!("{}.json", id.0)).exists())
    }

    fn publish_question(&self, id: &Id, question: &Message) -> Result<PathBuf> {
        let dir = self.dir.join("questions");
        private::ensure_dir(&dir)?;
        let path = dir.join(format!("{}.json", id.0));
        let temporary = dir.join(format!(".{}.tmp", id.0));
        write_temp(&temporary, &serde_json::to_vec(question)?)?;
        std::fs::rename(&temporary, &path)?;
        Ok(path)
    }

    /// Unanswered questions are independent of pending mail and need no ledger run.
    pub fn open_questions(&self) -> Result<Vec<Message>> {
        let dir = self.dir.join("questions");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let now: SystemTime = crate::clock::now().into();
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let message: Message = serde_json::from_reader(file)?;
            if now
                .duration_since(message.created_at)
                .is_ok_and(|age| age > ACK_RETENTION)
            {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                continue;
            }
            let answered = self.answered_at(&message.from);
            if answered.is_none_or(|at| at < message.created_at) {
                out.push(message);
            }
        }
        out.sort_by_key(|m| m.created_at);
        Ok(out)
    }

    /// The answer file of an earlier release; it is read, never written.
    fn answer_path(&self, role: &str) -> PathBuf {
        self.dir
            .join("answers")
            .join(format!("{}.json", file_safe(role)))
    }

    /// The open question `key` names: its short id (`#` optional) or its
    /// full id.
    pub fn find_question(&self, key: &str) -> Result<Option<Message>> {
        let key = key.trim().trim_start_matches('#');
        Ok(self
            .open_questions()?
            .into_iter()
            .find(|q| q.id.short() == key || q.id.0 == key))
    }

    /// Close the open question `key` names, and only that question (C2).
    /// `None`: no open question has that id.
    pub fn close_question(&self, key: &str) -> Result<Option<Message>> {
        let Some(question) = self.find_question(key)? else {
            return Ok(None);
        };
        let path = self
            .dir
            .join("questions")
            .join(format!("{}.json", question.id.0));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
        }
        Ok(Some(question))
    }

    /// When an earlier release last answered `role`: its newest marker, or
    /// its answer file, whichever is later. These are read, never written: a
    /// question asked now closes only by its id.
    fn answered_at(&self, role: &str) -> Option<SystemTime> {
        let earlier: Option<SystemTime> = std::fs::read(self.answer_path(role))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let newest = answer_markers(&self.answers_dir(role))
            .into_iter()
            .map(|(at, _)| at)
            .max();
        earlier.max(newest)
    }

    fn answers_dir(&self, role: &str) -> PathBuf {
        self.dir.join("answers").join(file_safe(role))
    }

    /// Shared persisted login state. Returns true only for the first observer.
    pub fn set_login_required(&self, required: bool) -> Result<bool> {
        self.ensure_dir()?;
        let path = self.dir.join("login-required");
        if required {
            return match private::create_new(&path) {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
                Err(e) => Err(e.into()),
            };
        }
        match std::fs::remove_file(path) {
            Ok(()) => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn login_required(&self) -> bool {
        self.dir.join("login-required").exists()
    }

    /// Mail in a dir that another user owns or can write is refused: it
    /// could hold lines planted for the orchestrator to read.
    pub fn pending(&self) -> Result<Vec<Message>> {
        private::check(&self.dir)?;
        read_pending(&self.pending_paths()?)
    }

    /// The pending mail files, oldest first.
    fn pending_paths(&self) -> Result<Vec<PathBuf>> {
        let Some(entries) = read_dir_if_exists(&self.dir.join("pending"))? else {
            return Ok(Vec::new());
        };
        let mut paths = entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
        paths.sort();
        Ok(paths)
    }

    pub fn ack(&self, id: &Id) -> Result<()> {
        if !valid_id(&id.0) {
            bail!("invalid mail id");
        }
        let pending = self.dir.join("pending").join(format!("{}.json", id.0));
        let acked_dir = self.dir.join("acked");
        private::ensure_dir(&acked_dir)?;
        let acked = acked_dir.join(format!("{}.json", id.0));
        match std::fs::rename(&pending, &acked) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("moving {} to {}", pending.display(), acked.display())),
        }
    }

    fn prune_acked(&self) -> Result<()> {
        let Some(entries) = read_dir_if_exists(&self.dir.join("acked"))? else {
            return Ok(());
        };
        self.prune_paths(entries.map(|entry| entry.map(|entry| entry.path())))
    }

    /// Remove the expired key files whose mail is not pending. A pending
    /// mail keeps its key: pending mail never expires.
    fn prune_keys(&self) -> Result<()> {
        let Some(entries) = read_dir_if_exists(&self.dir.join("keys"))? else {
            return Ok(());
        };
        let pending = self.dir.join("pending");
        let paths = entries.filter(|entry| {
            let Ok(entry) = entry else { return true };
            let named = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let id = named.trim();
            !(!id.is_empty() && valid_id(id) && pending.join(format!("{id}.json")).exists())
        });
        self.prune_paths(paths.map(|entry| entry.map(|entry| entry.path())))
    }

    /// Another enqueue can prune the same file first, so NotFound is skipped.
    fn prune_paths(&self, paths: impl IntoIterator<Item = std::io::Result<PathBuf>>) -> Result<()> {
        let now = SystemTime::now();
        for path in paths {
            match path {
                Ok(path) => remove_if_expired(&path, now)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("reading an acked mail entry"),
            }
        }
        Ok(())
    }
}

/// What [`Mail::claim_key`] found.
enum Claim {
    /// The mail of the key is pending or acknowledged already.
    Published(Id),
    /// Publish the mail with this id.
    Publish(Id),
}

/// The key file name of an idempotency key: 32 hex characters of its
/// sha256, safe as a file name for any key.
fn key_file(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A mail id is a file name: ASCII letters, digits and `-` only.
fn valid_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `Some` of an io result, or `None` when the path is missing.
fn if_exists<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// `read_dir`, with a missing dir as `None`.
fn read_dir_if_exists(dir: &Path) -> Result<Option<std::fs::ReadDir>> {
    if_exists(std::fs::read_dir(dir)).with_context(|| format!("reading {}", dir.display()))
}

/// Remove `path` when it is older than [`ACK_RETENTION`]. A file that a
/// concurrent prune removed first is skipped.
fn remove_if_expired(path: &Path, now: SystemTime) -> Result<()> {
    let metadata = if_exists(path.metadata().and_then(|metadata| metadata.modified()))
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    let expired = metadata.is_some_and(|modified| {
        now.duration_since(modified)
            .is_ok_and(|age| age > ACK_RETENTION)
    });
    if expired {
        if_exists(std::fs::remove_file(path))
            .with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

/// Move the written pending mail into place. A failed move removes it and
/// its published question.
fn publish_pending(temp: &Path, published: &Path, question: Option<&Path>) -> Result<()> {
    std::fs::rename(temp, published)
        .with_context(|| format!("moving {} to {}", temp.display(), published.display()))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(temp);
            question.map(std::fs::remove_file);
        })
}

/// Create `path` and write `bytes` to disk. A failed write removes the file.
fn write_temp(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file =
        private::create_new(path).with_context(|| format!("creating {}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", path.display()))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(path);
        })
}

/// The lines of `original` that ask a question or report a block.
fn question_lines(original: &str, from: &str) -> String {
    let prefix = format!("[{from}]");
    original
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix(&prefix).unwrap_or(line).trim_start();
            line.starts_with("QUESTION:") || line.starts_with("BLOCKED:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The sender tag of horch's own lines.
const HORCH: &str = "horch";

/// The actions of `text` that close a horch question: the ledger actions
/// with a closing event (C2).
fn closing_actions(text: &str) -> Vec<String> {
    ledger_actions(text)
        .into_iter()
        .filter(|a| {
            a.split_once(' ')
                .and_then(|(verb, _)| crate::taskledger::workers::closing_event(verb))
                .is_some()
        })
        .collect()
}

/// Whether `text` names a horch command: `horch` followed by a lowercase
/// subcommand, such as `horch compact` or `horch ledger requeue T-7`.
fn names_horch_command(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(2).any(|w| {
        w[0].trim_start_matches(|c: char| !c.is_ascii_alphanumeric()) == "horch"
            && w[1].starts_with(|c: char| c.is_ascii_lowercase())
            && w[1]
                .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
    })
}

/// A line of horch as horch sends it (C2). A QUESTION or BLOCKED must name
/// the horch command that answers it, such as `horch ledger requeue T-7`
/// or `horch compact <role> --request`. When `text` names none, each such
/// keyword becomes `NOTE:`.
pub fn horch_line(text: &str) -> String {
    if names_horch_command(text) {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let body = line.trim_start();
        let lead = &line[..line.len() - body.len()];
        let (tag, rest) = match body
            .strip_prefix("[horch")
            .and_then(|r| r.find(']').map(|i| i + 7))
        {
            Some(end) => body.split_at(end),
            None => ("", body),
        };
        let gap = &rest[..rest.len() - rest.trim_start().len()];
        let words = rest.trim_start();
        let note = ["QUESTION:", "BLOCKED:"]
            .iter()
            .find_map(|k| words.strip_prefix(k))
            .map(|after| format!("{lead}{tag}{gap}NOTE:{after}"));
        out.push(note.unwrap_or_else(|| line.to_string()));
    }
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// The ledger actions `text` names, as `<verb> T-<n>`: each
/// `horch ledger <verb> T-<n>` in it, in order, without repeats.
pub fn ledger_actions(text: &str) -> Vec<String> {
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-'))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for w in words.windows(4) {
        let task = w[3].strip_prefix("T-").unwrap_or("");
        let named = w[0] == "horch"
            && w[1] == "ledger"
            && !w[2].is_empty()
            && !task.is_empty()
            && task.bytes().all(|b| b.is_ascii_digit());
        let action = format!("{} {}", w[2], w[3]);
        if named && !out.contains(&action) {
            out.push(action);
        }
    }
    out
}

/// Read the pending messages at `paths`. A file that a concurrent ack moved
/// away after the listing is no longer pending, so it is skipped.
fn read_pending(paths: &[PathBuf]) -> Result<Vec<Message>> {
    paths
        .iter()
        .filter_map(|path| read_message(path).transpose())
        .collect()
}

/// Read one pending message, or `None` when its file is gone; its file name
/// must be its id.
fn read_message(path: &Path) -> Result<Option<Message>> {
    let Some(file) =
        if_exists(File::open(path)).with_context(|| format!("opening {}", path.display()))?
    else {
        return Ok(None);
    };
    let message: Message =
        serde_json::from_reader(file).with_context(|| format!("parsing {}", path.display()))?;
    let file_id = path.file_stem().and_then(|stem| stem.to_str());
    if file_id != Some(message.id.0.as_str()) {
        bail!("mail id does not match {}", path.display());
    }
    Ok(Some(message))
}

/// The answer markers in `dir` with their times. Other files are skipped.
fn answer_markers(dir: &Path) -> Vec<(SystemTime, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            if !name.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let nanos: u128 = name.parse().ok()?;
            let at = SystemTime::UNIX_EPOCH.checked_add(Duration::new(
                (nanos / 1_000_000_000) as u64,
                (nanos % 1_000_000_000) as u32,
            ))?;
            Some((at, path))
        })
        .collect()
}

/// Start the workspace's detached mail courier (`horch mail courier`). It
/// exits at once when another courier holds the lock.
pub fn start_courier(ctx: &crate::runtime::RuntimeContext, workspace: &str) -> Result<()> {
    let Some(exe) = ctx.bins.current_exe.clone() else {
        bail!("the mail courier cannot start: the horch binary path is unknown");
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["mail", "courier", "--workspace", workspace])
        .env("HORCH_STATE_DIR", &ctx.paths.state_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::runtime::process::spawn_detached(&mut cmd)
        .with_context(|| format!("starting the mail courier {}", exe.display()))?;
    Ok(())
}

/// A horch notice that ends a wait: queue it as durable mail from `horch`
/// to `role`, and start the courier, which redelivers it until it is
/// acknowledged. A courier that cannot start is printed: the mail stays.
pub fn queue_notice(
    ctx: &crate::runtime::RuntimeContext,
    workspace: &str,
    role: &str,
    line: &str,
) -> Result<Id> {
    let mail = Mail::for_role(&ctx.paths.state_root, workspace, role);
    let id = mail.enqueue("horch", line)?;
    if let Err(error) = start_courier(ctx, workspace) {
        eprintln!("horch: {error:#}; the notice stays queued");
    }
    Ok(id)
}

/// Early retries: 30s, 60s, 120s, 240s, 300s, 300s; then every five minutes.
const REDELIVERY_GAPS: [u64; 6] = [30, 60, 120, 240, 300, 300];

/// Total age at which a one-based attempt is due; pending mail never expires.
pub fn redelivery_offset(attempt: usize) -> Option<Duration> {
    if attempt == 0 {
        return None;
    }
    let early: u64 = REDELIVERY_GAPS.iter().take(attempt).sum();
    let late = u64::try_from(attempt.saturating_sub(REDELIVERY_GAPS.len()))
        .ok()?
        .checked_mul(300)?;
    Some(Duration::from_secs(early.checked_add(late)?))
}

/// Claude Code login screen match strings; fixtures cover every table row.
pub const LOGIN_PROMPTS: &[&str] = &["Please run /login"];
pub fn needs_login(screen: &str) -> bool {
    LOGIN_PROMPTS.iter().any(|prompt| screen.contains(prompt))
}

pub fn redelivery_due(created_at: SystemTime, attempt: usize) -> Option<SystemTime> {
    created_at.checked_add(redelivery_offset(attempt)?)
}

fn next_id(created_at: SystemTime) -> Id {
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    mail_id(created_at, SystemTime::now(), std::process::id(), counter)
}

/// A mail id. `pending` lists mail by id, so the id sorts by `created_at`,
/// then by the real time of the enqueue: 2 processes that enqueue in the
/// same millisecond, or under a pinned clock, keep their order. The pid and
/// the counter only make the id unique.
fn mail_id(created_at: SystemTime, enqueued_at: SystemTime, pid: u32, counter: u64) -> Id {
    let since = |at: SystemTime| {
        at.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
    };
    let millis = since(created_at).as_millis();
    let nanos = since(enqueued_at).as_nanos();
    Id(format!("{millis:013}-{nanos:020}-{pid}-{counter}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs::File;
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn enqueue_lists_oldest_first_and_ack_moves_the_message() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");

        let first = mail.enqueue("sonnet-1", "first").unwrap();
        let second = mail.enqueue("opus-1", "second").unwrap();

        let pending = mail.pending().unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(pending[0].id, first);
        assert_eq!(pending[1].id, second);

        mail.ack(&first).unwrap();
        assert_eq!(mail.pending().unwrap().len(), 1);
        assert!(mail
            .dir
            .join("acked")
            .join(format!("{}.json", first.0))
            .is_file());
    }

    /// A concurrent `horch inbox` acks a message after `pending` lists the
    /// dir and before it opens the file. The acked message is no longer
    /// pending: `pending` returns the rest, and does not fail.
    #[test]
    fn a_message_acked_between_the_listing_and_the_read_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let first = mail.enqueue("sonnet-1", "first").unwrap();
        let second = mail.enqueue("opus-1", "second").unwrap();

        let listed = mail.pending_paths().unwrap();
        mail.ack(&first).unwrap();
        let pending = read_pending(&listed).unwrap();

        assert_eq!(pending.iter().map(|m| &m.id).collect::<Vec<_>>(), [&second]);
    }

    #[test]
    fn concurrent_enqueue_produces_unique_ids() {
        let root = tempfile::tempdir().unwrap();
        let mail = Arc::new(Mail::under(root.path(), "w1"));
        let threads: Vec<_> = (0..2)
            .map(|thread| {
                let mail = Arc::clone(&mail);
                std::thread::spawn(move || {
                    (0..50)
                        .map(|message| mail.enqueue("worker", &format!("{thread}-{message}")))
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();

        let ids: Vec<_> = threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap().unwrap())
            .collect();
        assert_eq!(ids.len(), 100);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 100);
        assert_eq!(mail.pending().unwrap().len(), 100);
    }

    #[test]
    fn ack_of_a_missing_message_succeeds() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");

        mail.ack(&Id("missing".to_string())).unwrap();
    }

    #[test]
    fn enqueue_prunes_acked_files_older_than_seven_days() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let acked = mail.dir.join("acked");
        std::fs::create_dir_all(&acked).unwrap();
        let old = acked.join("old.json");
        let recent = acked.join("recent.json");
        File::create(&old).unwrap();
        File::create(&recent).unwrap();
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
            .unwrap();

        mail.enqueue("worker", "message").unwrap();

        assert!(!old.exists());
        assert!(recent.exists());
    }

    fn plant_old_acked(mail: &Mail, count: usize) {
        let acked = mail.dir.join("acked");
        std::fs::create_dir_all(&acked).unwrap();
        let old = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
        for n in 0..count {
            let file = File::create(acked.join(format!("old-{n}.json"))).unwrap();
            file.set_modified(old).unwrap();
        }
    }

    #[test]
    fn concurrent_enqueues_that_prune_the_same_acked_files_lose_no_message() {
        for round in 0..20 {
            let root = tempfile::tempdir().unwrap();
            let mail = Arc::new(Mail::under(root.path(), "w1"));
            plant_old_acked(&mail, 200);
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let threads: Vec<_> = (0..2)
                .map(|thread| {
                    let mail = Arc::clone(&mail);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        mail.enqueue("worker", &format!("{round}-{thread}"))
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap().unwrap();
            }
            assert_eq!(mail.pending().unwrap().len(), 2, "round {round}");
            assert_eq!(
                std::fs::read_dir(mail.dir.join("acked")).unwrap().count(),
                0
            );
        }
    }

    #[test]
    fn an_acked_file_removed_before_its_metadata_is_read_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        plant_old_acked(&mail, 1);
        let gone = mail.dir.join("acked").join("gone.json");

        mail.prune_paths(vec![Ok(gone), Ok(mail.dir.join("acked/old-0.json"))])
            .unwrap();

        assert!(!mail.dir.join("acked/old-0.json").exists());
        mail.enqueue("worker", "message").unwrap();
        assert_eq!(mail.pending().unwrap().len(), 1);
    }

    #[test]
    fn an_unreadable_acked_dir_does_not_block_enqueue() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        std::fs::create_dir_all(&mail.dir).unwrap();
        // A file in place of the dir makes read_dir fail with NotADirectory.
        std::fs::write(mail.dir.join("acked"), b"").unwrap();

        mail.enqueue("worker", "message").unwrap();

        let pending = mail.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].text, "message");
    }

    #[test]
    fn role_mail_is_separate_from_orchestrator_mail_and_listed_by_role() {
        let root = tempfile::tempdir().unwrap();
        let orchestrator = Mail::under(root.path(), "w1");
        let worker = Mail::to_role(root.path(), "w1", "opus-1");
        orchestrator.enqueue("opus-1", "report").unwrap();
        worker.enqueue("horch", "reviews in").unwrap();
        worker.enqueue("horch", "reviews in again").unwrap();

        assert_eq!(orchestrator.pending().unwrap().len(), 1);
        let roles = orchestrator.roles().unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].0, "opus-1");
        assert_eq!(roles[0].1.pending().unwrap().len(), 2);

        assert_eq!(worker.ack_all().unwrap(), 2);
        assert!(worker.pending().unwrap().is_empty());
        assert_eq!(orchestrator.pending().unwrap().len(), 1);
    }

    #[test]
    fn queue_notice_leaves_pending_mail_when_no_courier_can_start() {
        use crate::runtime::{MapEnv, RuntimeContext};
        let root = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/").with("HORCH_STATE_DIR", root.path().to_str().unwrap()),
        )
        .unwrap();
        assert!(ctx.bins.current_exe.is_none());

        queue_notice(&ctx, "w1", "orchestrator", "[horch] BLOCKED: x").unwrap();
        queue_notice(&ctx, "w1", "opus-1", "[horch] NOTE: y").unwrap();

        let orchestrator = Mail::under(root.path(), "w1").pending().unwrap();
        assert_eq!(orchestrator.len(), 1);
        // A horch BLOCKED that names no horch command is a NOTE (C2).
        assert_eq!(orchestrator[0].text, "[horch] NOTE: x");
        assert_eq!(orchestrator[0].from, "horch");
        let worker = Mail::to_role(root.path(), "w1", "opus-1")
            .pending()
            .unwrap();
        assert_eq!(worker[0].text, "[horch] NOTE: y");
    }

    /// #77: a new mail dir is 0700 and a mail file 0600. Mail in a dir
    /// that others can write is refused by name.
    #[cfg(unix)]
    #[test]
    fn mail_dirs_are_0700_files_0600_and_an_open_dir_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let id = mail.enqueue("worker", "QUESTION: which?").unwrap();
        assert_eq!(mode(&mail.dir), 0o700);
        assert_eq!(mode(&mail.dir.join("pending")), 0o700);
        let file = mail.dir.join("pending").join(format!("{}.json", id.0));
        assert_eq!(mode(&file), 0o600);
        let question = mail.dir.join("questions").join(format!("{}.json", id.0));
        assert_eq!(mode(&question), 0o600);

        std::fs::set_permissions(&mail.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = format!("{:#}", mail.pending().unwrap_err());
        assert!(err.contains(&mail.dir.display().to_string()), "{err}");
        assert!(mail.enqueue("worker", "more").is_err());
    }

    fn question(mail: &Mail, from: &str, text: &str) -> Id {
        let id = mail.enqueue(from, text).unwrap();
        mail.ack(&id).unwrap();
        id
    }

    fn open_ids(mail: &Mail) -> Vec<Id> {
        mail.open_questions()
            .unwrap()
            .into_iter()
            .map(|q| q.id)
            .collect()
    }

    /// TRF-25: an answer by id closes that question and no other one of
    /// the same role. `#` before the short id is optional.
    #[test]
    fn trf_25_close_question_closes_only_the_named_question() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let first = question(&mail, "worker", "QUESTION: which?");
        let second = question(&mail, "worker", "QUESTION: where?");
        let other = question(&mail, "other", "BLOCKED: no key");

        let closed = mail
            .close_question(&format!("#{}", second.short()))
            .unwrap()
            .expect("the question is open");
        assert_eq!(closed.id, second);
        assert_eq!(open_ids(&mail), vec![first.clone(), other.clone()]);

        assert!(mail.close_question(&second.short()).unwrap().is_none());
        assert!(mail.close_question("00000000").unwrap().is_none());
        assert_eq!(mail.close_question(&first.0).unwrap().unwrap().id, first);
        assert_eq!(open_ids(&mail), vec![other]);
    }

    /// TRF-25: nothing but an answer by id closes a question. New mail of
    /// the same role leaves it open.
    #[test]
    fn trf_25_new_mail_of_a_role_leaves_its_question_open() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let id = question(&mail, "worker", "QUESTION: which?");
        mail.enqueue("worker", "NOTE: still here").unwrap();
        assert_eq!(open_ids(&mail), vec![id]);
    }

    /// TRF-26: a horch question keeps the ledger actions it names; a
    /// worker question names none.
    #[test]
    fn trf_26_a_horch_question_records_the_actions_that_close_it() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let text = "[horch] QUESTION: T-7 is held: 3 attempts failed. Ask the operator: \
                    requeue it (horch ledger requeue T-7 \"<reason>\"), or cancel it \
                    (horch ledger cancel T-7 \"<reason>\").";
        mail.enqueue("horch", text).unwrap();
        mail.enqueue("worker", "QUESTION: run horch ledger cancel T-7?")
            .unwrap();
        let open = mail.open_questions().unwrap();
        assert_eq!(open[0].closes_on, vec!["requeue T-7", "cancel T-7"]);
        assert!(open[1].closes_on.is_empty());
    }

    /// TRF-26: a horch QUESTION or BLOCKED that names no horch command is
    /// queued as a NOTE, with no question file. One that names a command
    /// keeps its keyword.
    #[test]
    fn trf_26_a_horch_question_without_a_closing_action_is_a_note() {
        assert_eq!(
            horch_line("[horch] BLOCKED: Compaction of a failed. Log: x."),
            "[horch] NOTE: Compaction of a failed. Log: x."
        );
        assert_eq!(
            horch_line("[horch #0a1b2c3d] QUESTION: which one? Log: /a/horch/x."),
            "[horch #0a1b2c3d] NOTE: which one? Log: /a/horch/x."
        );
        // A named horch command keeps the keyword.
        let compact = "[horch] BLOCKED: Compaction of a failed. Run horch compact a --request.";
        assert_eq!(horch_line(compact), compact);
        assert_eq!(horch_line("QUESTION: why?\n"), "NOTE: why?\n");
        let kept = "[horch] QUESTION: T-2 is held. horch ledger requeue T-2 \"<reason>\"";
        assert_eq!(horch_line(kept), kept);
        assert_eq!(horch_line("[horch] NOTE: x"), "[horch] NOTE: x");

        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("horch", "[horch] BLOCKED: boom").unwrap();
        assert!(mail.open_questions().unwrap().is_empty());
        assert_eq!(mail.pending().unwrap()[0].text, "[horch] NOTE: boom");
        // A kept BLOCKED with no closing ledger action is mail, not a question.
        mail.enqueue("horch", compact).unwrap();
        assert!(mail.open_questions().unwrap().is_empty());
        mail.enqueue("worker", "[worker] BLOCKED: no key").unwrap();
        assert_eq!(mail.open_questions().unwrap().len(), 1);
    }

    #[test]
    fn trf_26_ledger_actions_reads_each_named_action_once() {
        assert_eq!(
            ledger_actions("(horch ledger requeue T-12 \"x\"), horch ledger requeue T-12."),
            vec!["requeue T-12"]
        );
        assert!(ledger_actions("horch ledger status").is_empty());
        assert!(ledger_actions("horch ledger cancel T-x now").is_empty());
        assert!(ledger_actions("horch spawn opus").is_empty());
    }

    /// A fleet that ran an earlier release has its answer in
    /// `answers/<role>.json` or as a marker in `answers/<role>/`: both
    /// still count, and the later one wins.
    #[test]
    fn an_answer_of_an_earlier_release_still_counts() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_900_000_000);
        private::ensure_dir(&mail.dir.join("answers")).unwrap();
        std::fs::write(
            mail.dir.join("answers/worker.json"),
            serde_json::to_vec(&at).unwrap(),
        )
        .unwrap();
        assert_eq!(mail.answered_at("worker"), Some(at));
        let marker = at + Duration::from_secs(60);
        let nanos = marker
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        private::ensure_dir(&mail.answers_dir("worker")).unwrap();
        std::fs::write(mail.answers_dir("worker").join(format!("{nanos:039}")), "").unwrap();
        assert_eq!(mail.answered_at("worker"), Some(marker));
    }

    /// 2 processes enqueue under the same `created_at` (a pinned clock, or
    /// the same millisecond). The later enqueue sorts last, whatever the
    /// pids: pid 10000 sorts before pid 9999 as text.
    #[test]
    fn ids_of_one_millisecond_sort_by_enqueue_time_not_by_pid() {
        let created = SystemTime::UNIX_EPOCH + Duration::from_secs(1_900_000_000);
        let first = mail_id(created, created + Duration::from_micros(10), 9999, 0);
        let second = mail_id(created, created + Duration::from_micros(20), 10000, 0);
        assert!(first.0 < second.0, "{first:?} {second:?}");
        let earlier = mail_id(created, created + Duration::from_secs(5), 1, 0);
        let later = mail_id(created + Duration::from_millis(1), created, 1, 0);
        assert!(earlier.0 < later.0, "created_at comes first");
    }

    #[test]
    fn redelivery_schedule_caps_at_five_minutes_and_continues() {
        let created_at = SystemTime::UNIX_EPOCH;
        // 30 s, 1 m, 2 m, 4 m, 5 m and 5 m apart.
        let expected = [30, 90, 210, 450, 750, 1050];

        for (attempt, seconds) in (1..=6).zip(expected) {
            assert_eq!(
                redelivery_due(created_at, attempt),
                Some(created_at + Duration::from_secs(seconds))
            );
        }
        assert_eq!(redelivery_due(created_at, 0), None);
        assert_eq!(
            redelivery_due(created_at, 7),
            Some(created_at + Duration::from_secs(1350))
        );
    }

    #[test]
    fn enqueue_keeps_only_question_and_blocked_lines_of_the_original() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let original =
            "progress note\n[worker] QUESTION: which?\n  BLOCKED: no key\nQUESTIONS: no\ndone";
        let id = mail
            .enqueue_with_questions("worker", "short", original)
            .unwrap();

        let open = mail.open_questions().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, id);
        assert_eq!(open[0].text, "[worker] QUESTION: which?\n  BLOCKED: no key");
        assert_eq!(mail.pending().unwrap()[0].text, "short");
    }

    #[test]
    fn enqueue_without_question_lines_writes_no_question_file() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("worker", "plain status").unwrap();

        assert!(mail.open_questions().unwrap().is_empty());
        assert!(!mail.dir.join("questions").exists());
    }

    #[test]
    fn pending_refuses_a_file_whose_name_is_not_its_id() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let id = mail.enqueue("worker", "hello").unwrap();
        let dir = mail.dir.join("pending");
        std::fs::rename(dir.join(format!("{}.json", id.0)), dir.join("other.json")).unwrap();

        let err = format!("{:#}", mail.pending().unwrap_err());
        assert!(err.contains("mail id does not match"), "{err}");
        assert!(err.contains("other.json"), "{err}");
    }

    #[test]
    fn pending_names_a_file_that_is_not_json() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("worker", "hello").unwrap();
        std::fs::write(mail.dir.join("pending").join("bad.json"), "{").unwrap();

        let err = format!("{:#}", mail.pending().unwrap_err());
        assert!(err.contains("parsing"), "{err}");
        assert!(err.contains("bad.json"), "{err}");
    }

    #[test]
    fn pending_and_roles_of_a_missing_dir_are_empty() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.ensure_dir().unwrap();
        assert!(mail.pending().unwrap().is_empty());
        assert!(mail.roles().unwrap().is_empty());
    }

    #[test]
    fn pending_ignores_files_without_a_json_extension() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("worker", "hello").unwrap();
        std::fs::write(mail.dir.join("pending").join("note.txt"), "x").unwrap();
        assert_eq!(mail.pending().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_publish_removes_the_temp_file_and_the_question() {
        let root = tempfile::tempdir().unwrap();
        let temp = root.path().join(".a.json.tmp");
        let question = root.path().join("q.json");
        std::fs::write(&temp, "{}").unwrap();
        std::fs::write(&question, "{}").unwrap();
        let blocked = root.path().join("a.json");
        std::fs::create_dir_all(blocked.join("child")).unwrap();

        let err = publish_pending(&temp, &blocked, Some(&question)).unwrap_err();
        assert!(format!("{err:#}").contains("moving"), "{err:#}");
        assert!(!temp.exists());
        assert!(!question.exists());
    }

    /// An id names a file in `pending/` and in `acked/`. One that holds a
    /// separator, a dot or a space is refused. Without the check, `a/b` would
    /// read as a message that is already gone, and `ack` would say Ok.
    #[test]
    fn ack_refuses_an_id_that_could_name_another_path() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("worker", "hello").unwrap();

        for bad in ["../x", "a/b", "a.b", "a b"] {
            let err = mail.ack(&Id(bad.into())).unwrap_err();
            assert_eq!(err.to_string(), "invalid mail id", "{bad:?}");
        }
        assert_eq!(mail.pending().unwrap().len(), 1, "nothing was moved");
    }

    #[test]
    fn trf_03_a_mail_id_shortens_to_8_hex_characters() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let first = mail.enqueue("worker", "one").unwrap();
        let second = mail.enqueue("worker", "two").unwrap();

        let short = first.short();
        assert_eq!(short.len(), 8, "{short}");
        assert!(short.chars().all(|c| c.is_ascii_hexdigit()), "{short}");
        assert_ne!(short, second.short());
        // The short id comes from the file name, so every reader agrees.
        assert_eq!(mail.pending().unwrap()[0].id.short(), short);
        assert_eq!(Id(first.0.clone()).short(), short);
    }

    fn pending_files(mail: &Mail) -> usize {
        std::fs::read_dir(mail.dir.join("pending"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "json")
            })
            .count()
    }

    /// TRF-21: an enqueue with an idempotency key reuses the first mail of
    /// that key, pending or acknowledged. Another key is new mail.
    #[test]
    fn trf_21_a_keyed_enqueue_reuses_its_first_mail() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let text = "[horch] NOTE: x";
        let first = mail.enqueue_with_key("row-1", "horch", text, text).unwrap();
        let again = mail.enqueue_with_key("row-1", "horch", text, text).unwrap();
        assert_eq!(again, first);
        assert_eq!(pending_files(&mail), 1);
        let other = mail.enqueue_with_key("row-2", "horch", text, text).unwrap();
        assert_ne!(other, first);
        assert_eq!(pending_files(&mail), 2);

        mail.ack(&first).unwrap();
        let acked = mail.enqueue_with_key("row-1", "horch", text, text).unwrap();
        assert_eq!(acked, first, "an acknowledged mail is reused");
        assert_eq!(pending_files(&mail), 1, "and not queued again");
    }

    /// TRF-21: a crash after the key names an id and before the mail is
    /// published leaves a temp file. The replay publishes that id once.
    #[test]
    fn trf_21_a_keyed_replay_after_a_crash_publishes_the_named_id() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let text = "[horch] QUESTION: T-7 is held. Run horch ledger requeue T-7.";
        let first = mail.enqueue_with_key("row-1", "horch", text, text).unwrap();
        // The crash: the pending file is not yet in place; its temp file is.
        let pending = mail.dir.join("pending");
        std::fs::rename(
            pending.join(format!("{}.json", first.0)),
            pending.join(format!(".{}.json.tmp", first.0)),
        )
        .unwrap();
        std::fs::write(
            mail.dir.join("questions").join(format!(".{}.tmp", first.0)),
            b"x",
        )
        .unwrap();

        let again = mail.enqueue_with_key("row-1", "horch", text, text).unwrap();
        assert_eq!(again, first);
        assert_eq!(pending_files(&mail), 1);
        assert_eq!(mail.pending().unwrap()[0].id.short(), first.short());
        assert_eq!(mail.open_questions().unwrap().len(), 1, "1 question file");
    }

    /// TRF-21: [`keyed`] gives its key to the first enqueue in its scope
    /// only. A later enqueue is new mail.
    #[test]
    fn trf_21_keyed_scopes_the_key_to_1_enqueue() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let first = keyed("row-1", || mail.enqueue("horch", "[horch] NOTE: x")).unwrap();
        let (again, plain) = keyed("row-1", || {
            (
                mail.enqueue("horch", "[horch] NOTE: x"),
                mail.enqueue("horch", "y"),
            )
        });
        assert_eq!(again.unwrap(), first);
        assert_ne!(plain.unwrap(), first);
        assert_ne!(mail.enqueue("horch", "[horch] NOTE: x").unwrap(), first);
        assert_eq!(pending_files(&mail), 3);
    }

    #[test]
    fn trf_03_a_mail_line_carries_its_mail_id() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        let id = mail.enqueue("sonnet-1", "[sonnet-1] NOTE: x").unwrap();

        let message = &mail.pending().unwrap()[0];
        assert_eq!(
            message.line(),
            format!("[sonnet-1 #{}] NOTE: x", id.short())
        );
        assert_eq!(
            message.text, "[sonnet-1] NOTE: x",
            "the stored text is unchanged"
        );
    }
}
