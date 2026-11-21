//! The sweep of stale horch state (#140).
//!
//! horch writes state per workspace and per launch, and the process that
//! owns it usually dies by signal, so no `Drop` removes it. This one pass
//! finds the state whose owner is provably gone and removes it:
//!
//! | kind        | where                                  | stale when                                            |
//! |-------------|----------------------------------------|-------------------------------------------------------|
//! | record      | session records (task ledger)          | `working`, and herdr answers without its workspace or pane |
//! | spool       | `<state>/messages/<ws>/`               | older than 7 days, and no pending mail names it       |
//! | socket      | `<state>/channels/*.sock`              | no relay answers a ping                               |
//! | registry    | `herdr-orchestration-<ws>` (state, temp) | herdr answers without `<ws>`, and `<ws>` has no pending mail |
//! | mail        | `<state>/mail/<ws>/`                   | as for the registry                                   |
//! | bundle      | `<state>/skill-bundles/<record id>/`   | its record is terminal                                |
//! | codex home  | `<state>/codex-home/<role>-<uuid>/`    | every record of the role is terminal, and codex left no stranded file |
//! | temp file   | `write_atomic` temp files              | its pid is dead, and it is older than 1 hour          |
//!
//! A herdr that does not answer leaves all state alone. A bundle that no record names stays: its owner is unknown.
//! The sweep runs at `horch fleet` and `horch spawn` at most once an hour
//! ([`run_due`]); `horch doctor` counts what it would remove ([`scan`]), and
//! `horch doctor --clean` removes it ([`Plan::apply`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::execution::legacy::{LedgerRecordV1, STATUS_WORKING};
use crate::execution::store::ExecutionStore;
use crate::messaging::mail::Mail;
use crate::runtime::{Backend, RuntimeContext};
use crate::workspace::client::WorkspaceClient;
use crate::workspace::host::{Hosts, PaneHost};
use crate::workspace::model::{Direction, NewWorkspace, Pane};

/// A spool file lives as long as acked mail does.
const SPOOL_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A workspace dir, a codex home or a temp file younger than this stays: its
/// owner may be starting while the sweep reads.
const SETTLE: Duration = Duration::from_secs(60 * 60);

/// [`run_due`] runs at most once in this interval.
const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The time [`run_due`] may take at `horch fleet` or `horch spawn`.
const HOOK_BUDGET: Duration = Duration::from_secs(3);

/// The time `horch doctor` may take to read the state.
pub const DOCTOR_BUDGET: Duration = Duration::from_secs(30);

const STAMP: &str = ".sweep-stamp";
const REGISTRY_PREFIX: &str = "herdr-orchestration-";

/// What a stale entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Record,
    Spool,
    Socket,
    Registry,
    Mail,
    Bundle,
    CodexHome,
    TempFile,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Record => "working records with a closed pane",
            Kind::Spool => "spool files",
            Kind::Socket => "channel sockets",
            Kind::Registry => "registry dirs",
            Kind::Mail => "mail dirs",
            Kind::Bundle => "skill bundles",
            Kind::CodexHome => "codex homes",
            Kind::TempFile => "temp files",
        }
    }
}

/// The dirs one sweep reads.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub state_root: &'a Path,
    /// The system temp dir, where an older horch kept the registries.
    /// `None` leaves those alone.
    pub temp_root: Option<&'a Path>,
}

/// What herdr says. `listed` is `None` when herdr did not answer.
pub struct Herd<'a> {
    pub listed: Option<BTreeSet<String>>,
    pub client: &'a dyn WorkspaceClient,
}

/// One pane host's view during a multi-host sweep.
pub struct HostView<'a> {
    pub backend: Backend,
    pub listed: Option<BTreeSet<String>>,
    pub client: &'a dyn WorkspaceClient,
}

/// Multi-host view for [`scan_hosts`].
pub struct SweepHosts<'a> {
    pub hosts: Vec<HostView<'a>>,
    pub all_installed_answer: bool,
}

/// One entry to remove. `after` names the records that must end first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    kind: Kind,
    path: PathBuf,
    after: Vec<String>,
}

