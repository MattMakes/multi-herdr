//! The Phase 6 keys of a fake agent: the fake idea-lead (fake-claude, and
//! fake-codex as its fallback) and the fake judge, so an e2e test can drive
//! each phase of an idea and each judge outcome.
//!
//! The idea folder of a sub-orchestrator is the first word of its launch
//! argument that is a folder with an `idea.md` (the launch argument names
//! the idea, the phase and the idea folder). The idea id is the name of
//! that folder. Each `horch` command runs with the `horch` on `PATH` and is
//! logged with its argv.
//!
//! Keys of the fake idea-lead, in the order they run:
//! - `die_mid_interview`: record 1 interview note, then exit 137 with no
//!   `assumptions.md`.
//! - `clarify`: write `assumptions.md` with a `feature:` line and record 2
//!   interview notes (`horch idea note`). Logged as `notes`.
//! - `clarify_open`: as `clarify`, and the file has 1 `[open]` item.
//! - `specify`: write `spec.md` and `requirements.yaml`; each requirement
//!   cites an assumption.
//! - `ticket`: write 1 C1 ticket `tickets/T1.md` and `plan.yaml`.
//! - `ticket_cycle`: write a `plan.yaml` of 2 tasks that wait for each other.
//! - `answer_covered`, `answer_own`: answer the oldest open question of the
//!   feature of the idea with `horch idea note --question`, case `covered`
//!   with a `--cite` of `assumptions.md`, or case `own_decision`. Logged as
//!   `answers`.
//! - `forge_approval`: write `approval.json` with the sha256 of
//!   `assumptions.md`, as if the operator approved it.
//! - `approve_inside`: run `horch idea approve <idea>`. Logged as `approve`.
//! - `operator_request`: send a raw `idea_approve` request with no token,
//!   on the channel and as a request file. Logged as `operator_request`.
//!
//! Keys of the fake judge: `verdict_pass`, `verdict_critical`,
//! `verdict_high` write `review.md` in the worktree of the attempt and
//! commit it: a `## Verdict` section with 1 word and a `## Findings` list
//! of ranked items. [`crate::agent::play`] runs them, only for a judge
//! task, before `result`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Request;
use horch_core::fleetd::fleet_runtime::adapters::scans::requests_dir;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::runtime::{Paths, ProcessEnv};
use horch_core::taskledger::Store;
use serde_json::{json, Map, Value};

use crate::agent::{git, horch, worktree};
use crate::{hash16, scenario_has, Call};

/// The idea-lead keys that need the idea folder.
const IDEA_KEYS: [&str; 11] = [
    "die_mid_interview",
    "clarify",
    "clarify_open",
    "specify",
    "ticket",
    "ticket_cycle",
    "answer_covered",
    "answer_own",
    "forge_approval",
    "approve_inside",
    "operator_request",
];

/// The words of the brief of fleetd before the feature id of the idea.
const FEATURE_LINE: &str = "The feature of your idea is ";

/// The keys of the fake judge.
pub(crate) const VERDICT_KEYS: [&str; 3] = ["verdict_pass", "verdict_critical", "verdict_high"];

/// The verdict file of the fake judge, in its worktree.
const VERDICT_FILE: &str = "review.md";

/// The file mode of each file that the fake writes.
const FILE_MODE: u32 = 0o600;

/// The longest wait for a connect to the channel.
const CONNECT_WAIT: Duration = Duration::from_secs(5);

