//! The live screen (section 12.4): a worker thread that owns the source and
//! does every file read and tick, an input thread that forwards terminal
//! events, and the UI thread that only holds snapshots and draws (SPC-09).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{Event, KeyEvent, MouseEvent, MouseEventKind};
use horch_core::clock;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, Probing, Snapshot};
use horch_core::telemetry::lock;
use horch_core::telemetry::store;
use horch_core::usage::Locations;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::view::{draw_buf, on_key, on_wheel, table_at, Action, Rows, ViewState};
use super::SHUTDOWN;

/// What the UI thread asks of the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Tick now (the `r` key).
    Refresh,
    /// Filter the rollups to this project, or show all.
    SetProject(Option<String>),
    Stop,
}

/// What the worker sends after a tick that changed something, a project
/// change, a new minute with a project set, and after every `r`.
#[derive(Debug, Clone)]
pub struct Update {
    pub snapshot: Arc<Snapshot>,
    /// `snapshot` with its rollups for `project` only.
    pub filtered: Option<Arc<Snapshot>>,
    /// The project `filtered` is for.
    pub project: Option<String>,
    /// The last tick's error, if any.
    pub status: Option<String>,
    /// `collector` or `viewer`.
    pub mode: &'static str,
}

/// Every message the UI thread waits for, on 1 channel.
pub enum Msg {
    Update(Update),
    Input(crossterm::event::Event),
    /// SIGINT, SIGTERM or SIGHUP.
    Shutdown,
    /// The worker ended without a `Stop` (a panic).
    WorkerGone,
    /// The input thread ended: the terminal is gone, or a read failed.
    InputGone(String),
    /// Nothing arrived before the wait ended.
    Idle,
}

/// Where the worker gets snapshots from.
pub trait Feed {
    /// A new snapshot, or `None` when nothing changed since the last call.
    fn next(&mut self) -> Result<Option<Snapshot>>;
    /// `snap` with its rollups recomputed for `project` only, with the
    /// windows that start before `now`.
    fn filter(&mut self, snap: &Snapshot, project: &str, now: DateTime<Utc>) -> Snapshot;
    fn mode(&self) -> &'static str;
}

/// Sends [`Msg::WorkerGone`] when the worker unwinds.
struct GoneGuard<'a>(&'a Sender<Msg>, bool);

impl Drop for GoneGuard<'_> {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.send(Msg::WorkerGone);
        }
    }
}

/// (generated_at, project, minute): what a filtered snapshot is for.
type FilterKey = (String, String, i64);

/// The last filtered snapshot and the key it is for.
#[derive(Default)]
struct FilterCache(Option<(FilterKey, Arc<Snapshot>)>);

impl FilterCache {
    fn key(snap: &Snapshot, project: &str, minute: i64) -> FilterKey {
        (snap.generated_at.clone(), project.to_string(), minute)
    }

    /// The cached snapshot, when it is for this key.
    fn get(&self, snap: &Snapshot, project: &str, minute: i64) -> Option<Arc<Snapshot>> {
        let key = Self::key(snap, project, minute);
        self.0.as_ref().filter(|c| c.0 == key).map(|c| c.1.clone())
    }

    /// A project is set and the cache is not for it.
    fn stale(&self, snap: &Snapshot, project: Option<&str>, minute: i64) -> bool {
        project.is_some_and(|p| self.get(snap, p, minute).is_none())
    }

    /// The snapshot filtered to `project`, from the cache or the feed.
    fn filtered<F: Feed>(
        &mut self,
        feed: &mut F,
        snap: &Snapshot,
        project: &str,
        minute: i64,
        now: &dyn Fn() -> DateTime<Utc>,
    ) -> Arc<Snapshot> {
        if let Some(f) = self.get(snap, project, minute) {
            return f;
        }
        let at = DateTime::from_timestamp(minute * 60, 0).unwrap_or_else(now);
        let f = Arc::new(feed.filter(snap, project, at));
        self.0 = Some((Self::key(snap, project, minute), f.clone()));
        f
    }
}

/// What the feed last gave the worker.
struct Seen {
    snap: Arc<Snapshot>,
    status: Option<String>,
    mode: &'static str,
}

/// What the UI asked of the worker since the last loop.
struct Asked {
    project: Option<String>,
    project_changed: bool,
    refresh: bool,
    due: Instant,
}

/// One feed tick into `seen`. Whether the UI needs an update: `changed`
/// from the caller, or a new snapshot, status or mode.
fn tick<F: Feed>(feed: &mut F, seen: &mut Seen, mut changed: bool) -> bool {
    match feed.next() {
        Ok(Some(mut s)) => {
            let at = std::mem::replace(&mut s.generated_at, seen.snap.generated_at.clone());
            // The first tick replaces the empty start snapshot.
            if changed || *seen.snap != s {
                s.generated_at = at;
                seen.snap = Arc::new(s);
                changed = true;
            }
            changed |= seen.status.take().is_some();
        }
        Ok(None) => changed |= seen.status.take().is_some(),
        Err(e) => {
            let s = Some(format!("{e:#}"));
            if s != seen.status {
                seen.status = s;
                changed = true;
            }
        }
    }
    if feed.mode() != seen.mode {
        seen.mode = feed.mode();
        changed = true;
    }
    changed
}

/// How long the worker waits for a request. With a project set the filtered
/// rollup expires at the end of the minute it was computed for (at once
/// when that has passed).
fn wait_for(due: Instant, project: bool, minute: i64, now: &dyn Fn() -> DateTime<Utc>) -> Duration {
    let wait = due.saturating_duration_since(Instant::now());
    if !project {
        return wait;
    }
    let ms = ((minute + 1) * 60_000 - now().timestamp_millis()).max(0);
    wait.min(Duration::from_millis(ms as u64))
}