/// A `working` record to end: its ledger, its record id, and its backend.
#[derive(Debug, Clone)]
struct End {
    store: ExecutionStore,
    record_id: String,
    backend: Backend,
}

/// What a sweep found. Nothing is removed until [`Plan::apply`].
#[derive(Debug, Default)]
pub struct Plan {
    items: Vec<Item>,
    ends: Vec<End>,
}

/// What [`Plan::apply`] did.
#[derive(Debug, Default)]
pub struct Applied {
    pub removed: BTreeMap<Kind, usize>,
    pub errors: Vec<String>,
}

impl Applied {
    pub fn total(&self) -> usize {
        self.removed.values().sum()
    }
}

impl Plan {
    /// The count of stale entries per kind.
    pub fn counts(&self) -> BTreeMap<Kind, usize> {
        let mut counts = BTreeMap::new();
        if !self.ends.is_empty() {
            counts.insert(Kind::Record, self.ends.len());
        }
        for item in &self.items {
            *counts.entry(item.kind).or_insert(0) += 1;
        }
        counts
    }

    /// End the records through `client`, then remove the entries. An entry
    /// that waits for a record stays when that record did not end.
    pub fn apply(self, client: &dyn WorkspaceClient) -> Applied {
        self.apply_hosts(&[(Backend::Herdr, client), (Backend::Hc, client)])
    }

    /// End each record through the client of its recorded [`Backend`], then
    /// remove the entries whose prerequisites ended.
    pub fn apply_hosts(self, clients: &[(Backend, &dyn WorkspaceClient)]) -> Applied {
        let mut applied = Applied::default();
        let mut ended = BTreeSet::new();
        for end in &self.ends {
            let Some((_, client)) = clients.iter().find(|(b, _)| *b == end.backend) else {
                continue;
            };
            let answered = Answered(*client);
            match end.store.end_if_pane_closed(&end.record_id, &answered) {
                Ok(true) => {
                    ended.insert(end.record_id.clone());
                    *applied.removed.entry(Kind::Record).or_insert(0) += 1;
                }
                Ok(false) => {}
                Err(e) => applied
                    .errors
                    .push(format!("ending {}: {e:#}", end.record_id)),
            }
        }
        for item in self.items {
            if !item.after.iter().all(|id| ended.contains(id)) {
                continue;
            }
            match remove(&item.path) {
                Ok(()) => *applied.removed.entry(item.kind).or_insert(0) += 1,
                Err(e) => applied
                    .errors
                    .push(format!("removing {}: {e}", item.path.display())),
            }
        }
        applied
    }
}

/// Remove a file, a link or a whole dir. A link is never followed.
fn remove(path: &Path) -> std::io::Result<()> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) => Err(e),
    };
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Find the stale state under `scope`. `now` is the clock the ages use;
/// the reads stop at `deadline`, and what is not read yet stays.
pub fn scan(scope: Scope<'_>, herd: &Herd<'_>, now: SystemTime, deadline: Instant) -> Plan {
    let hosts = SweepHosts {
        all_installed_answer: herd.listed.is_some(),
        hosts: vec![HostView {
            backend: Backend::Herdr,
            listed: herd.listed.clone(),
            client: herd.client,
        }],
    };
    scan_hosts(scope, &hosts, now, deadline)
}

/// Multi-host sweep: judges each recorded workspace on its owning backend's
/// [`HostView`], and unrecorded folders only when `hosts.all_installed_answer`
/// holds.
pub fn scan_hosts(
    scope: Scope<'_>,
    hosts: &SweepHosts<'_>,
    now: SystemTime,
    deadline: Instant,
) -> Plan {
    let mut scan = Scanner {
        scope,
        hosts,
        now,
        deadline,
        plan: Plan::default(),
    };
    scan.run();
    scan.plan
}

struct Scanner<'a> {
    scope: Scope<'a>,
    hosts: &'a SweepHosts<'a>,
    now: SystemTime,
    deadline: Instant,
    plan: Plan,
}

