use std::hash::{Hash, Hasher};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Deserialize;

fn requests_dir(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("quota-refresh")
}

pub(crate) fn request(state_root: &Path) -> Result<PathBuf> {
    let dir = requests_dir(state_root);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = dir.join(format!("{}-{nanos}", std::process::id()));
    std::fs::File::create_new(&path).with_context(|| format!("creating {}", path.display()))?;
    Ok(path)
}

pub(crate) fn pending(state_root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(requests_dir(state_root))
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

pub(crate) fn clear(requests: &[PathBuf]) {
    for path in requests {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Deserialize)]
struct ClaudeConfig {
    #[serde(rename = "oauthAccount")]
    account: Option<ClaudeAccount>,
}

#[derive(Deserialize)]
struct ClaudeAccount {
    #[serde(rename = "accountUuid")]
    id: Option<String>,
}

fn claude_account_hash(path: &Path) -> Option<Option<u64>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    let config: ClaudeConfig = serde_json::from_reader(BufReader::new(file)).ok()?;
    Some(config.account.and_then(|a| a.id).map(|id| {
        let mut hasher = std::hash::DefaultHasher::new();
        id.hash(&mut hasher);
        hasher.finish()
    }))
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

struct ClaudeWatch {
    path: PathBuf,
    mtime: Option<Option<SystemTime>>,
    account: Option<Option<u64>>,
}

impl ClaudeWatch {
    fn changed(&mut self) -> bool {
        let mtime = modified(&self.path);
        if self.mtime == Some(mtime) {
            return false;
        }
        let Some(account) = claude_account_hash(&self.path) else {
            return false;
        };
        self.mtime = Some(mtime);
        self.account
            .replace(account)
            .is_some_and(|old| old != account)
    }
}

struct MetadataWatch {
    path: PathBuf,
    seen: Option<Option<(u64, Option<SystemTime>)>>,
}

impl MetadataWatch {
    fn new(path: PathBuf) -> Self {
        MetadataWatch { path, seen: None }
    }

    fn changed(&mut self) -> bool {
        let now = std::fs::metadata(&self.path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        self.seen.replace(now).is_some_and(|old| old != now)
    }
}

pub(crate) struct AccountWatch {
    claude: ClaudeWatch,
    codex: MetadataWatch,
    google: MetadataWatch,
}

impl AccountWatch {
    pub(crate) fn new(home: &Path, claude_dir: Option<&Path>, codex_dir: Option<&Path>) -> Self {
        let codex_home = crate::harness::codex::codex_home(home, codex_dir);
        AccountWatch {
            claude: ClaudeWatch {
                path: crate::harness::trust::claude_config_file(home, claude_dir),
                mtime: None,
                account: None,
            },
            codex: MetadataWatch::new(codex_home.join("auth.json")),
            google: MetadataWatch::new(home.join(".gemini/google_accounts.json")),
        }
    }

    pub(crate) fn changed(&mut self) -> bool {
        let claude = self.claude.changed();
        let codex = self.codex.changed();
        let google = self.google.changed();
        claude || codex || google
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::clock;
    use crate::routing::policy::Policy;
    use crate::routing::quota::QuotaFile;
    use crate::routing::quota_probe::ProbeBins;
    use crate::routing::snapshot;
    use crate::telemetry::collect::{Collector, Probing};
    use crate::usage::Locations;
    use chrono::{DateTime, Duration, Utc};
    use std::time::Duration as StdDuration;

    const ACCOUNT: &str = "acct-5f1e-private-id";

    fn at(minute: i64) -> DateTime<Utc> {
        clock::parse("2026-11-22T10:00:00Z").unwrap() + Duration::minutes(minute)
    }

    struct Rig {
        _tmp: tempfile::TempDir,
        state: PathBuf,
        home: PathBuf,
        log: PathBuf,
        bins: ProbeBins,
    }

    fn rig(resets_at: &str) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let log = tmp.path().join("probes");
        let claude = tmp.path().join("claude");
        let reply = format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"horch-quota-1","response":{{"rate_limits_available":true,"rate_limits":{{"five_hour":{{"utilization":12,"resets_at":"{resets_at}"}}}}}}}}}}"#
        );
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nread line\necho probe >> '{}'\necho '{reply}'\n",
            log.display()
        );
        assert!(
            crate::test_support::write_warmed(&claude, &script, &["warm"])
                .status
                .success()
        );
        let absent = tmp.path().join("absent");
        let bins = ProbeBins {
            claude,
            codex: absent.clone(),
            antigravity: absent.clone(),
            pi: absent.clone(),
            ollama: absent,
            codex_sessions: home.join(".codex/sessions"),
            harnesses: Vec::new(),
            local_timeout: StdDuration::from_secs(600),
        };
        Rig {
            _tmp: tmp,
            state,
            home,
            log,
            bins,
        }
    }

    impl Rig {
        fn collector(&self, probing: Probing) -> Collector {
            let home = self.home.clone();
            let loc = Locations {
                home: home.clone(),
                claude_projects: home.join(".claude/projects"),
                codex_sessions: home.join(".codex/sessions"),
                pi_sessions: home.join(".pi/agent/sessions"),
                opencode_db: home.join(".local/share/opencode/opencode.db"),
                sqlite3: PathBuf::from("sqlite3"),
            };
            let mut c = Collector::open_at(&self.state, loc, probing, at(0)).unwrap();
            c.bins = self.bins.clone();
            c.quota_env.bins = self.bins.clone();
            c.quota_env.probe_timeout = Some(StdDuration::from_secs(600));
            c
        }

        fn probes(&self) -> usize {
            std::fs::read_to_string(&self.log).map_or(0, |t| t.lines().count())
        }

        fn write_claude(&self, id: &str, mtime_s: u64) {
            let path = self.home.join(".claude.json");
            let body = serde_json::json!({"oauthAccount": {"accountUuid": id}});
            std::fs::write(&path, body.to_string()).unwrap();
            set_mtime(&path, mtime_s);
        }
    }

    fn set_mtime(path: &Path, seconds: u64) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + StdDuration::from_secs(seconds))
            .unwrap();
    }

    const FUTURE: &str = "2026-11-23T10:00:00Z";

    #[test]
    fn a_scheduled_collector_probes_once_on_its_first_tick() {
        let r = rig(FUTURE);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        assert_eq!(r.probes(), 1);
        c.tick(at(1)).unwrap();
        assert_eq!(r.probes(), 1);
        let file = QuotaFile::load(&r.state, None).unwrap();
        let mut restarted = r.collector(Probing::Scheduled);
        assert!(file.pools["claude"].probed_at.is_some());
        restarted.tick(at(2)).unwrap();
        assert_eq!(
            r.probes(),
            2,
            "a restart probes although the reading is fresh"
        );
    }

    #[test]
    fn a_collector_that_never_probes_ignores_every_trigger() {
        let r = rig(FUTURE);
        let mut c = r.collector(Probing::Never);
        let waiting = request(&r.state).unwrap();
        c.tick(at(0)).unwrap();
        assert_eq!(r.probes(), 0);
        assert!(waiting.exists());
    }

    #[test]
    fn a_passed_reset_probes_at_most_once_per_minute() {
        let r = rig("2026-11-22T10:00:30Z");
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        assert_eq!(r.probes(), 1);
        let second = at(0) + Duration::seconds(40);
        c.tick(second).unwrap();
        assert_eq!(r.probes(), 1, "under 1 min since the last probe");
        c.tick(at(1)).unwrap();
        assert_eq!(r.probes(), 2, "the stored reset has passed");
        c.tick(at(1) + Duration::seconds(59)).unwrap();
        assert_eq!(r.probes(), 2, "the pool keeps the old reset");
        c.tick(at(2)).unwrap();
        assert_eq!(r.probes(), 3);
    }

    #[test]
    fn a_claude_account_change_forces_one_probe() {
        let r = rig(FUTURE);
        r.write_claude("account-a", 1_000);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        assert_eq!(r.probes(), 1, "the first observation is the baseline");
        r.write_claude("account-a", 1_001);
        c.tick(at(1)).unwrap();
        assert_eq!(r.probes(), 1, "same account, new mtime");
        r.write_claude("account-b", 1_002);
        c.tick(at(2)).unwrap();
        assert_eq!(r.probes(), 2);
        c.tick(at(3)).unwrap();
        assert_eq!(r.probes(), 2);
    }

    #[test]
    fn a_codex_auth_change_forces_one_probe() {
        let r = rig(FUTURE);
        let auth = r.home.join(".codex/auth.json");
        std::fs::write(&auth, "first").unwrap();
        set_mtime(&auth, 1_000);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        assert_eq!(r.probes(), 1);
        c.tick(at(1)).unwrap();
        assert_eq!(r.probes(), 1);
        std::fs::write(&auth, "second").unwrap();
        set_mtime(&auth, 1_001);
        c.tick(at(2)).unwrap();
        assert_eq!(r.probes(), 2);
    }

    #[test]
    fn the_watch_honors_the_harness_home_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let (claude, codex) = (tmp.path().join("ccd"), tmp.path().join("cxh"));
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        let mut watch = AccountWatch::new(&tmp.path().join("home"), Some(&claude), Some(&codex));
        assert!(!watch.changed());
        std::fs::write(codex.join("auth.json"), "x").unwrap();
        assert!(watch.changed());
        let body = serde_json::json!({"oauthAccount": {"accountUuid": "a"}});
        std::fs::write(claude.join(".claude.json"), body.to_string()).unwrap();
        assert!(watch.changed());
        assert!(!watch.changed());
    }

    #[test]
    fn a_half_written_claude_config_keeps_the_baseline() {
        let r = rig(FUTURE);
        r.write_claude("account-a", 1_000);
        let mut watch = AccountWatch::new(&r.home, None, None);
        assert!(!watch.changed());
        let path = r.home.join(".claude.json");
        std::fs::write(&path, "{\"oauthAcc").unwrap();
        set_mtime(&path, 1_001);
        assert!(!watch.changed());
        r.write_claude("account-a", 1_002);
        assert!(!watch.changed());
        r.write_claude("account-b", 1_003);
        assert!(watch.changed());
    }

    fn every_file(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                every_file(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    #[test]
    fn no_account_id_reaches_the_quota_or_event_files() {
        let r = rig(FUTURE);
        r.write_claude(ACCOUNT, 1_000);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        r.write_claude("acct-other-private-id", 1_001);
        c.tick(at(1)).unwrap();
        assert_eq!(r.probes(), 2);
        let mut files = Vec::new();
        every_file(&r.state, &mut files);
        assert!(files.iter().any(|f| f.ends_with("quota.json")), "{files:?}");
        for file in files {
            let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
            assert!(!text.contains("acct-"), "{} holds an id", file.display());
        }
    }

    #[test]
    fn refresh_advances_probed_at_under_a_live_collector() {
        let r = rig(FUTURE);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        let before = QuotaFile::load(&r.state, None).unwrap().pools["claude"]
            .probed_at
            .clone();
        let policy = Policy::default();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let view = std::thread::scope(|s| {
            let ticks = s.spawn(|| {
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    c.tick(at(1)).unwrap();
                    std::thread::sleep(StdDuration::from_millis(10));
                }
            });
            let view = snapshot::refresh_through_collector(
                &r.state,
                at(1),
                &policy,
                StdDuration::from_secs(600),
            );
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            ticks.join().unwrap();
            view
        })
        .unwrap();
        assert_eq!(r.probes(), 2);
        assert!(view.file.pools["claude"].probed_at > before);
        assert!(pending(&r.state).is_empty(), "the collector consumed it");
    }

    #[test]
    fn refresh_fails_at_the_deadline_and_withdraws_its_request() {
        let r = rig(FUTURE);
        let mut c = r.collector(Probing::Scheduled);
        c.tick(at(0)).unwrap();
        let err = snapshot::refresh_through_collector(
            &r.state,
            at(1),
            &Policy::default(),
            StdDuration::from_millis(50),
        )
        .err()
        .expect("no collector tick answers");
        assert_eq!(
            err.to_string(),
            "quota refresh failed: the collector published no new reading within 0 s"
        );
        assert!(pending(&r.state).is_empty());
    }
}