/// A request, or the end of the wait. Whether the worker goes on.
fn on_request(got: Result<Request, RecvTimeoutError>, asked: &mut Asked) -> bool {
    match got {
        Ok(Request::Refresh) => {
            asked.due = Instant::now();
            asked.refresh = true;
        }
        Ok(Request::SetProject(p)) => {
            if p != asked.project {
                asked.project = p;
                asked.project_changed = true;
            }
        }
        Ok(Request::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
        Err(RecvTimeoutError::Timeout) => {}
    }
    true
}

/// The worker loop: a tick every `tick` from the start of the last one (a
/// tick that overruns is followed at once, with no catch-up), and at once on
/// [`Request::Refresh`]. A snapshot that differs from the last one only in
/// `generated_at` is no update; a change of [`Feed::mode`] is one, and a
/// refresh always sends one. The filtered snapshot is computed only when
/// the snapshot's `generated_at`, the project, or the minute of `now()`
/// changes: a new minute moves the window starts, so with a project set the
/// worker wakes at each minute. Returns on [`Request::Stop`] or when the UI
/// is gone.
pub fn worker<F: Feed>(
    feed: &mut F,
    tick_every: Duration,
    now: impl Fn() -> DateTime<Utc>,
    rx: Receiver<Request>,
    tx: Sender<Msg>,
) {
    let mut guard = GoneGuard(&tx, false);
    let mut seen = Seen {
        snap: Arc::default(),
        status: None,
        mode: feed.mode(),
    };
    let mut asked = Asked {
        project: None,
        project_changed: false,
        refresh: false,
        due: Instant::now(),
    };
    let mut cache = FilterCache::default();
    let mut first = true;
    loop {
        let mut changed = std::mem::take(&mut first) | std::mem::take(&mut asked.refresh);
        if Instant::now() >= asked.due {
            let start = Instant::now();
            changed = tick(feed, &mut seen, changed);
            asked.due = start + tick_every;
        }
        let minute = now().timestamp().div_euclid(60);
        let project = asked.project.as_deref();
        let stale = cache.stale(&seen.snap, project, minute);
        if changed || std::mem::take(&mut asked.project_changed) || stale {
            let filtered = project.map(|p| cache.filtered(feed, &seen.snap, p, minute, &now));
            let update = Update {
                snapshot: seen.snap.clone(),
                filtered,
                project: asked.project.clone(),
                status: seen.status.clone(),
                mode: seen.mode,
            };
            if tx.send(Msg::Update(update)).is_err() {
                break;
            }
        }
        let wait = wait_for(asked.due, project.is_some(), minute, &now);
        if !on_request(rx.recv_timeout(wait), &mut asked) {
            break;
        }
    }
    guard.1 = true;
}

/// Where a snapshot comes from on each refresh. The collector variant holds
/// the lock for as long as it lives. `_lock` is never read; its `Drop`
/// releases the lock.
pub enum Source {
    Collector {
        collector: Box<Collector>,
        _lock: lock::CollectorLock,
    },
    Viewer,
}

/// The real [`Feed`]: the collector's tick, or `snapshot.json` for a viewer.
pub struct LiveFeed<'a> {
    pub ctx: &'a RuntimeContext,
    pub root: PathBuf,
    pub source: Source,
    /// The modification time and length of `snapshot.json` when a viewer
    /// last read it.
    seen: Option<(SystemTime, u64)>,
}

impl<'a> LiveFeed<'a> {
    pub fn new(ctx: &'a RuntimeContext, root: &Path, source: Source) -> LiveFeed<'a> {
        LiveFeed {
            ctx,
            root: root.to_path_buf(),
            source,
            seen: None,
        }
    }
}

impl LiveFeed<'_> {
    /// A viewer whose collector died takes over (the next `ensure` would).
    fn take_over(&mut self) -> Result<()> {
        let (ctx, root) = (self.ctx, self.root.as_path());
        if !matches!(self.source, Source::Viewer) || lock::collector_live(root) {
            return Ok(());
        }
        if let Ok(Ok(held)) = lock::acquire(root, &lock::this_process(&clock::now_stamp(), ctx)) {
            let c = Collector::open_in(
                ctx,
                Locations::from_context(ctx),
                Probing::Scheduled,
                clock::now(),
            )?;
            self.source = Source::Collector {
                collector: Box::new(c),
                _lock: held,
            };
        }
        Ok(())
    }

    /// A viewer's snapshot, when `snapshot.json` changed since it last read it.
    fn read_if_new(&mut self) -> Result<Option<Snapshot>> {
        let path = collect::snapshot_path(&self.root);
        let stamp = std::fs::metadata(&path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        if stamp.is_some() && stamp == self.seen {
            return Ok(None);
        }
        let snap = Snapshot::read(&path)?;
        self.seen = stamp;
        Ok(Some(snap))
    }
}

impl Feed for LiveFeed<'_> {
    fn next(&mut self) -> Result<Option<Snapshot>> {
        self.take_over()?;
        match &mut self.source {
            Source::Collector { collector, .. } => collector.tick(clock::now()).map(Some),
            Source::Viewer => self.read_if_new(),
        }
    }

    /// A collector folds its in-memory events and reads no file; a viewer
    /// reads the event files (on this worker thread, cached per minute).
    fn filter(&mut self, snap: &Snapshot, project: &str, now: DateTime<Utc>) -> Snapshot {
        match &self.source {
            Source::Collector { collector, .. } => {
                let mut out = snap.clone();
                out.rollups = collector.project_rollups(project, now);
                out
            }
            Source::Viewer => filtered(snap, project, &self.root, now),
        }
    }

    fn mode(&self) -> &'static str {
        match self.source {
            Source::Collector { .. } => "collector",
            Source::Viewer => "viewer",
        }
    }
}

/// The snapshot with its rollups recomputed for one project, from the event
/// files, for the windows at `now`: a viewer's filter. Runs on the worker
/// thread only.
fn filtered(snap: &Snapshot, project: &str, root: &Path, now: DateTime<Utc>) -> Snapshot {
    let events: Vec<_> = store::read_all(&horch_core::telemetry::dir(root))
        .into_iter()
        .filter(|e| e.project.as_deref() == Some(project))
        .collect();
    let done = collect::done_record_ids(&collect::read_ledgers(root));
    let mut out = snap.clone();
    for w in collect::WINDOWS {
        let since = collect::window_start(w, now);
        out.rollups.insert(
            w.to_string(),
            store::rollup(&events, since.as_deref(), &done),
        );
    }
    out
}