/// Every record of every project under the state root, with its store: the
/// session records and old JSON ledgers, with the task ledger's worker rows
/// laid over them (TRF-45). `stored` holds the ids of the records a store
/// holds, which a sweep can end; a worker row with no record is the task
/// ledger watchdog's to end.
struct Records {
    rows: Vec<(ExecutionStore, LedgerRecordV1)>,
    stored: BTreeSet<String>,
}

impl Records {
    fn read(state_root: &Path) -> Records {
        let (db, task_ledger) = crate::execution::store::session_files(state_root);
        Self::read_in(state_root, &db, task_ledger.as_deref())
    }

    fn read_in(state_root: &Path, db: &Path, task_ledger: Option<&Path>) -> Records {
        use crate::execution::store::read_all_projects;
        let stored = read_all_projects(state_root, db, None)
            .into_iter()
            .flat_map(|p| p.records)
            .map(|r| r.record_id)
            .collect();
        let mut rows = Vec::new();
        for p in read_all_projects(state_root, db, task_ledger) {
            let Some(stem) = p.path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // Without a project path, the slug stands for it: a slug is its
            // own slug, so the store of that name reads the same records.
            let project = p.project.unwrap_or_else(|| stem.to_string());
            let mut store = ExecutionStore::in_db(state_root, db, &project);
            if let Some(ledger) = task_ledger {
                store = store.with_task_ledger(ledger, Path::new(&project));
            }
            rows.extend(p.records.into_iter().map(|r| (store.clone(), r)));
        }
        Records { rows, stored }
    }
}

