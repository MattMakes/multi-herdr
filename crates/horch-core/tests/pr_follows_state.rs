//! The pull request follows the state (docs/specs/fleetd/design.md, "The
//! pull request follows the state", "Between features": The pull request;
//! preservation-04-behavior.md row F17; docs/tickets/fleetd-p4/39).
//!
//! Each test makes its own temporary ledger, a local bare repository as the
//! remote `origin`, a clone of it as the repository of the project, and a
//! fake `gh` on the PATH of the run. The fake `gh` keeps 1 pull request in
//! the folder `gh/` beside its own folder, and logs each call there. The
//! push job runs with the fleet clock of the ledger, set by the test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_fetch::Tools;
use horch_core::fleetd::integration::adapters::exec_push;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::remote;
use horch_core::fleetd::integration::domain::project::Remote;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::runtime::MapEnv;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection, TransactionBehavior};

const FEATURE: &str = "F-0001";
const BRANCH: &str = "fleet/run/x";
/// The web address of the remote, as `remote.origin.url` names it. A push
/// goes to the local bare repository through `url.<bare>.pushInsteadOf`.
const WEB: &str = "https://github.com/acme/x";
/// The address that the fake `gh` gives to a new pull request.
const NEW_PR: &str = "https://github.com/acme/x/pull/7";

/// The fake `gh`: `pr view`, `pr create`, `pr edit` and `pr ready` of 1
/// pull request, kept as files in `<bin>/../gh/`.
const FAKE_GH: &str = r#"#!/bin/sh
d="$(cd "$(dirname "$0")/../gh" && pwd)"
echo "$*" >> "$d/log"
json() { perl -0777 -pe 's/\\/\\\\/g; s/"/\\"/g; s/\n/\\n/g' "$1"; }
case "$1 $2" in
"pr view")
  [ -f "$d/url" ] || { echo "no pull requests found for branch $3" >&2; exit 1; }
  printf '{"url":"%s","state":"%s","isDraft":%s,"body":"%s"}\n' \
    "$(cat "$d/url")" "$(cat "$d/state")" "$(cat "$d/draft")" "$(json "$d/body")"
  ;;
"pr create")
  shift 2
  while [ $# -gt 0 ]; do
    case "$1" in
      --body-file) cp "$2" "$d/body"; shift ;;
      --base) echo "$2" > "$d/base"; shift ;;
      --head) echo "$2" > "$d/head"; shift ;;
      --title) shift ;;
    esac
    shift
  done
  echo "https://github.com/acme/x/pull/7" > "$d/url"
  echo OPEN > "$d/state"
  echo false > "$d/draft"
  echo "Creating pull request for fleet/run/x into main" >&2
  cat "$d/url"
  ;;
"pr edit")
  cp "$5" "$d/body"
  ;;
"pr ready")
  if [ "$4" = "--undo" ]; then echo true > "$d/draft"; else echo false > "$d/draft"; fi
  ;;
*)
  echo "fake gh: unknown call: $*" >&2
  exit 2
  ;;
esac
"#;

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    repo: PathBuf,
    run: runs::Run,
    tools: Tools,
}

/// A temp ledger with the fleet clock at 0, a run with the feature, its
/// branch row and `tasks` landed task branches, a remote with `main`, a
/// local feature branch 1 commit past `main`, and the fake `gh`. `pr` is
/// the `pr` key of the project file on the feature branch, if any.
fn world(pr: Option<&str>, tasks: &[&str]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().canonicalize().unwrap();
    let mut conn = Connection::open(top.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, 1, 1, 'now', 0, 'now')",
        [],
    )
    .unwrap();

    let bare = top.join("remote.git");
    git(&top, &["init", "-q", "--bare", "-b", "main", path(&bare)]);
    let repo = top.join("repo");
    git(&top, &["init", "-q", "-b", "main", path(&repo)]);
    commit(&repo, "a.txt", "a\n");
    git(&repo, &["remote", "add", "origin", WEB]);
    let rewrite = format!("url.{}.pushInsteadOf", path(&bare));
    git(&repo, &["config", &rewrite, WEB]);
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["checkout", "-q", "-b", BRANCH]);
    match pr {
        Some(word) => commit(&repo, "fleet.toml", &format!("pr = \"{word}\"\n")),
        None => commit(&repo, "f.txt", "f\n"),
    }

    let bin = top.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(top.join("gh")).unwrap();
    std::fs::create_dir_all(top.join("tmp")).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, FAKE_GH).unwrap();
    make_runnable(&gh);
    let env = MapEnv {
        vars: BTreeMap::from([("PATH".to_string(), path(&bin).to_string())]),
        temp: Some(top.join("tmp")),
        ..MapEnv::default()
    };
    let tools = Tools::from_env(&env);
    assert_eq!(tools.gh.as_deref(), Some(gh.as_path()), "gh is on PATH");

    let new_run = NewRun {
        slug: "x".into(),
        base_branch: Some("main".into()),
        run_branch: Some(BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &repo, &new_run).unwrap();
    let mut w = World {
        dir,
        conn,
        config: FleetConfig {
            push_failure_limit: 3,
            job_backoff_s: vec![30, 120, 480],
            ..FleetConfig::default()
        },
        repo,
        run,
        tools,
    };
    w.tx(|tx| {
        let new = NewFeature {
            id: FEATURE,
            name: Some("x"),
            branch: Some(BRANCH),
            primary_branch: "main",
        };
        rows::insert_feature(tx, None, &new, cause("feature accepted"))?;
        // FR-12: a task of the run needs an idea in flight of the feature.
        let idea = NewIdea {
            id: "I-1",
            feature_id: FEATURE,
            priority: 0,
        };
        ideas_store::insert_idea(tx, None, &idea, cause("idea added"))?;
        let branch = NewBranch {
            name: BRANCH,
            feature_id: FEATURE,
            task_id: None,
            owner: "fleetd",
            due_at: 0,
        };
        rows::insert_branch(tx, None, &branch, cause("feature branch"))
    });
    for title in tasks {
        w.landed_task(title);
    }
    w.see_local_tip();
    w
}