/// Where the input thread reads terminal events from.
pub trait Events {
    fn poll(&mut self, timeout: Duration) -> std::io::Result<bool>;
    fn read(&mut self) -> std::io::Result<crossterm::event::Event>;
}

/// The real terminal.
pub struct TermEvents;

impl Events for TermEvents {
    fn poll(&mut self, timeout: Duration) -> std::io::Result<bool> {
        crossterm::event::poll(timeout)
    }
    fn read(&mut self) -> std::io::Result<crossterm::event::Event> {
        crossterm::event::read()
    }
}

/// The next terminal event as a message, or `None` when nothing arrived
/// within the poll.
fn read_msg<E: Events>(events: &mut E) -> Option<Msg> {
    let ev = match events.poll(Duration::from_millis(200)) {
        Ok(true) => events.read(),
        Ok(false) => return None,
        Err(e) => Err(e),
    };
    Some(match ev {
        Ok(ev) => Msg::Input(ev),
        Err(e) => Msg::InputGone(format!("the terminal input failed: {e}")),
    })
}

/// Forwards terminal events, and a shutdown signal, until `stop` is set.
/// Every other way out tells the UI first: a read error sends
/// [`Msg::InputGone`].
fn input<E: Events>(mut events: E, tx: Sender<Msg>, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        if SHUTDOWN.load(Ordering::SeqCst) {
            let _ = tx.send(Msg::Shutdown);
            return;
        }
        let Some(msg) = read_msg(&mut events) else {
            continue;
        };
        let gone = matches!(msg, Msg::InputGone(_));
        if tx.send(msg).is_err() || gone {
            return;
        }
    }
}

/// The UI's next message, waiting at most `wait`. A shutdown signal is
/// seen here too, so the UI stops when no thread is left to forward it;
/// a channel with no sender left is [`Msg::WorkerGone`].
pub fn recv(rx: &Receiver<Msg>, wait: Duration) -> Msg {
    // Wake at least this often to look at the shutdown flag.
    const POLL: Duration = Duration::from_millis(250);
    let until = Instant::now() + wait;
    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            break Msg::Shutdown;
        }
        let left = until.saturating_duration_since(Instant::now());
        let got = match rx.recv_timeout(left.min(POLL)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Disconnected) => Some(Msg::WorkerGone),
            Err(RecvTimeoutError::Timeout) if left <= POLL => Some(Msg::Idle),
            Err(RecvTimeoutError::Timeout) => None,
        };
        if let Some(m) = got {
            break m;
        }
    }
}

/// Run the screen: the worker and input threads, and `ui` on this thread
/// with the receiving end. `ui` returns when the operator quits, or on
/// [`Msg::Shutdown`], [`Msg::WorkerGone`] or [`Msg::InputGone`]; then the
/// worker is stopped and joined, so the lock is released before return.
pub fn run<F, E, U>(feed: &mut F, tick: Duration, events: E, ui: U) -> Result<()>
where
    F: Feed + Send,
    E: Events + Send,
    U: FnOnce(&Receiver<Msg>, &Sender<Request>) -> Result<()>,
{
    let (req_tx, req_rx) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        let wtx = tx.clone();
        let worker = std::thread::Builder::new()
            .name("telemetry-worker".into())
            .spawn_scoped(s, move || worker(feed, tick, clock::now, req_rx, wtx))?;
        let itx = tx.clone();
        let stop = &stop;
        let input = std::thread::Builder::new()
            .name("telemetry-input".into())
            .spawn_scoped(s, move || input(events, itx, stop))?;
        drop(tx);
        let out = ui(&rx, &req_tx);
        let _ = req_tx.send(Request::Stop);
        stop.store(true, Ordering::SeqCst);
        let _ = input.join();
        let panicked = worker.join().is_err();
        match out {
            Ok(()) if panicked => anyhow::bail!("the telemetry worker thread panicked"),
            other => other,
        }
    })
}

/// Button and wheel reports in the SGR form (modes 1000 and 1006), not
/// every pointer move (mode 1003).
pub const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1006h";
pub const MOUSE_OFF: &str = "\x1b[?1006l\x1b[?1000l";

/// Restores the terminal however the screen loop ends, also after a panic:
/// it ends a synchronized update first, so the terminal shows the rest.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = crossterm::execute!(out, crossterm::terminal::EndSynchronizedUpdate);
        let _ = out.write_all(MOUSE_OFF.as_bytes());
        let _ = crossterm::execute!(
            out,
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// A synchronized update: begun on [`Synced::begin`], ended and flushed on
/// drop, also when a write in between fails.
pub struct Synced<'a, W: std::io::Write>(&'a mut W);

impl<'a, W: std::io::Write> Synced<'a, W> {
    pub fn begin(w: &'a mut W) -> std::io::Result<Synced<'a, W>> {
        crossterm::queue!(w, crossterm::terminal::BeginSynchronizedUpdate)?;
        Ok(Synced(w))
    }
}

impl<W: std::io::Write> Drop for Synced<'_, W> {
    fn drop(&mut self) {
        let _ = crossterm::queue!(self.0, crossterm::terminal::EndSynchronizedUpdate);
        let _ = self.0.flush();
    }
}

impl<W: std::io::Write> std::ops::Deref for Synced<'_, W> {
    type Target = W;
    fn deref(&self) -> &W {
        self.0
    }
}

impl<W: std::io::Write> std::ops::DerefMut for Synced<'_, W> {
    fn deref_mut(&mut self) -> &mut W {
        self.0
    }
}

/// Write `next` inside 1 synchronized update: only the cells that differ
/// from `prev`, or every cell when there is no `prev` of the same size (the
/// first frame, a resize). No clear: every cell is written.
pub fn write_frame<W: std::io::Write>(
    out: &mut ratatui::backend::CrosstermBackend<W>,
    prev: Option<&ratatui::buffer::Buffer>,
    next: &ratatui::buffer::Buffer,
) -> std::io::Result<()> {
    use ratatui::backend::Backend;
    use ratatui::buffer::Cell;
    let blank;
    let prev = match prev {
        Some(p) if p.area == next.area => p,
        // A symbol no frame holds: every cell differs.
        _ => {
            blank = Buffer::filled(next.area, Cell::new("\0"));
            &blank
        }
    };
    let mut out = Synced::begin(out)?;
    out.draw(prev.diff(next).into_iter())?;
    Backend::flush(&mut *out)
}