impl Scanner<'_> {
    fn late(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn run(&mut self) {
        // Without any host's answer no owner can be proven gone: remove nothing.
        if !self.hosts.hosts.iter().any(|h| h.listed.is_some()) {
            return;
        }
        let records = Records::read(self.scope.state_root);
        let mut ws_backends: BTreeMap<String, Backend> = BTreeMap::new();
        for (_, r) in &records.rows {
            if let Some(ws) = r.workspace_id.as_deref() {
                ws_backends.insert(ws.to_string(), r.backend);
                ws_backends.insert(crate::messaging::file_safe(ws), r.backend);
            }
        }
        self.ends(&records);
        let ending: BTreeSet<String> = self.plan.ends.iter().map(|e| e.record_id.clone()).collect();
        self.bundles(&records, &ending);
        self.codex_homes(&records, &ending);
        let pending = PendingMail::read(self.scope.state_root);
        self.spool(&pending, &ws_backends);
        self.sockets();
        self.workspaces(&pending, &ws_backends);
        self.temp_files(self.scope.state_root, 0);
    }

    /// The `working` records whose workspace or pane their own backend no
    /// longer lists.
    fn ends(&mut self, records: &Records) {
        let mut panes: BTreeMap<(Backend, String), Option<BTreeSet<String>>> = BTreeMap::new();
        for (store, r) in &records.rows {
            if self.late() {
                return;
            }
            if r.status != STATUS_WORKING
                || r.is_orchestrator()
                || !records.stored.contains(&r.record_id)
            {
                continue;
            }
            let (Some(pane), Some(ws)) = (r.pane_id.as_deref(), r.workspace_id.as_deref()) else {
                continue;
            };
            let Some(view) = self.hosts.hosts.iter().find(|h| h.backend == r.backend) else {
                continue;
            };
            let Some(listed) = &view.listed else {
                continue;
            };
            let closed = !listed.contains(ws)
                || panes
                    .entry((r.backend, ws.to_string()))
                    .or_insert_with(|| pane_ids_on(view.client, ws))
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(pane));
            if closed {
                self.plan.ends.push(End {
                    store: store.clone(),
                    record_id: r.record_id.clone(),
                    backend: r.backend,
                });
            }
        }
    }

    /// A bundle named for a terminal record, or for one this sweep ends.
    fn bundles(&mut self, records: &Records, ending: &BTreeSet<String>) {
        let mut terminal: BTreeMap<&str, bool> = BTreeMap::new();
        for (_, r) in &records.rows {
            // The newest row of a record decides: a resumed record is live.
            terminal.insert(&r.record_id, r.execution_status().is_terminal());
        }
        for entry in entries(&self.scope.state_root.join("skill-bundles")) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let after = match terminal.get(name.as_str()) {
                Some(true) => Vec::new(),
                Some(false) if ending.contains(&name) => vec![name],
                _ => continue,
            };
            self.plan.items.push(Item {
                kind: Kind::Bundle,
                path: entry.path(),
                after,
            });
        }
    }

    /// A private codex home whose role has records, all of them terminal
    /// or ending in this sweep. The home names only its role, so any
    /// unfinished record of that role keeps it, and a role with no record
    /// has an unknown owner and stays too.
    fn codex_homes(&mut self, records: &Records, ending: &BTreeSet<String>) {
        // Per role: the unfinished records, which must end first.
        let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, r) in &records.rows {
            let unfinished = owners.entry(codex_slug(&r.role)).or_default();
            if !r.execution_status().is_terminal() {
                unfinished.push(r.record_id.clone());
            }
        }
        for entry in entries(&self.scope.state_root.join("codex-home")) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(after) = name
                .len()
                .checked_sub(37)
                .and_then(|n| owners.get(&name[..n]))
            else {
                continue;
            };
            if !after.iter().all(|id| ending.contains(id))
                || !self.settled(&path)
                || !crate::harness::codex::stranded_entries(&path).is_empty()
            {
                continue;
            }
            self.plan.items.push(Item {
                kind: Kind::CodexHome,
                path,
                after: after.clone(),
            });
        }
    }

    /// Spool files older than [`SPOOL_RETENTION`] that no pending mail
    /// names, and the empty spool dir of a gone workspace.
    fn spool(&mut self, pending: &PendingMail, ws_backends: &BTreeMap<String, Backend>) {
        for dir in entries(&self.scope.state_root.join("messages")) {
            let files = entries(&dir.path());
            if files.is_empty()
                && self.gone(&dir.file_name().to_string_lossy(), pending, ws_backends)
            {
                self.push(Kind::Spool, dir.path());
            }
            for file in files {
                let path = file.path();
                if self.older(&path, SPOOL_RETENTION) && !pending.names(&path) {
                    self.push(Kind::Spool, path);
                }
            }
        }
    }

    /// Sockets that no relay answers.
    fn sockets(&mut self) {
        for entry in entries(&self.scope.state_root.join("channels")) {
            let path = entry.path();
            if self.late() {
                return;
            }
            if path.extension().is_some_and(|e| e == "sock")
                && !crate::messaging::channel::is_live(&path)
            {
                self.push(Kind::Socket, path);
            }
        }
    }

    /// The registry and mail dirs of a workspace its owning host no longer lists.
    fn workspaces(&mut self, pending: &PendingMail, ws_backends: &BTreeMap<String, Backend>) {
        for root in std::iter::once(self.scope.state_root).chain(self.scope.temp_root) {
            for entry in entries(root) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(ws) = name.strip_prefix(REGISTRY_PREFIX) {
                    if self.gone(ws, pending, ws_backends) && self.settled(&entry.path()) {
                        self.push(Kind::Registry, entry.path());
                    }
                }
            }
        }
        for entry in entries(&self.scope.state_root.join("mail")) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if self.gone(&name, pending, ws_backends) && self.settled(&entry.path()) {
                self.push(Kind::Mail, entry.path());
            }
        }
    }

    /// True when `ws` has no pending mail and is provably gone on its recorded
    /// backend (or on every installed host when unrecorded).
    fn gone(
        &self,
        ws: &str,
        pending: &PendingMail,
        ws_backends: &BTreeMap<String, Backend>,
    ) -> bool {
        let safe = crate::messaging::file_safe(ws);
        if pending.holds(&safe) {
            return false;
        }
        if let Some(&backend) = ws_backends.get(ws).or_else(|| ws_backends.get(&safe)) {
            let Some(listed) = self
                .hosts
                .hosts
                .iter()
                .find(|h| h.backend == backend)
                .and_then(|h| h.listed.as_ref())
            else {
                return false;
            };
            return !listed
                .iter()
                .any(|l| l == ws || crate::messaging::file_safe(l) == safe);
        }
        if !self.hosts.all_installed_answer
            || self.hosts.hosts.is_empty()
            || self.hosts.hosts.iter().any(|h| h.listed.is_none())
        {
            return false;
        }
        !self
            .hosts
            .hosts
            .iter()
            .filter_map(|h| h.listed.as_ref())
            .flatten()
            .any(|l| l == ws || crate::messaging::file_safe(l) == safe)
    }

    /// `write_atomic` temp files of a dead process, under `dir`. Bundles and
    /// codex homes are not walked: their owners never write temp files there.
    fn temp_files(&mut self, dir: &Path, depth: usize) {
        if depth > 4 || self.late() {
            return;
        }
        for entry in entries(dir) {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if depth == 0 && matches!(name.as_str(), "skill-bundles" | "codex-home") {
                    continue;
                }
                self.temp_files(&path, depth + 1);
            } else if kind.is_file()
                && temp_pid(&name).is_some_and(|pid| !owner_runs(pid, &path))
                && self.settled(&path)
            {
                self.push(Kind::TempFile, path);
            }
        }
    }

    fn push(&mut self, kind: Kind, path: PathBuf) {
        self.plan.items.push(Item {
            kind,
            path,
            after: Vec::new(),
        });
    }

    fn settled(&self, path: &Path) -> bool {
        self.older(path, SETTLE)
    }

    /// Whether `path` was last modified more than `age` before `now`. An
    /// unreadable time is not old.
    fn older(&self, path: &Path, age: Duration) -> bool {
        std::fs::symlink_metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| self.now.duration_since(t).ok())
            .is_some_and(|d| d > age)
    }
}