impl World {
    fn gh_dir(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap().join("gh")
    }

    fn gh_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.gh_dir().join(name))
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    /// The calls of the fake `gh`, 1 for each line.
    fn gh_log(&self) -> Vec<String> {
        self.gh_file("log").lines().map(str::to_string).collect()
    }

    /// A pull request that a claim attempt opened before: open, with
    /// `body`.
    fn existing_pr(&self, body: &str) {
        let d = self.gh_dir();
        std::fs::write(d.join("url"), format!("{NEW_PR}\n")).unwrap();
        std::fs::write(d.join("state"), "OPEN\n").unwrap();
        std::fs::write(d.join("draft"), "false\n").unwrap();
        std::fs::write(d.join("body"), body).unwrap();
    }

    fn tx(&mut self, f: impl FnOnce(&Connection) -> anyhow::Result<()>) {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        f(&tx).unwrap();
        tx.commit().unwrap();
    }

    /// A task `title` of the run whose task branch landed on the feature.
    fn landed_task(&mut self, title: &str) {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let name = format!("{BRANCH}-{title}");
        self.tx(|tx| {
            let branch = NewBranch {
                name: &name,
                feature_id: FEATURE,
                task_id: Some(&task),
                owner: "fleetd",
                due_at: 0,
            };
            rows::insert_branch(tx, None, &branch, cause("task branch"))
        });
        self.conn
            .execute(
                "UPDATE branches SET landing_state = 'landed' WHERE name = ?1",
                params![name],
            )
            .unwrap();
    }

    fn set_state(&self, state: &str) {
        self.conn
            .execute(
                "UPDATE features SET state = ?1 WHERE id = ?2",
                params![state, FEATURE],
            )
            .unwrap();
    }

    /// A new commit on the feature branch, seen by the pass.
    fn new_commit(&mut self, file: &str) {
        commit(&self.repo, file, "x\n");
        self.see_local_tip();
    }

    fn see_local_tip(&mut self) {
        let tip = git(&self.repo, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
        self.tx(|tx| {
            rows::set_branch_fact(
                tx,
                None,
                BRANCH,
                BranchFact::SeenTip(&tip),
                cause("the pass saw the tip"),
            )
        });
    }

    /// The push rule at `clock`.
    fn queue(&mut self, clock: i64) -> Option<Queued> {
        let config = self.config.clone();
        let repo = path(&self.repo).to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let queued = remote::queue_push(
            &ctx(&config),
            &tx,
            &repo,
            FEATURE,
            &Remote::default(),
            false,
            FleetSeconds(clock),
        )
        .unwrap();
        tx.commit().unwrap();
        queued
    }

    /// Queue the push at `clock` and run its executor, as `horch job run`
    /// runs it. Give the run row.
    fn push(&mut self, clock: i64) -> JobRow {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
            .unwrap();
        let id = match self.queue(clock) {
            Some(Queued::New(id)) => id,
            other => panic!("no new push run at {clock} s: {other:?}"),
        };
        let row = jobs_store::get(&self.conn, id).unwrap().unwrap();
        let job = Store::open_read_only(Path::new(self.conn.path().unwrap())).unwrap();
        let ran = exec_push::run_with(job.conn(), &row, &self.tools, &Launcher).unwrap();
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        exec_push::record(&ctx(&config), &tx, &row, &ran).unwrap();
        tx.commit().unwrap();
        self.conn
            .execute(
                "UPDATE jobs SET state = 'passed', ended_at = 'now' WHERE id = ?1",
                [id],
            )
            .unwrap();
        row
    }

    fn feature(&self) -> rows::FeatureRow {
        rows::feature(&self.conn, FEATURE).unwrap().unwrap()
    }

    /// The text of each open needs-you item.
    fn open_items(&self) -> Vec<String> {
        needs_you::open_items(&self.conn)
            .unwrap()
            .into_iter()
            .map(|i| i.text)
            .collect()
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[cfg(unix)]
fn make_runnable(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_runnable(_: &Path) {}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, body: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

// ─── the tests ──────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn ready_feature_opens_its_pull_request() {
    let mut w = world(None, &["one"]);
    // An open feature with no pull request: no push, no pull request.
    assert!(w.queue(0).is_none());
    assert!(w.gh_log().is_empty(), "{:?}", w.gh_log());

    w.set_state("ready");
    w.push(10);
    let calls = w.gh_log();
    assert!(
        calls.iter().any(|c| c.starts_with("pr create")),
        "{calls:?}"
    );
    assert_eq!(w.gh_file("base"), "main");
    assert_eq!(w.gh_file("head"), BRANCH);
    let f = w.feature();
    assert_eq!(f.pr.as_deref(), Some(NEW_PR));
    assert_eq!(f.pr_state, "open");
    assert_eq!(w.gh_file("draft"), "false");

    // The pull request exists: a second push opens no second one.
    w.new_commit("g.txt");
    w.push(20);
    let creates = w
        .gh_log()
        .iter()
        .filter(|c| c.starts_with("pr create"))
        .count();
    assert_eq!(creates, 1, "{:?}", w.gh_log());
}

#[cfg(unix)]
#[test]
fn ready_feature_whose_tip_is_pushed_still_opens_its_pull_request() {
    let mut w = world(None, &["one"]);
    // A rebase pushed the tip while the feature was open.
    w.conn
        .execute(
            "UPDATE branches SET seen_remote_tip = seen_tip WHERE name = ?1",
            params![BRANCH],
        )
        .unwrap();
    let tip = git(&w.repo, &["rev-parse", BRANCH]);
    git(
        &w.repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{tip}:refs/heads/{BRANCH}"),
        ],
    );
    w.set_state("ready");
    w.push(10);
    assert_eq!(w.feature().pr.as_deref(), Some(NEW_PR));
}

#[cfg(unix)]
#[test]
fn feature_open_again_marks_it_draft() {
    let mut w = world(None, &["one"]);
    w.set_state("ready");
    w.push(10);
    assert_eq!(w.gh_file("draft"), "false");

    // A new idea: the feature is open again, and its tip moves.
    w.set_state("open");
    w.new_commit("g.txt");
    w.push(20);
    assert_eq!(w.gh_file("draft"), "true", "{:?}", w.gh_log());
    assert!(
        w.gh_log()
            .iter()
            .any(|c| c == &format!("pr ready {NEW_PR} --undo")),
        "{:?}",
        w.gh_log()
    );

    // Ready again: the draft becomes ready for review.
    w.set_state("ready");
    w.new_commit("h.txt");
    w.push(30);
    assert_eq!(w.gh_file("draft"), "false", "{:?}", w.gh_log());
}

#[cfg(unix)]
#[test]
fn manual_pr_puts_the_compare_url_on_the_needs_you_list() {
    let mut w = world(Some("manual"), &["one"]);
    w.set_state("ready");
    w.push(10);
    let url = format!("{WEB}/compare/main...{BRANCH}");
    let items = w.open_items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].contains(&url), "{items:?}");
    assert!(!items[0].contains("do not merge"), "{items:?}");
    assert_eq!(w.feature().pr.as_deref(), Some(url.as_str()));
    assert!(w.gh_log().is_empty(), "manual runs no gh: {:?}", w.gh_log());

    w.set_state("open");
    w.new_commit("g.txt");
    w.push(20);
    let items = w.open_items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].contains("do not merge"), "{items:?}");
    assert!(items[0].contains(&url), "{items:?}");
    assert!(w.gh_log().is_empty(), "manual runs no gh: {:?}", w.gh_log());
}