/// When the loop draws (SPC-08): a new update, a change of the view, a
/// resize, or a new minute on the header clock. Nothing else draws.
#[derive(Debug, Default)]
pub struct Redraw {
    minute: Option<i64>,
    size: Option<(u16, u16)>,
}

impl Redraw {
    /// Whether to draw now, and whether the terminal was resized (then
    /// every cell is written again; nothing is cleared).
    pub fn due(&mut self, minute: i64, size: (u16, u16), update: bool, view: bool) -> (bool, bool) {
        let resized = self.size != Some(size);
        let new_minute = self.minute != Some(minute);
        self.size = Some(size);
        self.minute = Some(minute);
        (update || view || resized || new_minute, resized)
    }
}

/// The snapshot to show, and whether the view's project is still loading:
/// the filtered snapshot when it is for the view's project; else the global
/// one, whose rollups the view must not show under a project.
pub fn shown<'a>(u: &'a Update, view: &ViewState) -> (&'a Snapshot, bool) {
    match &u.filtered {
        _ if view.project.is_none() => (&u.snapshot, false),
        Some(f) if u.project == view.project => (f, false),
        _ => (&u.snapshot, true),
    }
}

/// Raw mode, the alternate screen and the mouse on. The guard restores the
/// terminal, also when a step in between fails.
fn enter_screen() -> Result<(TerminalGuard, std::io::Stdout)> {
    use std::io::Write;
    crossterm::terminal::enable_raw_mode()?;
    let guard = TerminalGuard;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )?;
    stdout.write_all(MOUSE_ON.as_bytes())?;
    stdout.flush()?;
    Ok((guard, stdout))
}

/// The state of the screen loop on the UI thread.
struct Ui<'a> {
    out: CrosstermBackend<std::io::Stdout>,
    req: &'a Sender<Request>,
    view: ViewState,
    /// Shown until the first update arrives.
    empty: Update,
    last: Option<Update>,
    redraw: Redraw,
    prev: Option<Buffer>,
    /// An update arrived since the last draw.
    update: bool,
    /// The view changed since the last draw.
    changed: bool,
    /// The terminal area at the last draw.
    area: Rect,
}

impl<'a> Ui<'a> {
    fn new(
        out: CrosstermBackend<std::io::Stdout>,
        view: ViewState,
        req: &'a Sender<Request>,
    ) -> Self {
        Ui {
            out,
            req,
            view,
            empty: Update {
                snapshot: Arc::default(),
                filtered: None,
                project: None,
                status: None,
                mode: "",
            },
            last: None,
            redraw: Redraw::default(),
            prev: None,
            update: true,
            changed: true,
            area: Rect::default(),
        }
    }

    /// The snapshot to show, and whether it is still loading.
    fn shown(&self) -> (&Snapshot, bool) {
        shown(self.last.as_ref().unwrap_or(&self.empty), &self.view)
    }

    /// The rows the keys act on.
    fn rows(&self) -> Rows {
        Rows::of(self.shown().0, &self.view, self.area)
    }

    /// Draw when something changed, and write only the changed cells.
    fn draw(&mut self, now: DateTime<Utc>) -> Result<()> {
        let (w, h) = crossterm::terminal::size()?;
        self.area = Rect::new(0, 0, w, h);
        let (draw, resized) = self.redraw.due(
            now.timestamp().div_euclid(60),
            (w, h),
            std::mem::take(&mut self.update),
            std::mem::take(&mut self.changed),
        );
        if !draw {
            return Ok(());
        }
        self.view.loading = self.shown().1;
        let u = self.last.as_ref().unwrap_or(&self.empty);
        let snap = shown(u, &self.view).0;
        self.view.sync(&Rows::of(snap, &self.view, self.area));
        let mut buf = Buffer::empty(self.area);
        draw_buf(&mut buf, self.area, snap, &self.view, now);
        if resized {
            self.prev = None;
        }
        // An equal frame writes nothing, not even the sync marks.
        if self.prev.as_ref() != Some(&buf) {
            write_frame(&mut self.out, self.prev.as_ref(), &buf)?;
            self.prev = Some(buf);
        }
        Ok(())
    }

    fn set_view(&mut self, next: ViewState) {
        self.changed = next != self.view;
        self.view = next;
    }

    fn on_key(&mut self, k: KeyEvent) -> Action {
        let rows = self.rows();
        let (next, action) = on_key(self.view.clone(), k, &rows);
        self.set_view(next);
        action
    }

    fn on_mouse(&mut self, m: MouseEvent) -> Action {
        let down = match m.kind {
            MouseEventKind::ScrollDown => true,
            MouseEventKind::ScrollUp => false,
            _ => return Action::None,
        };
        let snap = self.shown().0;
        let Some(t) = table_at(snap, &self.view, self.area, m.column, m.row) else {
            return Action::None;
        };
        let rows = Rows::of(snap, &self.view, self.area);
        let (next, action) = on_wheel(self.view.clone(), t, down, &rows);
        self.set_view(next);
        action
    }

    /// A message: whether the loop ends.
    fn on_msg(&mut self, msg: Msg) -> Result<bool> {
        let action = match msg {
            Msg::Idle => Action::None,
            Msg::Update(u) => {
                self.view.status = u.status.clone();
                self.view.mode = Some(u.mode);
                self.last = Some(u);
                self.update = true;
                Action::None
            }
            Msg::Input(Event::Key(k)) => self.on_key(k),
            Msg::Input(Event::Mouse(m)) => self.on_mouse(m),
            // A resize draws at once: the loop reads the new size.
            Msg::Input(_) => Action::None,
            Msg::Shutdown | Msg::WorkerGone => return Ok(true),
            Msg::InputGone(why) => anyhow::bail!(why),
        };
        Ok(self.act(action))
    }