/// The pending mail of every workspace: which workspaces have it, and the
/// text, to see which spool files it names.
struct PendingMail {
    workspaces: BTreeSet<String>,
    texts: Vec<String>,
}

impl PendingMail {
    fn read(state_root: &Path) -> PendingMail {
        let mut out = PendingMail {
            workspaces: BTreeSet::new(),
            texts: Vec::new(),
        };
        for entry in entries(&state_root.join("mail")) {
            let ws = entry.file_name().to_string_lossy().into_owned();
            let mail = Mail::under(state_root, &ws);
            let roles = mail.roles().unwrap_or_default();
            let boxes = std::iter::once(mail).chain(roles.into_iter().map(|(_, m)| m));
            for mail in boxes {
                // Mail that cannot be read counts as pending: it stays.
                let Ok(messages) = mail.pending() else {
                    out.workspaces.insert(ws.clone());
                    continue;
                };
                if !messages.is_empty() {
                    out.workspaces.insert(ws.clone());
                }
                out.texts.extend(messages.into_iter().map(|m| m.text));
            }
        }
        out
    }

    fn holds(&self, safe_ws: &str) -> bool {
        self.workspaces.contains(safe_ws)
    }

    fn names(&self, path: &Path) -> bool {
        let path = path.to_string_lossy();
        self.texts.iter().any(|t| t.contains(path.as_ref()))
    }
}

/// The entries of `dir`, sorted; none when it cannot be read.
fn entries(dir: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = read.flatten().collect();
    out.sort_by_key(|e| e.file_name());
    out
}

/// The pid in a `write_atomic` temp name: `.<name>.tmp-<pid>-<nonce>`, or
/// the older `<name>.tmp.<pid>`.
fn temp_pid(name: &str) -> Option<u32> {
    if let Some((_, rest)) = name.strip_prefix('.').and_then(|n| n.rsplit_once(".tmp-")) {
        let (pid, nonce) = rest.split_once('-')?;
        let hex = nonce.len() == 12 && nonce.chars().all(|c| c.is_ascii_hexdigit());
        return hex.then(|| pid.parse().ok()).flatten();
    }
    let (_, pid) = name.rsplit_once(".tmp.")?;
    pid.parse().ok()
}

