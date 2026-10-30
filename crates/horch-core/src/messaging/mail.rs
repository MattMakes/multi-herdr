//! Durable mail: worker-to-orchestrator, and horch's own notices to a
//! worker role (`roles/<role>/` under the workspace's mail dir).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::messaging::private;

const ACK_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct Id(String);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub id: Id,
    pub from: String,
    pub text: String,
    pub created_at: SystemTime,
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
        let dir = self.dir.join("roles");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
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
    pub fn enqueue_with_questions(&self, from: &str, text: &str, original: &str) -> Result<Id> {
        let pending = self.dir.join("pending");
        self.ensure_dir()?;
        private::ensure_dir(&pending)?;
        // Cleanup is best effort: a prune error must never block a send.
        if let Err(error) = self.prune_acked() {
            eprintln!("horch: pruning acked mail: {error:#}");
        }

        let created_at = crate::clock::now().into();
        let id = next_id(created_at);
        let message = Message {
            id: id.clone(),
            from: from.to_string(),
            text: text.to_string(),
            created_at,
        };
        let final_path = pending.join(format!("{}.json", id.0));
        let temp_path = pending.join(format!(".{}.json.tmp", id.0));
        let mut file = private::create_new(&temp_path)
            .with_context(|| format!("creating {}", temp_path.display()))?;
        let bytes = serde_json::to_vec(&message).context("serializing mail")?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("writing {}", temp_path.display()));
        }
        let questions = original
            .lines()
            .filter(|line| {
                let line = line.trim_start();
                let line = line
                    .strip_prefix(&format!("[{from}]"))
                    .unwrap_or(line)
                    .trim_start();
                line.starts_with("QUESTION:") || line.starts_with("BLOCKED:")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let question_path = if questions.is_empty() {
            None
        } else {
            let dir = self.dir.join("questions");
            private::ensure_dir(&dir)?;
            let path = dir.join(format!("{}.json", id.0));
            let temporary = dir.join(format!(".{}.tmp", id.0));
            let question = Message {
                text: questions,
                ..message
            };
            let bytes = serde_json::to_vec(&question)?;
            let mut file = private::create_new(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            Some(path)
        };
        // Publish questions first: a courier/inbox can observe pending mail as
        // soon as this rename completes, and acknowledgement must not erase them.
        if let Err(error) = std::fs::rename(&temp_path, &final_path) {
            let _ = std::fs::remove_file(&temp_path);
            if let Some(path) = question_path {
                let _ = std::fs::remove_file(path);
            }
            return Err(error).with_context(|| {
                format!("moving {} to {}", temp_path.display(), final_path.display())
            });
        }
        Ok(id)
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

    /// An orchestrator reply closes questions created before this tell.
    pub fn answer(&self, role: &str) -> Result<()> {
        self.mark_answered(role, crate::clock::now().into())
    }

    /// Leave an empty marker named by `at` in `answers/<role>/`. The newest
    /// marker is the answer time. No lock: replies that overlap each leave
    /// their own marker, so an older one cannot replace a newer one, and no
    /// reply waits for another.
    fn mark_answered(&self, role: &str, at: SystemTime) -> Result<()> {
        let dir = self.answers_dir(role);
        self.ensure_dir()?;
        private::ensure_dir(&dir)?;
        let name = answer_marker(at).context("an answer time before 1970")?;
        match private::create_new(&dir.join(name)) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(e).with_context(|| format!("writing an answer in {}", dir.display()))
            }
        }
        // Markers older than the newest no longer count. The newest is never
        // removed, so a race here cannot make the answer older.
        let markers = answer_markers(&dir);
        if let Some(newest) = markers.iter().map(|(at, _)| *at).max() {
            for (at, path) in markers {
                if at < newest {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        Ok(())
    }

    /// When `role` was last answered: the newest marker, or the answer file
    /// an earlier release wrote, whichever is later.
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
        let dir = self.dir.join("pending");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
        };
        let mut paths = entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        });
        paths.sort();

        paths
            .into_iter()
            .map(|path| {
                let file =
                    File::open(&path).with_context(|| format!("opening {}", path.display()))?;
                let message: Message = serde_json::from_reader(file)
                    .with_context(|| format!("parsing {}", path.display()))?;
                let file_id = path.file_stem().and_then(|stem| stem.to_str());
                if file_id != Some(message.id.0.as_str()) {
                    bail!("mail id does not match {}", path.display());
                }
                Ok(message)
            })
            .collect()
    }

    pub fn ack(&self, id: &Id) -> Result<()> {
        if !id.0.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
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
        let dir = self.dir.join("acked");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
        };
        self.prune_paths(entries.map(|entry| entry.map(|entry| entry.path())))
    }

    /// Another enqueue can prune the same file first, so NotFound is skipped.
    fn prune_paths(&self, paths: impl IntoIterator<Item = std::io::Result<PathBuf>>) -> Result<()> {
        let now = SystemTime::now();
        for path in paths {
            let path = match path {
                Ok(path) => path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("reading an acked mail entry"),
            };
            let modified = match path.metadata().and_then(|metadata| metadata.modified()) {
                Ok(modified) => modified,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("reading metadata for {}", path.display()))
                }
            };
            if now
                .duration_since(modified)
                .is_ok_and(|age| age > ACK_RETENTION)
            {
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| format!("removing {}", path.display()))
                    }
                }
            }
        }
        Ok(())
    }
}

