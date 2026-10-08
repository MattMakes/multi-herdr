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
use horch_core::clock;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, Probing, Snapshot};
use horch_core::telemetry::lock;
use horch_core::telemetry::store;
use horch_core::usage::Locations;

use super::view::{
    draw as draw_frame, draw_buf, on_key, on_wheel, table_at, Action, Rows, ViewState,
};
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

/// What the worker sends after a tick or a project change.
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
    /// SIGINT or SIGTERM.
    Shutdown,
    /// The worker ended without a `Stop` (a panic).
    WorkerGone,
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

/// (generated_at, project, minute) and the filtered snapshot for them.
type FilterCache = ((String, String, i64), Arc<Snapshot>);

/// The worker loop: a tick every `tick`, and at once on [`Request::Refresh`].
/// A snapshot that differs from the last one only in `generated_at` is no
/// update. The filtered snapshot is computed only when the snapshot's
/// `generated_at`, the project, or the minute of `now()` changes: a new
/// minute moves the window starts. Returns on [`Request::Stop`] or when the
/// UI is gone.
pub fn worker<F: Feed>(
    feed: &mut F,
    tick: Duration,
    now: impl Fn() -> DateTime<Utc>,
    rx: Receiver<Request>,
    tx: Sender<Msg>,
) {
    let mut guard = GoneGuard(&tx, false);
    let mut snap: Arc<Snapshot> = Arc::default();
    let mut status: Option<String> = None;
    let mut project: Option<String> = None;
    let mut cache: Option<FilterCache> = None;
    let mut first = true;
    let mut due = Instant::now();
    let mut project_changed = false;
    loop {
        let mut changed = std::mem::take(&mut first);
        if Instant::now() >= due {
            match feed.next() {
                Ok(Some(mut s)) => {
                    let at = std::mem::replace(&mut s.generated_at, snap.generated_at.clone());
                    // The first tick replaces the empty start snapshot.
                    if changed || *snap != s {
                        s.generated_at = at;
                        snap = Arc::new(s);
                        changed = true;
                    }
                    changed |= status.take().is_some();
                }
                Ok(None) => changed |= status.take().is_some(),
                Err(e) => {
                    let s = Some(format!("{e:#}"));
                    if s != status {
                        status = s;
                        changed = true;
                    }
                }
            }
            due = Instant::now() + tick;
        }
        let minute = now().timestamp().div_euclid(60);
        let key = |p: &String| (snap.generated_at.clone(), p.clone(), minute);
        let cached = |cache: &Option<FilterCache>, p: &String| {
            cache
                .as_ref()
                .filter(|c| c.0 == key(p))
                .map(|c| c.1.clone())
        };
        let stale = project
            .as_ref()
            .is_some_and(|p| cached(&cache, p).is_none());
        if changed || std::mem::take(&mut project_changed) || stale {
            let filtered = project.as_ref().map(|p| match cached(&cache, p) {
                Some(f) => f,
                None => {
                    let at = DateTime::from_timestamp(minute * 60, 0).unwrap_or_else(&now);
                    let f = Arc::new(feed.filter(&snap, p, at));
                    cache = Some((key(p), f.clone()));
                    f
                }
            });
            let update = Update {
                snapshot: snap.clone(),
                filtered,
                project: project.clone(),
                status: status.clone(),
                mode: feed.mode(),
            };
            if tx.send(Msg::Update(update)).is_err() {
                break;
            }
        }
        match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(Request::Refresh) => due = Instant::now(),
            Ok(Request::SetProject(p)) => {
                if p != project {
                    project = p;
                    project_changed = true;
                }
            }
            Ok(Request::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
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

impl Feed for LiveFeed<'_> {
    fn next(&mut self) -> Result<Option<Snapshot>> {
        let (ctx, root) = (self.ctx, self.root.as_path());
        // A viewer whose collector died takes over (the next `ensure` would).
        if matches!(self.source, Source::Viewer) && !lock::collector_live(root) {
            if let Ok(Ok(held)) = lock::acquire(root, &lock::this_process(&clock::now_stamp(), ctx))
            {
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
        }
        match &mut self.source {
            Source::Collector { collector, .. } => collector.tick(clock::now()).map(Some),
            Source::Viewer => {
                let path = collect::snapshot_path(root);
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
    }

    fn filter(&mut self, snap: &Snapshot, project: &str, now: DateTime<Utc>) -> Snapshot {
        filtered(snap, project, &self.root, now)
    }

    fn mode(&self) -> &'static str {
        match self.source {
            Source::Collector { .. } => "collector",
            Source::Viewer => "viewer",
        }
    }
}

/// The snapshot with its rollups recomputed for one project, from the event
/// files, for the windows at `now`. Runs on the worker thread only.
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

/// Forwards terminal events, and a shutdown signal, until `stop` is set.
fn input(tx: Sender<Msg>, stop: &AtomicBool) {
    use crossterm::event;
    while !stop.load(Ordering::SeqCst) {
        if SHUTDOWN.load(Ordering::SeqCst) {
            let _ = tx.send(Msg::Shutdown);
            return;
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => match event::read() {
                Ok(ev) => {
                    if tx.send(Msg::Input(ev)).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            },
            Ok(false) => {}
            Err(_) => return,
        }
    }
}

/// Run the screen: the worker and input threads, and `ui` on this thread
/// with the receiving end. `ui` returns when the operator quits; then the
/// worker is stopped and joined, so the lock is released before return.
pub fn run<F, U>(feed: &mut F, tick: Duration, ui: U) -> Result<()>
where
    F: Feed + Send,
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
            .spawn_scoped(s, move || input(itx, stop))?;
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

/// Restores the terminal however the screen loop ends, also after a panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

/// When the loop draws (SPC-08): a new update, a change of the view, a
/// resize, or a new minute on the header clock. Nothing else draws.
#[derive(Debug, Default)]
pub struct Redraw {
    minute: Option<i64>,
    size: Option<(u16, u16)>,
}

impl Redraw {
    /// Whether to draw now, and whether the terminal was resized (then the
    /// screen is cleared first).
    pub fn due(&mut self, minute: i64, size: (u16, u16), update: bool, view: bool) -> (bool, bool) {
        let resized = self.size != Some(size);
        let new_minute = self.minute != Some(minute);
        self.size = Some(size);
        self.minute = Some(minute);
        (update || view || resized || new_minute, resized)
    }
}

/// The snapshot to show: the filtered one when it is for the view's project.
fn shown<'a>(u: &'a Update, view: &ViewState) -> &'a Snapshot {
    match &u.filtered {
        Some(f) if view.project.is_some() && u.project == view.project => f,
        _ => &u.snapshot,
    }
}

/// The screen loop on a terminal. It reads no file and runs no tick: the
/// worker does (SPC-09). It writes to the terminal only when the frame
/// changed, and only the changed cells (SPC-08).
pub fn screen<F: Feed + Send>(feed: &mut F, tick: Duration) -> Result<()> {
    use crossterm::event::{Event, MouseEventKind};
    use ratatui::backend::CrosstermBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    crossterm::terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Hide
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut view = ViewState {
        color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        status: Some("waiting for the first tick".into()),
        ..ViewState::default()
    };
    run(feed, tick, |rx, req| {
        let empty = Update {
            snapshot: Arc::default(),
            filtered: None,
            project: None,
            status: None,
            mode: "",
        };
        let mut last: Option<Update> = None;
        let mut redraw = Redraw::default();
        let mut prev: Option<Buffer> = None;
        let (mut update, mut changed) = (true, true);
        loop {
            let now = clock::now();
            let size = terminal.size()?;
            let area = Rect::new(0, 0, size.width, size.height);
            let u = last.as_ref().unwrap_or(&empty);
            let (draw, clear) = redraw.due(
                now.timestamp().div_euclid(60),
                (size.width, size.height),
                std::mem::take(&mut update),
                std::mem::take(&mut changed),
            );
            if draw {
                let snap = shown(u, &view);
                view.sync(&Rows::of(snap, &view, area));
                let mut buf = Buffer::empty(area);
                draw_buf(&mut buf, area, snap, &view, now);
                if clear {
                    terminal.clear()?;
                    prev = None;
                }
                // An equal frame writes nothing, not even the sync marks.
                if prev.as_ref() != Some(&buf) {
                    crossterm::execute!(
                        std::io::stdout(),
                        crossterm::terminal::BeginSynchronizedUpdate
                    )?;
                    terminal.draw(|f| draw_frame(f, snap, &view, now))?;
                    crossterm::execute!(
                        std::io::stdout(),
                        crossterm::terminal::EndSynchronizedUpdate
                    )?;
                    prev = Some(buf);
                }
            }
            // Wake at the next minute boundary for the header clock.
            let wait =
                Duration::from_millis(60_000 - now.timestamp_millis().rem_euclid(60_000) as u64);
            let msg = match rx.recv_timeout(wait) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
            let u = last.as_ref().unwrap_or(&empty);
            let action = match msg {
                Msg::Update(u) => {
                    view.status = u.status.clone();
                    view.mode = Some(u.mode);
                    last = Some(u);
                    update = true;
                    Action::None
                }
                Msg::Input(Event::Key(k)) => {
                    let rows = Rows::of(shown(u, &view), &view, area);
                    let (next, action) = on_key(view.clone(), k, &rows);
                    changed = next != view;
                    view = next;
                    action
                }
                Msg::Input(Event::Mouse(m)) => {
                    let down = match m.kind {
                        MouseEventKind::ScrollDown => true,
                        MouseEventKind::ScrollUp => false,
                        _ => continue,
                    };
                    let snap = shown(u, &view);
                    let Some(t) = table_at(snap, &view, area, m.column, m.row) else {
                        continue;
                    };
                    let rows = Rows::of(snap, &view, area);
                    let (next, action) = on_wheel(view.clone(), t, down, &rows);
                    changed = next != view;
                    view = next;
                    action
                }
                Msg::Input(_) => Action::None,
                Msg::Shutdown | Msg::WorkerGone => return Ok(()),
            };
            match action {
                Action::Quit => return Ok(()),
                Action::Refresh => {
                    let _ = req.send(Request::Refresh);
                }
                Action::SetProject(p) => {
                    let _ = req.send(Request::SetProject(p));
                }
                Action::None | Action::Redraw => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A feed that returns `snaps` in turn, then the last one again, and
    /// counts its filter calls.
    struct FakeFeed {
        snaps: Vec<Snapshot>,
        at: usize,
        filters: Arc<AtomicUsize>,
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
            "collector"
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

    /// SPC-09: 1 project change gives 1 filter computation; 10 new snapshots
    /// with the same `generated_at` give 0 more; a new `generated_at` gives 1.
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

    #[test]
    fn spc_09_the_worker_caches_the_filtered_snapshot() {
        let filters = Arc::new(AtomicUsize::new(0));
        // Each snapshot differs (so each is an update), with 1 `generated_at`.
        let mut snaps: Vec<Snapshot> = (0..12).map(|i| snap("T1", i)).collect();
        snaps.push(snap("T2", 99));
        let mut feed = FakeFeed {
            snaps,
            at: 0,
            filters: filters.clone(),
        };
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
        let filters = Arc::new(AtomicUsize::new(0));
        let mut feed = FakeFeed {
            snaps: vec![snap("T1", 1)],
            at: 0,
            filters: filters.clone(),
        };
        let secs = Arc::new(std::sync::atomic::AtomicI64::new(fixed().timestamp()));
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
            // 20 s later, the same minute: no new filter and no update.
            secs.fetch_add(20, Ordering::SeqCst);
            req_tx.send(Request::Refresh).unwrap();
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

    /// SPC-09: a snapshot that differs only in `generated_at` (a collector's
    /// tick with no new content) is no update.
    #[test]
    fn spc_09_a_new_generated_at_alone_is_no_update() {
        let mut feed = FakeFeed {
            snaps: vec![snap("T1", 1), snap("T2", 1), snap("T3", 1)],
            at: 0,
            filters: Arc::default(),
        };
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // Owned here: a failed assert drops it, and the worker ends.
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), fixed, req_rx, tx));
            assert_eq!(update(&rx).snapshot.generated_at, "T1");
            req_tx.send(Request::Refresh).unwrap();
            req_tx.send(Request::Refresh).unwrap();
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert!(rx.try_recv().is_err());
    }

    /// SPC-09: a snapshot equal to the last one is no update.
    #[test]
    fn spc_09_the_worker_sends_no_update_for_an_equal_snapshot() {
        let mut feed = FakeFeed {
            snaps: vec![snap("T1", 1)],
            at: 0,
            filters: Arc::default(),
        };
        let (req_tx, req_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // Owned here: a failed assert drops it, and the worker ends.
            let req_tx = req_tx;
            let w = s.spawn(|| worker(&mut feed, Duration::from_secs(3600), fixed, req_rx, tx));
            update(&rx);
            for _ in 0..5 {
                req_tx.send(Request::Refresh).unwrap();
            }
            req_tx.send(Request::Stop).unwrap();
            w.join().unwrap();
        });
        assert!(rx.try_recv().is_err());
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
