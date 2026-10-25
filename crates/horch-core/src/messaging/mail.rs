//! Durable worker-to-orchestrator mail.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

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

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn enqueue(&self, from: &str, text: &str) -> Result<Id> {
        let pending = self.dir.join("pending");
        std::fs::create_dir_all(&pending)
            .with_context(|| format!("creating {}", pending.display()))?;
        self.prune_acked()?;

        let created_at = SystemTime::now();
        let id = next_id(created_at);
        let message = Message {
            id: id.clone(),
            from: from.to_string(),
            text: text.to_string(),
            created_at,
        };
        let final_path = pending.join(format!("{}.json", id.0));
        let temp_path = pending.join(format!(".{}.json.tmp", id.0));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("creating {}", temp_path.display()))?;
        let bytes = serde_json::to_vec(&message).context("serializing mail")?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("writing {}", temp_path.display()));
        }
        if let Err(error) = std::fs::rename(&temp_path, &final_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error).with_context(|| {
                format!("moving {} to {}", temp_path.display(), final_path.display())
            });
        }
        Ok(id)
    }

    pub fn pending(&self) -> Result<Vec<Message>> {
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
        std::fs::create_dir_all(&acked_dir)
            .with_context(|| format!("creating {}", acked_dir.display()))?;
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
        let now = SystemTime::now();
        for entry in entries {
            let path = entry?.path();
            let modified = path
                .metadata()
                .with_context(|| format!("reading metadata for {}", path.display()))?
                .modified()
                .with_context(|| format!("reading modified time for {}", path.display()))?;
            if now
                .duration_since(modified)
                .is_ok_and(|age| age > ACK_RETENTION)
            {
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
        }
        Ok(())
    }
}

/// The wait before each redelivery, from the one before it (or from the
/// enqueue, for the first): 30 s doubling, capped at 5 min, 6 in all.
const REDELIVERY_GAPS: [u64; 6] = [30, 60, 120, 240, 300, 300];

/// How long after the enqueue redelivery `attempt` (1-based) is due. `None`
/// after the last one.
pub fn redelivery_offset(attempt: usize) -> Option<Duration> {
    let gaps = REDELIVERY_GAPS.get(..attempt).filter(|_| attempt > 0)?;
    Some(Duration::from_secs(gaps.iter().sum()))
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

    #[test]
    fn redelivery_schedule_caps_at_five_minutes_and_stops_after_six() {
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
        assert_eq!(redelivery_due(created_at, 7), None);
    }
}