/// Run the Phase 6 keys of `$HORCH_FAKE_SCENARIO` for a pane launch.
pub fn play(call: &mut Call) {
    if !IDEA_KEYS.iter().any(|k| scenario_has(k)) {
        return;
    }
    let Some(folder) = idea_folder(&call.argv) else {
        call.extra.insert(
            "idea_lead".into(),
            json!({"error": "the launch argument names no idea folder with an idea.md"}),
        );
        call.flush();
        return;
    };
    let lead = Lead::new(folder, feature_id(&call.argv));
    if scenario_has("die_mid_interview") {
        let note = lead.note("Which file does the idea change?", "fake.txt");
        call.extra.insert("notes".into(), json!([note]));
        call.flush();
        std::process::exit(137);
    }
    let open = scenario_has("clarify_open");
    if open || scenario_has("clarify") {
        lead.write("assumptions.md", &lead.assumptions(open));
        let notes = [
            lead.note("Which file does the idea change?", "fake.txt"),
            lead.note("How many tasks does it need?", "1 task"),
        ];
        call.extra.insert("notes".into(), json!(notes));
    }
    if scenario_has("specify") {
        lead.write("spec.md", &lead.spec());
        lead.write("requirements.yaml", &lead.requirements());
    }
    let cycle = scenario_has("ticket_cycle");
    if cycle || scenario_has("ticket") {
        lead.write("tickets/T1.md", TICKET);
        lead.write("plan.yaml", &lead.plan(cycle));
    }
    let mut answers = Vec::new();
    if scenario_has("answer_covered") {
        answers.push(lead.answer("covered"));
    }
    if scenario_has("answer_own") {
        answers.push(lead.answer("own_decision"));
    }
    if !answers.is_empty() {
        call.extra.insert("answers".into(), json!(answers));
    }
    if scenario_has("forge_approval") {
        lead.write("approval.json", &lead.approval());
    }
    if scenario_has("approve_inside") {
        let out = run_horch(&lead.folder, &["idea", "approve", &lead.idea]);
        call.extra.insert("approve".into(), out);
    }
    if scenario_has("operator_request") {
        let out = lead.operator_request(&call.call_id);
        call.extra.insert("operator_request".into(), out);
    }
    call.flush();
}

/// The first word of `argv` that is a folder with an `idea.md`.
fn idea_folder(argv: &[String]) -> Option<PathBuf> {
    argv.iter()
        .flat_map(|a| a.split_whitespace())
        .map(PathBuf::from)
        .find(|p| p.join("idea.md").is_file())
}

/// `horch <args>` in `dir`, logged with its argv.
fn run_horch(dir: &Path, args: &[&str]) -> Value {
    let mut out = horch(dir, args);
    out["argv"] = json!(args);
    out
}

/// The fake idea-lead of 1 idea folder.
struct Lead {
    folder: PathBuf,
    idea: String,
    /// The feature id of the launch argument, if it names one.
    feature_id: Option<String>,
}

impl Lead {
    fn new(folder: PathBuf, feature_id: Option<String>) -> Lead {
        let idea = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Lead {
            folder,
            idea,
            feature_id,
        }
    }

    /// The feature name of the idea: kebab-case.
    fn feature(&self) -> String {
        format!("fake-{}", self.idea.to_lowercase())
    }

    /// Write `name` in the idea folder atomically. A failure goes to stderr:
    /// the test reads the folder.
    fn write(&self, name: &str, text: &str) {
        let path = self.folder.join(name);
        if let Err(e) = write_file(&path, text) {
            eprintln!("fake idea-lead: {e}");
        }
    }

    /// Record 1 interview note: `<question> - <answer>`.
    fn note(&self, question: &str, answer: &str) -> Value {
        let text = format!("{question} - {answer}");
        run_horch(&self.folder, &["idea", "note", &self.idea, &text])
    }

    fn assumptions(&self, open: bool) -> String {
        let mut text = format!(
            "# {} assumptions\nfeature: {}\n\n\
             1. [confirmed] The idea changes the file fake.txt.\n\
             2. [assumed] The idea needs 1 task.\n",
            self.idea,
            self.feature()
        );
        if open {
            text.push_str("3. [open] Which line of fake.txt changes?\n");
        }
        text
    }

    fn spec(&self) -> String {
        format!(
            "# {} spec\n\n## Behaviour\n\nThe task writes 1 line in fake.txt.\n",
            self.idea
        )
    }