/// Whether the process that wrote `path` may still run: `pid` runs and,
/// where the start time can be read, started before the file was written.
fn owner_runs(pid: u32, path: &Path) -> bool {
    if !crate::procid::alive(pid, None) {
        return false;
    }
    let written = std::fs::symlink_metadata(path).and_then(|m| m.modified());
    match (crate::procid::start_time(pid), written) {
        (Some(_), Ok(t)) => crate::procid::started_by(pid, t),
        _ => true,
    }
}

/// The role part of a codex home name, as the codex harness writes it.
fn codex_slug(role: &str) -> String {
    role.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// A client whose server is known to answer. Every other call goes to the
/// wrapped client.
struct Answered<'a>(&'a dyn WorkspaceClient);

impl WorkspaceClient for Answered<'_> {
    fn pane_get(&self, pane: &str) -> anyhow::Result<Pane> {
        self.0.pane_get(pane)
    }
    fn pane_list(&self, workspace: &str) -> anyhow::Result<Vec<Pane>> {
        self.0.pane_list(workspace)
    }
    fn pane_split(&self, from: &str, direction: Direction) -> anyhow::Result<String> {
        self.0.pane_split(from, direction)
    }
    fn pane_run(&self, pane: &str, command: &str) -> anyhow::Result<()> {
        self.0.pane_run(pane, command)
    }
    fn pane_close(&self, pane: &str) -> anyhow::Result<()> {
        self.0.pane_close(pane)
    }
    fn agent_prompt(&self, pane: &str, text: &str) -> anyhow::Result<()> {
        self.0.agent_prompt(pane, text)
    }
    fn pane_send_text(&self, pane: &str, text: &str) -> anyhow::Result<()> {
        self.0.pane_send_text(pane, text)
    }
    fn pane_send_keys(&self, pane: &str, keys: &str) -> anyhow::Result<()> {
        self.0.pane_send_keys(pane, keys)
    }
    fn pane_read(&self, pane: &str, source: &str) -> anyhow::Result<String> {
        self.0.pane_read(pane, source)
    }
    fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> anyhow::Result<NewWorkspace> {
        self.0.workspace_create(label, cwd, focus)
    }
    fn workspace_close(&self, workspace: &str) -> anyhow::Result<()> {
        self.0.workspace_close(workspace)
    }
    fn server_reachable(&self) -> bool {
        true
    }
}

fn pane_ids_on(client: &dyn WorkspaceClient, ws: &str) -> Option<BTreeSet<String>> {
    let panes = client.pane_list(ws).ok()?;
    Some(panes.into_iter().map(|p| p.pane_id).collect())
}

/// What herdr lists, through `herdr`. `None` when it does not answer.
pub fn listed(herdr: &PaneHost) -> Option<BTreeSet<String>> {
    let workspaces = herdr.workspace_list().ok()?;
    Some(workspaces.into_iter().map(|w| w.workspace_id).collect())
}

/// The sweep with the context's dirs and herdr, cut off at `budget`.
pub fn plan_in(ctx: &RuntimeContext, herdr: &PaneHost, budget: Duration) -> (Plan, Option<usize>) {
    let deadline = Instant::now() + budget;
    let herdr = herdr.clone().until(Some(deadline));
    let herd = Herd {
        listed: listed(&herdr),
        client: &herdr,
    };
    let scope = scope_of(ctx);
    let plan = scan(scope, &herd, SystemTime::now(), deadline);
    let workspaces = herd.listed.as_ref().map(BTreeSet::len);
    (plan, workspaces)
}

/// Sweep now, through `herdr`, and apply.
pub fn clean_in(ctx: &RuntimeContext, herdr: &PaneHost, budget: Duration) -> Applied {
    let deadline = Instant::now() + budget;
    let herdr = herdr.clone().until(Some(deadline));
    let (plan, _) = plan_in(ctx, &herdr, budget);
    plan.apply(&herdr)
}

