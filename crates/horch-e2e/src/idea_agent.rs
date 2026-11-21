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
use crate::{hang, hash16, scenario_has, Call};

const IDEA_KEYS: [&str; 12] = [
    "die_mid_interview",
    "clarify",
    "clarify_open",
    "specify",
    "ticket",
    "ticket_cycle",
    "answer_covered",
    "answer_own",
    "await_approval",
    "forge_approval",
    "approve_inside",
    "operator_request",
];

const FEATURE_LINE: &str = "The feature of your idea is ";

pub(crate) const VERDICT_KEYS: [&str; 3] = ["verdict_pass", "verdict_critical", "verdict_high"];

pub(crate) const VERDICT_FILE: &str = "review.md";

const FILE_MODE: u32 = 0o600;

const AWAIT_APPROVAL: Duration = Duration::from_secs(60);

const CONNECT_WAIT: Duration = Duration::from_secs(5);

pub fn play(call: &mut Call) {
    lead_keys(call);
    if call.fake == "claude" && scenario_has("stay") {
        call.flush();
        hang();
    }
}

fn lead_keys(call: &mut Call) {
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
        let notes: Vec<Value> = [
            ("Which file does the idea change?", "fake.txt"),
            ("How many tasks does it need?", "1 task"),
        ]
        .into_iter()
        .filter(|(q, a)| !lead.answered(q, a))
        .map(|(q, a)| lead.note(q, a))
        .collect();
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
    if scenario_has("await_approval") {
        call.extra
            .insert("awaited".into(), json!(await_approval(&lead.idea)));
        call.flush();
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

pub(crate) fn idea_folder(argv: &[String]) -> Option<PathBuf> {
    argv.iter()
        .flat_map(|a| a.split_whitespace())
        .map(PathBuf::from)
        .find(|p| p.join("idea.md").is_file())
}

fn run_horch(dir: &Path, args: &[&str]) -> Value {
    let mut out = horch(dir, args);
    out["argv"] = json!(args);
    out
}

struct Lead {
    folder: PathBuf,
    idea: String,
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

    fn feature(&self) -> String {
        format!("fake-{}", self.idea.to_lowercase())
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.folder.join(name);
        if let Err(e) = write_file(&path, text) {
            eprintln!("fake idea-lead: {e}");
        }
    }

    fn answered(&self, question: &str, answer: &str) -> bool {
        std::fs::read_to_string(self.folder.join("interview.md"))
            .is_ok_and(|t| t.contains(&format!("\n{question}\n\nAnswer: {answer}\n")))
    }

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

    fn first_item_line(&self) -> usize {
        std::fs::read_to_string(self.folder.join("assumptions.md"))
            .ok()
            .and_then(|t| t.lines().position(|l| l.starts_with("1. ")))
            .map_or(1, |i| i + 1)
    }

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

fn await_approval(idea: &str) -> Option<String> {
    let end = Instant::now() + AWAIT_APPROVAL;
    loop {
        let state = idea_state(idea);
        if state.as_deref() == Some("awaiting_approval") || Instant::now() >= end {
            return state;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn idea_state(idea: &str) -> Option<String> {
    let store = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db).ok()?;
    store
        .conn()
        .query_row("SELECT state FROM ideas WHERE id = ?1", [idea], |r| {
            r.get(0)
        })
        .ok()
}

fn feature_id(argv: &[String]) -> Option<String> {
    argv.iter().find_map(|a| {
        let rest = a.split(FEATURE_LINE).nth(1)?;
        let id = rest.split_whitespace().next()?.trim_end_matches('.');
        (!id.is_empty()).then(|| id.to_string())
    })
}

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

pub(crate) fn send_on_channel(fleet_root: &Path, request: &Request) -> Value {
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

fn write_request_file(fleet_root: &Path, request: &Request) -> Value {
    let dir = requests_dir(fleet_root);
    let path = dir.join(format!("{}.json", request.request_id.as_str()));
    match write_file(&path, &request.to_line()) {
        Ok(()) => json!({"path": path.to_string_lossy()}),
        Err(e) => json!({"error": e}),
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
    }
    horch_core::fsx::write_atomic(path, text.as_bytes(), FILE_MODE)
        .map_err(|e| format!("writing {}: {e:#}", path.display()))
}

pub(crate) fn judge(call: &mut Call, file: &str) {
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
    let out = write_file(&dir.join(file), &text).and_then(|()| {
        let message = format!("fake {}: verdict {}", call.fake, verdict.0);
        git(&dir, &["add", "--", file])?;
        git(&dir, &["commit", "-q", "-m", &message, "--", file])?;
        git(&dir, &["rev-parse", "HEAD"])
    });
    let logged = match out {
        Ok(head) => json!({"verdict": verdict.0, "head": head}),
        Err(e) => json!({"error": e}),
    };
    call.extra.insert("verdict".into(), logged);
}

const TICKET: &str = "# T1: Write fake.txt\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** light\n\
**Check:** `true`\n\
**Own:** `fake.txt`\n\n\
## Goal\n\nThe task writes 1 line in fake.txt.\n";