    fn requirements(&self) -> String {
        format!(
            "idea: {}\nrequirements:\n\
             \x20 - id: R1\n    text: The task writes fake.txt.\n    cites: [A1]\n\
             \x20 - id: R2\n    text: The plan has 1 task.\n    cites: [A2]\n",
            self.idea
        )
    }

    /// plan.yaml: 1 task, or 2 tasks that wait for each other. Its feature
    /// is the feature id of the launch argument, else the feature name when
    /// the launch argument names none.
    fn plan(&self, cycle: bool) -> String {
        let mut text = format!(
            "idea: {}\nrepo: .\nfeature: {}\nbase_branch: main\ntasks:\n",
            self.idea,
            self.feature_id.clone().unwrap_or_else(|| self.feature())
        );
        let task = |key: &str, after: &str| {
            format!(
                "  - key: {key}\n    title: Write fake.txt\n    ticket: tickets/T1.md\n\
                 \x20   kind: feature\n    profile: opus\n    covers: [R1, R2]\n\
                 \x20   outputs: [fake.txt]\n    acceptance:\n      - run: \"true\"\n{after}"
            )
        };
        if cycle {
            text.push_str(&task("T1", "    after: [T2]\n"));
            text.push_str(&task("T2", "    after: [T1]\n"));
        } else {
            text.push_str(&task("T1", ""));
        }
        text
    }

    /// approval.json with the hash of assumptions.md: a forgery.
    fn approval(&self) -> String {
        let bytes = std::fs::read(self.folder.join("assumptions.md")).unwrap_or_default();
        json!({
            "idea": self.idea,
            "file": "assumptions.md",
            "sha256": sha256_bytes(&bytes).hex(),
            "approved_at": "2026-01-01T00:00:00Z",
            "by": "operator",
        })
        .to_string()
    }

    /// Answer the oldest open question of the feature of the idea.
    fn answer(&self, case: &str) -> Value {
        let Some(question) = open_question(&self.idea) else {
            return json!({"error": format!("no open question for the feature of {}", self.idea)});
        };
        let mut args = vec![
            "idea".to_string(),
            "note".into(),
            self.idea.clone(),
            "--question".into(),
            question,
            "--case".into(),
            case.into(),
        ];
        if case == "covered" {
            args.extend([
                "--cite".into(),
                format!("assumptions.md:{}", self.first_item_line()),
            ]);
        }
        args.push(format!("fake answer, case {case}"));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_horch(&self.folder, &args)
    }

    /// The line number of item 1 of assumptions.md, from 1.
    fn first_item_line(&self) -> usize {
        std::fs::read_to_string(self.folder.join("assumptions.md"))
            .ok()
            .and_then(|t| t.lines().position(|l| l.starts_with("1. ")))
            .map_or(1, |i| i + 1)
    }

    /// Send `idea_approve` of the idea with no token: on the channel, then
    /// as a request file.
    fn operator_request(&self, call_id: &str) -> Value {
        let bytes = std::fs::read(self.folder.join("assumptions.md")).unwrap_or_default();
        let mut payload = Map::new();
        payload.insert("idea".into(), json!(self.idea));
        payload.insert("sha256".into(), json!(sha256_bytes(&bytes).hex()));
        let request = |suffix: &str| Request {
            request_id: RequestId::parse(&hash16(&format!("{call_id}-{suffix}")))
                .expect("16 hex digits are a request id"),
            kind: Kind::IdeaApprove,
            payload: payload.clone(),
            token: None,
        };
        let fleet_root = Paths::from_env(&ProcessEnv).fleet_root;
        json!({
            "channel": send_on_channel(&fleet_root, &request("channel")),
            "file": write_request_file(&fleet_root, &request("file")),
        })
    }
}