/// The marker name for an answer at `at`: nanoseconds since 1970, padded,
/// so names sort by time.
fn answer_marker(at: SystemTime) -> Option<String> {
    let nanos = at.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_nanos();
    Some(format!("{nanos:039}"))
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
    let millis = created_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    Id(format!("{millis:013}-{}-{counter}", std::process::id()))
}

fn file_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
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
        assert_eq!(orchestrator[0].text, "[horch] BLOCKED: x");
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
        mail.answer("worker").unwrap();
        assert_eq!(mode(&mail.dir), 0o700);
        assert_eq!(mode(&mail.dir.join("pending")), 0o700);
        let file = mail.dir.join("pending").join(format!("{}.json", id.0));
        assert_eq!(mode(&file), 0o600);
        let question = mail.dir.join("questions").join(format!("{}.json", id.0));
        assert_eq!(mode(&question), 0o600);
        assert_eq!(mode(&mail.dir.join("answers/worker")), 0o700);
        let marker = std::fs::read_dir(mail.dir.join("answers/worker"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(mode(&marker.path()), 0o600);

        std::fs::set_permissions(&mail.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = format!("{:#}", mail.pending().unwrap_err());
        assert!(err.contains(&mail.dir.display().to_string()), "{err}");
        assert!(mail.enqueue("worker", "more").is_err());
    }

    /// A reply takes no lock: a held lock of an earlier release, or a slow
    /// reply, does not hold it. A held lock made `answer` fail after 5 s.
    #[test]
    fn an_answer_does_not_wait_for_another_reply() {
        let root = tempfile::tempdir().unwrap();
        let mail = Mail::under(root.path(), "w1");
        mail.enqueue("worker", "QUESTION: which?").unwrap();
        let answers = mail.dir.join("answers");
        private::ensure_dir(&answers).unwrap();
        let _held = crate::fsx::DirLock::acquire(
            &answers,
            "answer-worker",
            Duration::from_secs(300),
            Duration::from_secs(1),
        )
        .unwrap();

        let started = std::time::Instant::now();
        mail.answer("worker").unwrap();

        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(mail.open_questions().unwrap().is_empty());
    }

    /// Replies that overlap keep the newest answer: an older reply that
    /// lands last does not reopen a question asked between the 2 replies.
    #[test]
    fn overlapping_replies_keep_the_newest_answer() {
        let root = tempfile::tempdir().unwrap();
        let mail = std::sync::Arc::new(Mail::under(root.path(), "w1"));
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_900_000_000);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8u64)
            .map(|n| {
                let mail = std::sync::Arc::clone(&mail);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    mail.mark_answered("worker", base + Duration::from_secs(n))
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        mail.mark_answered("worker", base).unwrap();

        assert_eq!(
            mail.answered_at("worker"),
            Some(base + Duration::from_secs(7))
        );
    }

    /// A fleet that ran an earlier release has its answer in
    /// `answers/<role>.json`: it still counts.
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
        mail.mark_answered("worker", at - Duration::from_secs(60))
            .unwrap();
        assert_eq!(mail.answered_at("worker"), Some(at));
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
}