    /// What the action asks of the worker: whether the loop ends.
    fn act(&self, action: Action) -> bool {
        match action {
            Action::Quit => return true,
            Action::Refresh => {
                let _ = self.req.send(Request::Refresh);
            }
            Action::SetProject(p) => {
                let _ = self.req.send(Request::SetProject(p));
            }
            Action::None | Action::Redraw => {}
        }
        false
    }
}

/// The screen loop on a terminal. It reads no file and runs no tick: the
/// worker does (SPC-09). It writes to the terminal only when the frame
/// changed, and only the changed cells (SPC-08).
pub fn screen<F: Feed + Send>(feed: &mut F, tick: Duration) -> Result<()> {
    let (_guard, stdout) = enter_screen()?;
    let view = ViewState {
        color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        status: Some("waiting for the first tick".into()),
        ..ViewState::default()
    };
    run(feed, tick, TermEvents, |rx, req| {
        let mut ui = Ui::new(CrosstermBackend::new(stdout), view, req);
        loop {
            let now = clock::now();
            ui.draw(now)?;
            // Wake at the next minute boundary for the header clock.
            let wait =
                Duration::from_millis(60_000 - now.timestamp_millis().rem_euclid(60_000) as u64);
            if ui.on_msg(recv(rx, wait))? {
                return Ok(());
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, AtomicUsize};
    use std::sync::Mutex;

    /// A feed that returns `snaps` in turn, then the last one again, and
    /// counts its filter calls. It is a viewer for its first `viewer_for`
    /// calls of `next`, then a collector.
    struct FakeFeed {
        snaps: Vec<Snapshot>,
        at: usize,
        filters: Arc<AtomicUsize>,
        viewer_for: usize,
    }

    impl FakeFeed {
        fn new(snaps: Vec<Snapshot>) -> FakeFeed {
            FakeFeed {
                snaps,
                at: 0,
                filters: Arc::default(),
                viewer_for: 0,
            }
        }
    }

    impl Feed for FakeFeed {
        fn next(&mut self) -> Result<Option<Snapshot>> {
            let s = self.snaps[self.at.min(self.snaps.len() - 1)].clone();
            self.at += 1;
            Ok(Some(s))
        }
        fn filter(&mut self, snap: &Snapshot, _project: &str, now: DateTime<Utc>) -> Snapshot {
            self.filters.fetch_add(1, Ordering::SeqCst);
            let mut out = snap.clone();
            out.collector.started_at = clock::stamp(now);
            out
        }
        fn mode(&self) -> &'static str {
            if self.at < self.viewer_for {
                "viewer"
            } else {
                "collector"
            }
        }
    }

    fn snap(generated_at: &str, projects: usize) -> Snapshot {
        Snapshot {
            generated_at: generated_at.into(),
            projects,
            ..Snapshot::default()
        }
    }

    fn fixed() -> DateTime<Utc> {
        clock::parse("2026-10-07T12:00:30Z").unwrap()
    }

    /// Wait for the next update.
    fn update(rx: &Receiver<Msg>) -> Update {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Msg::Update(u)) => u,
            Ok(_) => panic!("not an update"),
            Err(e) => panic!("no update: {e}"),
        }
    }

    /// SPC-08: no update, no event, the same minute and the same size give
    /// no draw; each of them alone gives 1.
    #[test]
    fn spc_08_no_change_gives_no_draw() {
        let mut r = Redraw::default();
        assert_eq!(r.due(100, (80, 24), false, false), (true, true));
        assert_eq!(r.due(100, (80, 24), false, false), (false, false));
        assert_eq!(r.due(100, (80, 24), true, false), (true, false));
        assert_eq!(r.due(100, (80, 24), false, true), (true, false));
        assert_eq!(r.due(101, (80, 24), false, false), (true, false));
        assert_eq!(r.due(101, (100, 24), false, false), (true, true));
        assert_eq!(r.due(101, (100, 24), false, false), (false, false));
    }

    /// SPC-09: 1 project change gives 1 filter computation; 10 new snapshots
    /// with the same `generated_at` give 0 more; a new `generated_at` gives 1.
    #[test]
    fn spc_09_the_worker_caches_the_filtered_snapshot() {
        // Each snapshot differs (so each is an update), with 1 `generated_at`.
        let mut snaps: Vec<Snapshot> = (0..12).map(|i| snap("T1", i)).collect();
        snaps.push(snap("T2", 99));
        let mut feed = FakeFeed::new(snaps);
        let filters = feed.filters.clone();
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // Owned here: a failed assert drops it, and the worker ends.
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), fixed, req_rx, tx));
            let first = update(&rx);
            assert!(first.filtered.is_none());
            assert_eq!(first.mode, "collector");
            req_tx.send(Request::SetProject(Some("/p".into()))).unwrap();
            let u = update(&rx);
            assert_eq!(u.project.as_deref(), Some("/p"));
            assert!(u.filtered.is_some());
            assert_eq!(filters.load(Ordering::SeqCst), 1);
            for _ in 0..10 {
                req_tx.send(Request::Refresh).unwrap();
                let u = update(&rx);
                assert_eq!(u.snapshot.generated_at, "T1");
                assert!(u.filtered.is_some());
            }
            assert_eq!(filters.load(Ordering::SeqCst), 1);
            req_tx.send(Request::Refresh).unwrap();
            assert_eq!(update(&rx).snapshot.generated_at, "T1");
            req_tx.send(Request::Refresh).unwrap();
            assert_eq!(update(&rx).snapshot.generated_at, "T2");
            assert_eq!(filters.load(Ordering::SeqCst), 2);
            // The same project again is no change.
            req_tx.send(Request::SetProject(Some("/p".into()))).unwrap();
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert_eq!(filters.load(Ordering::SeqCst), 2);
        assert!(rx.try_recv().is_err(), "a stop sends nothing");
    }

    /// SPC-09: a new minute moves the window starts, so the filtered
    /// snapshot is computed again for the same `generated_at`, at the start
    /// of the new minute; within 1 minute it is not.
    #[test]
    fn spc_09_a_new_minute_recomputes_the_filtered_snapshot() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1)]);
        let filters = feed.filters.clone();
        let secs = Arc::new(AtomicI64::new(fixed().timestamp()));
        let clock_secs = secs.clone();
        let now = move || DateTime::from_timestamp(clock_secs.load(Ordering::SeqCst), 0).unwrap();
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // Owned here: a failed assert drops it, and the worker ends.
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), now, req_rx, tx));
            update(&rx);
            req_tx.send(Request::SetProject(Some("/p".into()))).unwrap();
            let u = update(&rx);
            let at = &u.filtered.unwrap().collector.started_at;
            assert_eq!(at, "2026-10-07T12:00:00Z");
            // 20 s later, the same minute: the refresh's update reuses the
            // cached filtered snapshot.
            secs.fetch_add(20, Ordering::SeqCst);
            req_tx.send(Request::Refresh).unwrap();
            let u = update(&rx);
            let at = &u.filtered.unwrap().collector.started_at;
            assert_eq!(at, "2026-10-07T12:00:00Z");
            assert_eq!(filters.load(Ordering::SeqCst), 1);
            // 20 s more: a new minute.
            secs.fetch_add(20, Ordering::SeqCst);
            req_tx.send(Request::Refresh).unwrap();
            let u = update(&rx);
            assert_eq!(u.snapshot.generated_at, "T1");
            let at = &u.filtered.unwrap().collector.started_at;
            assert_eq!(at, "2026-10-07T12:01:00Z");
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert_eq!(filters.load(Ordering::SeqCst), 2);
        assert!(rx.try_recv().is_err());
    }

    /// SPC-09 (R1): with a project set, the filtered rollup expires at the
    /// minute without a tick or a key: the worker wakes at the minute and
    /// sends the snapshot for the new minute. The tick here is 1 hour.
    #[test]
    fn spc_09_a_filtered_rollup_expires_at_the_minute_without_a_tick() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1)]);
        let ms = Arc::new(AtomicI64::new(
            clock::parse("2026-10-07T12:00:59Z")
                .unwrap()
                .timestamp_millis()
                + 900,
        ));
        let clock_ms = ms.clone();
        let now = move || DateTime::from_timestamp_millis(clock_ms.load(Ordering::SeqCst)).unwrap();
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), now, req_rx, tx));
            update(&rx);
            req_tx.send(Request::SetProject(Some("/p".into()))).unwrap();
            let u = update(&rx);
            assert_eq!(
                u.filtered.unwrap().collector.started_at,
                "2026-10-07T12:00:00Z"
            );
            ms.fetch_add(200, Ordering::SeqCst);
            let u = update(&rx);
            assert_eq!(
                u.filtered.unwrap().collector.started_at,
                "2026-10-07T12:01:00Z"
            );
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
    }

    /// Send `n` ticks' time to a worker with a short tick, then stop it;
    /// every message it sent.
    fn run_ticks(feed: &mut FakeFeed, ms: u64) -> Vec<Update> {
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            let req_tx = req_tx;
            let w = s.spawn(|| worker(feed, Duration::from_millis(5), fixed, req_rx, tx));
            std::thread::sleep(Duration::from_millis(ms));
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        rx.try_iter()
            .map(|m| match m {
                Msg::Update(u) => u,
                _ => panic!("not an update"),
            })
            .collect()
    }

    /// SPC-09: a snapshot that differs only in `generated_at` (a collector's
    /// tick with no new content) is no update.
    #[test]
    fn spc_09_a_new_generated_at_alone_is_no_update() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1), snap("T2", 1), snap("T3", 1)]);
        let sent = run_ticks(&mut feed, 200);
        assert!(feed.at >= 3, "{} ticks", feed.at);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].snapshot.generated_at, "T1");
    }

    /// SPC-09: a snapshot equal to the last one is no update.
    #[test]
    fn spc_09_the_worker_sends_no_update_for_an_equal_snapshot() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1)]);
        let sent = run_ticks(&mut feed, 200);
        assert!(feed.at >= 3, "{} ticks", feed.at);
        assert_eq!(sent.len(), 1);
    }

    /// SPC-09 (R1): a viewer that becomes the collector says so, also when
    /// its first snapshot differs from the last only in `generated_at`.
    #[test]
    fn spc_09_a_mode_change_is_an_update() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1), snap("T2", 1)]);
        feed.viewer_for = 3;
        let sent = run_ticks(&mut feed, 200);
        let modes: Vec<&str> = sent.iter().map(|u| u.mode).collect();
        assert_eq!(modes, ["viewer", "collector"]);
    }

    /// SPC-04 (Q1): `r` always gets an update, so the screen can clear its
    /// `refreshing…` status, also when nothing changed.
    #[test]
    fn spc_04_a_refresh_always_sends_an_update() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1)]);
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), fixed, req_rx, tx));
            update(&rx);
            for _ in 0..3 {
                req_tx.send(Request::Refresh).unwrap();
                assert_eq!(update(&rx).snapshot.generated_at, "T1");
            }
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert!(rx.try_recv().is_err());
    }

    /// A feed whose tick takes `busy`; it records when each tick started
    /// and how long it took.
    struct SlowFeed {
        busy: Duration,
        ticks: Arc<Mutex<Vec<(Instant, Duration)>>>,
    }

    impl Feed for SlowFeed {
        fn next(&mut self) -> Result<Option<Snapshot>> {
            let start = Instant::now();
            std::thread::sleep(self.busy);
            self.ticks.lock().unwrap().push((start, start.elapsed()));
            Ok(None)
        }
        fn filter(&mut self, snap: &Snapshot, _: &str, _: DateTime<Utc>) -> Snapshot {
            snap.clone()
        }
        fn mode(&self) -> &'static str {
            "collector"
        }
    }

    /// SPC-09 (R1): ticks start every `tick` from the last tick's start, not
    /// `tick` after its end. A 300 ms tick with a 400 ms interval starts
    /// every 400 ms, not every 700 ms.
    #[test]
    fn spc_09_the_tick_interval_counts_from_the_tick_start() {
        let tick = Duration::from_millis(400);
        let ticks: Arc<Mutex<Vec<(Instant, Duration)>>> = Arc::default();
        let mut feed = SlowFeed {
            busy: Duration::from_millis(300),
            ticks: ticks.clone(),
        };
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, _rx) = mpsc::channel();
        std::thread::scope(|s| {
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, tick, fixed, req_rx, tx));
            let until = Instant::now() + Duration::from_secs(20);
            while ticks.lock().unwrap().len() < 4 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        let ticks = ticks.lock().unwrap();
        assert!(ticks.len() >= 4, "{} ticks", ticks.len());
        for pair in ticks.windows(2) {
            let ((a, took), (b, _)) = (pair[0], pair[1]);
            // Late: `took + tick`. On time: `max(took, tick)`, and the
            // margin covers the scheduler.
            assert!(
                b - a < took + Duration::from_millis(300),
                "a tick of {took:?} was followed after {:?}",
                b - a
            );
        }
    }

    /// A feed whose first `fail` ticks fail with `boom`.
    struct FailingFeed {
        fail: usize,
        at: usize,
    }

    impl Feed for FailingFeed {
        fn next(&mut self) -> Result<Option<Snapshot>> {
            self.at += 1;
            if self.at <= self.fail {
                anyhow::bail!("boom");
            }
            Ok(Some(snap("T1", 1)))
        }
        fn filter(&mut self, snap: &Snapshot, _: &str, _: DateTime<Utc>) -> Snapshot {
            snap.clone()
        }
        fn mode(&self) -> &'static str {
            "collector"
        }
    }

    /// A tick error is an update once, and the next good tick clears it.
    #[test]
    fn spc_04_a_tick_error_shows_once_and_clears_on_the_next_good_tick() {
        let mut feed = FailingFeed { fail: 3, at: 0 };
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_millis(5), fixed, req_rx, tx));
            let first = update(&rx);
            assert_eq!(first.status.as_deref(), Some("boom"));
            let second = update(&rx);
            assert_eq!(second.status, None);
            assert_eq!(second.snapshot.generated_at, "T1");
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert!(rx.try_recv().is_err());
    }

    /// `recv` hands on a message, ends an empty wait with `Idle`, and reads
    /// a channel with no sender as `WorkerGone`.
    #[test]
    fn spc_04_recv_returns_a_message_idle_or_worker_gone() {
        let (tx, rx) = mpsc::channel();
        assert!(matches!(recv(&rx, Duration::from_millis(1)), Msg::Idle));
        tx.send(Msg::InputGone("why".into())).unwrap();
        match recv(&rx, Duration::from_secs(5)) {
            Msg::InputGone(why) => assert_eq!(why, "why"),
            _ => panic!("not the sent message"),
        }
        drop(tx);
        assert!(matches!(recv(&rx, Duration::from_secs(5)), Msg::WorkerGone));
    }

    /// One scripted terminal event, then silence.
    struct OneKey(Option<crossterm::event::Event>);

    impl Events for OneKey {
        fn poll(&mut self, _: Duration) -> std::io::Result<bool> {
            Ok(self.0.is_some())
        }
        fn read(&mut self) -> std::io::Result<crossterm::event::Event> {
            Ok(self.0.take().expect("polled before read"))
        }
    }

    /// `input` forwards a terminal event as `Msg::Input`, and stops when
    /// the UI is gone.
    #[test]
    fn spc_04_input_forwards_an_event() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let key = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let (tx, rx) = mpsc::channel();
        let stop = AtomicBool::new(false);
        std::thread::scope(|s| {
            let h = s.spawn(|| input(OneKey(Some(key.clone())), tx, &stop));
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Msg::Input(got)) => assert_eq!(got, key),
                _ => panic!("no input message"),
            }
            stop.store(true, Ordering::SeqCst);
            h.join().unwrap();
        });
    }

    /// Terminal events that fail at once, as when the terminal closes.
    struct Broken;

    impl Events for Broken {
        fn poll(&mut self, _: Duration) -> std::io::Result<bool> {
            Err(std::io::Error::other("the terminal is gone"))
        }
        fn read(&mut self) -> std::io::Result<crossterm::event::Event> {
            Err(std::io::Error::other("the terminal is gone"))
        }
    }

    /// SPC-04 (R1 high): an input error reaches the UI as `InputGone`, while
    /// the worker still runs and sends nothing; the UI stops, and `run`
    /// stops and joins the worker. Before the fix the input thread ended
    /// silently and the UI waited for ever.
    #[test]
    fn spc_04_an_input_error_ends_the_screen_and_stops_the_worker() {
        let mut feed = FakeFeed::new(vec![snap("T1", 1)]);
        let started = Instant::now();
        let mut got = None;
        let out = run(&mut feed, Duration::from_secs(3600), Broken, |rx, _| loop {
            match recv(rx, Duration::from_millis(50)) {
                Msg::InputGone(why) => {
                    got = Some(why);
                    return Ok(());
                }
                Msg::Shutdown | Msg::WorkerGone => anyhow::bail!("the wrong end"),
                _ if started.elapsed() > Duration::from_secs(10) => {
                    anyhow::bail!("the UI is stranded")
                }
                _ => {}
            }
        });
        out.unwrap();
        assert!(got.unwrap().contains("the terminal is gone"));
    }

    /// SPC-08 (R1): a synchronized update ends when a write inside it fails.
    #[test]
    fn spc_08_a_failed_draw_still_ends_the_synchronized_update() {
        fn failing(out: &mut Vec<u8>) -> std::io::Result<()> {
            let _sync = Synced::begin(out)?;
            Err(std::io::Error::other("draw failed"))
        }
        let mut out = Vec::new();
        assert!(failing(&mut out).is_err());
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "\x1b[?2026h\x1b[?2026l");
    }

    /// SPC-08 (Q1): a frame after a resize writes every cell inside 1
    /// synchronized update, with no clear; the next equal-size frame writes
    /// only the changed cells.
    #[test]
    fn spc_08_a_resize_writes_every_cell_and_no_clear() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        let area = Rect::new(0, 0, 10, 2);
        let mut a = Buffer::empty(area);
        a.set_string(0, 0, "hello", ratatui::style::Style::default());
        let mut bytes = Vec::new();
        write_frame(
            &mut ratatui::backend::CrosstermBackend::new(&mut bytes),
            None,
            &a,
        )
        .unwrap();
        let first = String::from_utf8(bytes).unwrap();
        assert!(first.starts_with("\x1b[?2026h") && first.ends_with("\x1b[?2026l"));
        assert!(!first.contains("\x1b[2J"), "{first:?}");
        assert!(first.contains("hello"));
        // 20 cells: 5 letters and 15 spaces.
        assert_eq!(first.matches(' ').count(), 15, "{first:?}");
        let mut b = a.clone();
        b.set_string(0, 1, "x", ratatui::style::Style::default());
        let mut bytes = Vec::new();
        write_frame(
            &mut ratatui::backend::CrosstermBackend::new(&mut bytes),
            Some(&a),
            &b,
        )
        .unwrap();
        let next = String::from_utf8(bytes).unwrap();
        assert!(next.contains('x') && !next.contains("hello"), "{next:?}");
    }

    /// SPC-07 (Q1): the mouse reports buttons and the wheel (1000, SGR
    /// 1006), never every pointer move (1003, 1002).
    #[test]
    fn spc_07_the_mouse_reports_buttons_and_the_wheel_only() {
        assert_eq!(MOUSE_ON, "\x1b[?1000h\x1b[?1006h");
        assert_eq!(MOUSE_OFF, "\x1b[?1006l\x1b[?1000l");
        for mode in ["1002", "1003", "1015"] {
            assert!(!MOUSE_ON.contains(mode));
        }
    }

    /// SPC-04 (R1): while the filtered snapshot for the view's project is not
    /// there, the screen shows the global snapshot as loading, never as the
    /// project's.
    #[test]
    fn spc_04_a_new_project_loads_before_its_rollups_show() {
        let global = Arc::new(snap("T1", 1));
        let filtered = Arc::new(snap("T1", 2));
        let view = |p: Option<&str>| ViewState {
            project: p.map(String::from),
            ..ViewState::default()
        };
        let u = Update {
            snapshot: global.clone(),
            filtered: Some(filtered),
            project: Some("/a".into()),
            status: None,
            mode: "collector",
        };
        assert_eq!(shown(&u, &view(None)), (&*global, false));
        let (s, loading) = shown(&u, &view(Some("/a")));
        assert_eq!((s.projects, loading), (2, false));
        assert_eq!(shown(&u, &view(Some("/b"))), (&*global, true));
        let none = Update {
            filtered: None,
            project: None,
            ..u
        };
        assert_eq!(shown(&none, &view(Some("/a"))), (&*global, true));
    }

    /// SPC-09: in collector mode a project change reads no event file: the
    /// filter folds the collector's memory. The event files are gone before
    /// the filter runs, and the project's rollup still holds its event.
    #[test]
    fn spc_09_a_collector_filters_a_project_without_reading_event_files() {
        use horch_core::telemetry::{Event, TokenClasses};
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HORCH_STATE_DIR", &state.to_string_lossy()),
        )
        .unwrap();
        let now = clock::now();
        let tel = horch_core::telemetry::dir(&state);
        std::fs::create_dir_all(&tel).unwrap();
        let event = |project: &str, id: &str| Event {
            ts: clock::stamp(now - chrono::Duration::minutes(30)),
            project: Some(project.into()),
            record_id: format!("r-{id}"),
            session_id: format!("s-{id}"),
            event_id: id.into(),
            role: "opus-1".into(),
            teammate: "opus".into(),
            via: None,
            kind: "worker".into(),
            agent: "claude".into(),
            model: "claude-opus-5-5".into(),
            effort: None,
            phase: None,
            plan: None,
            subagent: false,
            delta: false,
            tool_nested: false,
            idle: false,
            tokens: TokenClasses {
                input: 1000,
                ..TokenClasses::default()
            },
            cost_usd: Some(1.0),
            harness_cost: None,
        };
        let lines: Vec<String> = [event("/p", "e1"), event("/q", "e2")]
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        let file = tel.join(format!("events-{}.jsonl", &clock::stamp(now)[..10]));
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        let home = tmp.path().join("home");
        let loc = Locations {
            home: home.clone(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join("opencode.db"),
            sqlite3: PathBuf::from("sqlite3"),
        };
        let collector = Collector::open_at(&state, loc, Probing::Never, now).unwrap();
        let held = lock::acquire(&state, &lock::this_process(&clock::now_stamp(), &ctx))
            .unwrap()
            .unwrap();
        let mut feed = LiveFeed::new(
            &ctx,
            &state,
            Source::Collector {
                collector: Box::new(collector),
                _lock: held,
            },
        );
        std::fs::remove_file(&file).unwrap();
        let out = feed.filter(&Snapshot::default(), "/p", now);
        let rows = &out.rollups["5h"].by_project;
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["/p"]);
        assert_eq!(rows[0].tokens.input, 1000);
    }

    /// SPC-09: a viewer reads `snapshot.json` only when its modification
    /// time or length changed.
    #[test]
    fn spc_09_a_viewer_reads_the_snapshot_only_when_it_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HORCH_STATE_DIR", &tmp.path().to_string_lossy()),
        )
        .unwrap();
        let path = collect::snapshot_path(tmp.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&snap("T1", 1)).unwrap()).unwrap();
        let mut feed = LiveFeed::new(&ctx, tmp.path(), Source::Viewer);
        // Hold the lock as a live collector would, so the viewer stays one.
        let me = lock::this_process(&clock::now_stamp(), &ctx);
        let _held = lock::acquire(tmp.path(), &me).unwrap().unwrap();
        assert_eq!(feed.next().unwrap().unwrap().generated_at, "T1");
        assert!(feed.next().unwrap().is_none());
        std::fs::write(&path, serde_json::to_string(&snap("T22", 1)).unwrap()).unwrap();
        assert_eq!(feed.next().unwrap().unwrap().generated_at, "T22");
        assert!(feed.next().unwrap().is_none());
        assert_eq!(feed.mode(), "viewer");
    }
}