fn scope_of(ctx: &RuntimeContext) -> Scope<'_> {
    // The temp registries belong to the default state root: a moved state
    // root (a test, a second install) must not judge them.
    Scope {
        state_root: &ctx.paths.state_root,
        temp_root: ctx
            .paths
            .state_override
            .is_none()
            .then_some(ctx.paths.temp_root.as_path()),
    }
}

/// Plan the sweep across every installed pane host (`Hosts::reachable`),
/// returning `(plan, answered_host_count, unreachable_backends)`.
pub fn plan_all(ctx: &RuntimeContext, budget: Duration) -> (Plan, usize, Vec<Backend>) {
    let deadline = Instant::now() + budget;
    let probed = Hosts::reachable(ctx);
    let bounded: Vec<PaneHost> = probed
        .hosts
        .into_iter()
        .map(|h| h.until(Some(deadline)))
        .collect();
    let mut unreachable = probed.unreachable;
    let mut views = Vec::with_capacity(bounded.len());
    let mut answered = 0usize;
    for host in &bounded {
        let l = listed(host);
        if l.is_some() {
            answered += 1;
        } else {
            unreachable.push(host.backend());
        }
        views.push(HostView {
            backend: host.backend(),
            listed: l,
            client: host,
        });
    }
    let sweep_hosts = SweepHosts {
        all_installed_answer: answered > 0 && unreachable.is_empty(),
        hosts: views,
    };
    let plan = scan_hosts(scope_of(ctx), &sweep_hosts, SystemTime::now(), deadline);
    (plan, answered, unreachable)
}

/// Sweep across every installed pane host and apply.
pub fn clean_all(ctx: &RuntimeContext, budget: Duration) -> (Applied, Vec<Backend>) {
    let deadline = Instant::now() + budget;
    let probed = Hosts::reachable(ctx);
    let bounded: Vec<PaneHost> = probed
        .hosts
        .into_iter()
        .map(|h| h.until(Some(deadline)))
        .collect();
    let mut unreachable = probed.unreachable;
    let mut views = Vec::with_capacity(bounded.len());
    let mut answered = 0usize;
    for host in &bounded {
        let l = listed(host);
        if l.is_some() {
            answered += 1;
        } else {
            unreachable.push(host.backend());
        }
        views.push(HostView {
            backend: host.backend(),
            listed: l,
            client: host,
        });
    }
    let sweep_hosts = SweepHosts {
        all_installed_answer: answered > 0 && unreachable.is_empty(),
        hosts: views,
    };
    let plan = scan_hosts(scope_of(ctx), &sweep_hosts, SystemTime::now(), deadline);
    let clients: Vec<(Backend, &dyn WorkspaceClient)> = bounded
        .iter()
        .map(|h| (h.backend(), h as &dyn WorkspaceClient))
        .collect();
    (plan.apply_hosts(&clients), unreachable)
}

/// Whether the stamp at `stamp` is missing or [`INTERVAL`] old at `now`.
fn due(stamp: &Path, now: SystemTime) -> bool {
    std::fs::metadata(stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| now.duration_since(t).ok())
        .is_none_or(|d| d >= INTERVAL)
}

/// Sweep when the last sweep is an hour old, within 3 seconds.
/// Best effort: it never fails, and it logs only errors and what it removed.
pub fn run_due(ctx: &RuntimeContext) {
    let stamp = ctx.paths.state_root.join(STAMP);
    if !due(&stamp, SystemTime::now()) || !ctx.paths.state_root.is_dir() {
        return;
    }
    // Stamp first, so 2 spawns at once do not both sweep.
    if let Err(e) = std::fs::write(&stamp, b"") {
        eprintln!(
            "horch: the state sweep cannot write {}: {e}",
            stamp.display()
        );
        return;
    }
    let (applied, _) = clean_all(ctx, HOOK_BUDGET);
    for error in &applied.errors {
        eprintln!("horch: state sweep: {error}");
    }
    if applied.total() > 0 {
        eprintln!(
            "horch: removed {} stale state entries (horch doctor lists the kinds)",
            applied.total()
        );
    }
}

#[cfg(test)]
mod tests;