/// The feature id that the launch argument names in its line `The
/// feature of your idea is F-NNNN.`, as the brief of fleetd gives it.
fn feature_id(argv: &[String]) -> Option<String> {
    argv.iter().find_map(|a| {
        let rest = a.split(FEATURE_LINE).nth(1)?;
        let id = rest.split_whitespace().next()?.trim_end_matches('.');
        (!id.is_empty()).then(|| id.to_string())
    })
}

/// The id of the oldest open question of the feature of `idea`.
fn open_question(idea: &str) -> Option<String> {
    let store = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db).ok()?;
    store
        .conn()
        .query_row(
            "SELECT q.id FROM questions q JOIN ideas i ON i.feature_id = q.feature_id
             WHERE i.id = ?1 AND q.state = 'open'
             ORDER BY q.asked_at, q.id LIMIT 1",
            [idea],
            |r| r.get(0),
        )
        .ok()
}

/// Send `request` on the channel of the fleet; the reply line, or the error.
fn send_on_channel(fleet_root: &Path, request: &Request) -> Value {
    let channel = fleet_root.join("run").join("fleetd.sock");
    let stream =
        match horch_platform::channel::Stream::connect(&channel, Instant::now() + CONNECT_WAIT) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("connect {}: {e}", channel.display())}),
        };
    let mut writer = &stream;
    if let Err(e) = writer
        .write_all(request.to_line().as_bytes())
        .and_then(|()| writer.flush())
    {
        return json!({"error": format!("send: {e}")});
    }
    let mut reply = String::new();
    match BufReader::new(&stream).read_line(&mut reply) {
        Ok(_) => json!({"reply": reply.trim_end()}),
        Err(e) => json!({"error": format!("reply: {e}")}),
    }
}

/// Write `request` as a request file of the fleet; its path, or the error.
fn write_request_file(fleet_root: &Path, request: &Request) -> Value {
    let dir = requests_dir(fleet_root);
    let path = dir.join(format!("{}.json", request.request_id.as_str()));
    match write_file(&path, &request.to_line()) {
        Ok(()) => json!({"path": path.to_string_lossy()}),
        Err(e) => json!({"error": e}),
    }
}

/// Make the folder of `path` and write `text` in `path` atomically.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
    }
    horch_core::fsx::write_atomic(path, text.as_bytes(), FILE_MODE)
        .map_err(|e| format!("writing {}: {e:#}", path.display()))
}

/// The fake judge: write the verdict of its key in `review.md` and commit
/// it.
pub(crate) fn judge(call: &mut Call) {
    let verdict = if scenario_has("verdict_critical") {
        (
            "critical_open",
            "- [CRITICAL] fake: the change loses a write.\n",
        )
    } else if scenario_has("verdict_high") {
        ("pass", "- [HIGH] fake: an error is not reported.\n")
    } else if scenario_has("verdict_pass") {
        ("pass", "- [LOW] fake: a name could be clearer.\n")
    } else {
        return;
    };
    let text = format!(
        "# Review\n\n## Verdict\n\n{}\n\n## Findings\n\n{}",
        verdict.0, verdict.1
    );
    let dir = worktree();
    let out = std::fs::write(dir.join(VERDICT_FILE), text)
        .map_err(|e| format!("writing {VERDICT_FILE}: {e}"))
        .and_then(|()| {
            let message = format!("fake {}: verdict {}", call.fake, verdict.0);
            git(&dir, &["add", "--", VERDICT_FILE])?;
            git(&dir, &["commit", "-q", "-m", &message, "--", VERDICT_FILE])?;
            git(&dir, &["rev-parse", "HEAD"])
        });
    let logged = match out {
        Ok(head) => json!({"verdict": verdict.0, "head": head}),
        Err(e) => json!({"error": e}),
    };
    call.extra.insert("verdict".into(), logged);
}

/// A C1 ticket of 1 task.
const TICKET: &str = "# T1: Write fake.txt\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** light\n\
**Check:** `true`\n\
**Own:** `fake.txt`\n\n\
## Goal\n\nThe task writes 1 line in fake.txt.\n";
