use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, PassBudget};
use horch_core::fleetd::ideas::adapters::needs_you_folder::{self, Mirrored};
use horch_core::fleetd::ideas::adapters::notify::{self, Notice};
use horch_core::fleetd::ideas::application::needs_you;
use horch_core::fleetd::ideas::domain::needs_you::Write;
use horch_core::taskledger::store::Store;
use horch_core::workspace::PaneHost;

#[cfg(unix)]
#[path = "common/warm.rs"]
mod warm;

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    folder: std::path::PathBuf,
    cursor: Cursor,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let folder = dir.path().join("fleet").join("needs-you");
    World {
        _dir: dir,
        store,
        folder,
        cursor: Cursor::new("t0"),
    }
}

impl World {
    fn open(&mut self, key: &str, text: &str) {
        let (kind, id) = key.split_once(':').unwrap();
        self.store
            .immediate(|tx| {
                let w = Write {
                    key,
                    kind,
                    entity: "task",
                    entity_id: id,
                    text,
                };
                needs_you::open_item(tx, &w)
            })
            .unwrap();
    }

    fn resolve(&mut self, key: &str) {
        let done = self
            .store
            .immediate(|tx| needs_you::resolve(tx, key))
            .unwrap();
        assert!(done, "{key} was not open");
    }

    fn pass(&mut self, now: &str) -> Mirrored {
        let items = needs_you::open_items(self.store.conn()).unwrap();
        needs_you_folder::mirror(
            &self.folder,
            &items,
            &mut self.cursor,
            PassBudget::new(200),
            now,
        )
        .unwrap()
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

#[test]
fn open_item_has_1_file() {
    let mut w = world();
    w.open("question_timeout:Q-7", "Q-7 has no answer after 24 h.");
    let mirrored = w.pass("t1");
    assert_eq!(w.files(), ["question_timeout-Q-7.md"]);
    assert_eq!((mirrored.new, mirrored.open), (1, 1));
    let body = std::fs::read_to_string(w.folder.join("question_timeout-Q-7.md")).unwrap();
    assert!(body.contains("Q-7 has no answer after 24 h."), "{body}");
    assert!(body.contains("question_timeout:Q-7"), "{body}");
}

#[test]
fn resolved_item_file_is_removed() {
    let mut w = world();
    w.open("question_timeout:Q-7", "Q-7 has no answer.");
    w.open("push_failed:F-0002", "The push failed 3 times.");
    w.pass("t1");
    assert_eq!(w.files().len(), 2);

    w.resolve("question_timeout:Q-7");
    let mirrored = w.pass("t2");
    assert_eq!(w.files(), ["push_failed-F-0002.md"]);
    assert_eq!((mirrored.removed, mirrored.open), (1, 1));
}

#[test]
fn same_escalation_on_2_passes_gives_1_file() {
    let mut w = world();
    w.open("question_timeout:Q-7", "Q-7 has no answer after 24 h.");
    let first = w.pass("t1");
    w.open("question_timeout:Q-7", "Q-7 has no answer after 25 h.");
    let second = w.pass("t2");
    assert_eq!(w.files(), ["question_timeout-Q-7.md"]);
    assert_eq!(first.new, 1);
    assert_eq!(second.new, 0);
    let body = std::fs::read_to_string(w.folder.join("question_timeout-Q-7.md")).unwrap();
    assert!(body.contains("after 25 h."), "{body}");
}

#[test]
fn rows_past_the_budget_wait_for_the_next_pass() {
    let mut w = world();
    for i in 0..3 {
        w.open(&format!("question_timeout:Q-{i}"), "no answer");
    }
    let items = needs_you::open_items(w.store.conn()).unwrap();
    let budget = PassBudget::new(2);
    needs_you_folder::mirror(&w.folder, &items, &mut w.cursor, budget, "t1").unwrap();
    assert_eq!(w.files().len(), 2);
    needs_you_folder::mirror(&w.folder, &items, &mut w.cursor, budget, "t2").unwrap();
    assert_eq!(w.files().len(), 3);
}

#[cfg(unix)]
fn scripted_herdr(dir: &std::path::Path) -> (PaneHost, std::path::PathBuf) {
    let log = dir.join("calls.log");
    let script = dir.join("herdr");
    let body = format!(
        "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\necho \"$*\" >> '{}'\n",
        log.display()
    );
    warm::write_warmed(&script, &body, &["warm"]);
    (PaneHost::herdr_bin(&script), log)
}

#[cfg(unix)]
#[test]
fn new_open_item_reports_1_notification() {
    let mut w = world();
    let herdr_dir = tempfile::tempdir().unwrap();
    let (herdr, log) = scripted_herdr(herdr_dir.path());

    w.open("question_timeout:Q-7", "Q-7 has no answer.");
    let first = w.pass("t1");
    assert_eq!(notify::notify(&herdr, &first, 1), Notice::Sent);
    w.open("question_timeout:Q-7", "Q-7 has no answer, again.");
    let second = w.pass("t2");
    assert_eq!(notify::notify(&herdr, &second, 2), Notice::Quiet);

    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 1, "{calls}");
    let call = lines[0];
    assert!(
        call.starts_with(&format!("pane report-agent {}", notify::PANE)),
        "{call}"
    );
    assert!(call.contains("--state blocked"), "{call}");
    assert!(call.contains("(1)"), "the count of open items: {call}");
}

#[test]
fn no_herdr_is_not_an_error() {
    let mut w = world();
    let dir = tempfile::tempdir().unwrap();
    let herdr = PaneHost::herdr_bin(dir.path().join("no-such-herdr"));
    w.open("question_timeout:Q-7", "Q-7 has no answer.");
    let mirrored = w.pass("t1");
    match notify::notify(&herdr, &mirrored, 1) {
        Notice::Failed(line) => {
            assert_eq!(line.lines().count(), 1, "{line}");
            assert!(line.contains("herdr"), "{line}");
        }
        other => panic!("expected a log line, got {other:?}"),
    }
}