#[cfg(unix)]
#[test]
fn task_sections_keep_their_markers() {
    let mut w = world(None, &["one", "two"]);
    let t1 = tasks::of_run(&w.conn, &w.run.run_id).unwrap()[0].short();
    let t2 = tasks::of_run(&w.conn, &w.run.run_id).unwrap()[1].short();
    // A claim attempt opened the pull request with the section of T-1.
    let old = format!(
        "Run `x`.\n\n<!-- horch:task {t1} -->\nclaim text of {t1}\n<!-- /horch:task {t1} -->\n\nOperator note."
    );
    w.existing_pr(&old);

    w.set_state("ready");
    w.push(10);
    assert!(
        !w.gh_log().iter().any(|c| c.starts_with("pr create")),
        "the open pull request is used: {:?}",
        w.gh_log()
    );
    assert_eq!(w.feature().pr.as_deref(), Some(NEW_PR));
    let body = w.gh_file("body");
    assert!(
        body.starts_with(&old),
        "the section of {t1} and the text outside it stay: {body}"
    );
    let open = format!("<!-- horch:task {t2} -->\n");
    let close = format!("<!-- /horch:task {t2} -->");
    let (a, b) = (body.find(&open).unwrap(), body.find(&close).unwrap());
    assert!(a < b, "{body}");
    assert!(body[a..b].contains("two"), "{body}");
    assert_eq!(
        body.matches(&format!("<!-- horch:task {t1} -->")).count(),
        1
    );

    // A second push changes no section.
    w.new_commit("g.txt");
    w.push(20);
    assert_eq!(w.gh_file("body"), body);
}
