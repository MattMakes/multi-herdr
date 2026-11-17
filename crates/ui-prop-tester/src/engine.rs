use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cli::{ReproduceArgs, RunArgs};
use crate::driver::{
    Action, BrowserConfig, Driver, DriverError, DriverFactory, DriverKind, ExtractOutcome,
    Observation, SpecSetup, TerminalConfig,
};
use crate::output_path::{check_distinct_from_input, check_output_path};
use crate::rng::{seed_from_clock, SplitMix64};
use crate::spec::{SpecRuntime, Specification};
use crate::trace::{
    read_trace_file, write_trace_file, Trace, TraceHeader, TraceState, TraceSummary, Violation,
};
use crate::{Io, EXIT_ERROR, EXIT_OK, EXIT_VIOLATION};

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Browser(BrowserConfig),
    Terminal(TerminalConfig),
}

impl Target {
    pub fn kind(&self) -> DriverKind {
        match self {
            Target::Browser(_) => DriverKind::Browser,
            Target::Terminal(_) => DriverKind::Terminal,
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Target::Browser(config) => serde_json::to_value(config),
            Target::Terminal(config) => serde_json::to_value(config),
        }
        .unwrap_or(Value::Null)
    }

    pub fn from_header(kind: DriverKind, target: &Value) -> Result<Self, String> {
        let parsed = match kind {
            DriverKind::Browser => serde_json::from_value(target.clone()).map(Target::Browser),
            DriverKind::Terminal => serde_json::from_value(target.clone()).map(Target::Terminal),
        };
        parsed.map_err(|error| format!("the trace header does not describe its target: {error}"))
    }

    pub fn open(&self, drivers: &dyn DriverFactory) -> Result<Box<dyn Driver>, DriverError> {
        match self {
            Target::Browser(config) => drivers.browser(config),
            Target::Terminal(config) => drivers.terminal(config),
        }
    }
}

pub fn kind_name(kind: DriverKind) -> &'static str {
    match kind {
        DriverKind::Browser => "browser",
        DriverKind::Terminal => "terminal",
    }
}

pub struct RunPlan<'a> {
    pub target: &'a Target,
    pub specification: Option<&'a Specification>,
    pub seed: u64,
    pub time_limit: Option<Duration>,
    pub hard_deadline: Option<Instant>,
    pub actions_max: Option<u64>,
    pub exit_on_violation: bool,
    pub stop_on_property: Option<&'a str>,
    pub replay: Option<&'a [Action]>,
    pub swarm: bool,
    pub stop: Option<&'a AtomicBool>,
}

impl<'a> RunPlan<'a> {
    pub fn new(target: &'a Target, specification: Option<&'a Specification>, seed: u64) -> Self {
        Self {
            target,
            specification,
            seed,
            time_limit: None,
            hard_deadline: None,
            actions_max: None,
            exit_on_violation: false,
            stop_on_property: None,
            replay: None,
            swarm: false,
            stop: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    TargetEnded,
    Violation,
    TimeLimit,
    ActionLimit,
    ReplayDone,
    Interrupted,
    Stopped,
}

impl Ending {
    pub fn describe(self) -> &'static str {
        match self {
            Ending::TargetEnded => "the target ended",
            Ending::Violation => "stopped at the first violation",
            Ending::TimeLimit => "the time limit was reached",
            Ending::ActionLimit => "the action limit was reached",
            Ending::ReplayDone => "every recorded action was replayed",
            Ending::Interrupted => "interrupted",
            Ending::Stopped => "the campaign stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    Error(String),
    Divergence(String),
    NoActions(u64),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Error(message) | Failure::Divergence(message) => message.clone(),
            Failure::NoActions(index) => format!("no actions are available in state {index}"),
        }
    }
}

pub struct RunReport {
    pub trace: Trace,
    pub ending: Option<Ending>,
    pub failure: Option<Failure>,
    pub violations: Vec<Violation>,
    pub actions: u64,
    pub categories: Vec<String>,
    pub enabled: Vec<String>,
}

impl RunReport {
    pub fn exit_code(&self) -> i32 {
        if self.failure.is_some() {
            EXIT_ERROR
        } else if !self.violations.is_empty() {
            EXIT_VIOLATION
        } else {
            EXIT_OK
        }
    }

    pub fn violated(&self, property: &str) -> bool {
        self.violations
            .iter()
            .any(|violation| violation.property == property)
    }
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

pub fn reset_interrupt() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}

pub fn raise_interrupt() {
    #[cfg(unix)]
    if HANDLER_INSTALLED.load(Ordering::SeqCst) {
        unsafe {
            libc::raise(libc::SIGINT);
        }
        return;
    }
    INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
extern "C" fn on_interrupt(_signal: libc::c_int) {
    if INTERRUPTED.swap(true, Ordering::SeqCst) {
        unsafe {
            libc::_exit(130);
        }
    }
}

pub fn install_interrupt_handler() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        #[cfg(unix)]
        unsafe {
            let handler: extern "C" fn(libc::c_int) = on_interrupt;
            libc::signal(libc::SIGINT, handler as libc::sighandler_t);
        }
        HANDLER_INSTALLED.store(cfg!(unix), Ordering::SeqCst);
    });
}

pub fn swarm_subset(categories: &[String], seed: u64) -> Vec<String> {
    if categories.is_empty() {
        return Vec::new();
    }
    let mut rng = SplitMix64::new(seed ^ 0x5357_4152_4D5F_5345);
    let mut picked: Vec<String> = categories
        .iter()
        .filter(|_| rng.next_u64() & 1 == 1)
        .cloned()
        .collect();
    if picked.is_empty() {
        picked.push(categories[rng.below(categories.len() as u64) as usize].clone());
    }
    picked
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub fn execute(plan: &RunPlan<'_>, drivers: &dyn DriverFactory) -> RunReport {
    let header = TraceHeader {
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        driver: plan.target.kind(),
        seed: plan.seed,
        target: plan.target.to_json(),
        specification: plan.specification.map(Specification::record),
        started_unix_ms: unix_ms(),
        coverage_symbols: None,
    };
    let mut report = RunReport {
        trace: Trace {
            header,
            states: Vec::new(),
            summary: None,
        },
        ending: None,
        failure: None,
        violations: Vec::new(),
        actions: 0,
        categories: Vec::new(),
        enabled: Vec::new(),
    };
    match SpecRuntime::load(plan.target.kind(), plan.specification, plan.seed) {
        Ok(mut runtime) => {
            report.categories = runtime.categories.clone();
            report.enabled = if plan.swarm {
                swarm_subset(&runtime.categories, plan.seed)
            } else {
                runtime.categories.clone()
            };
            match plan.target.open(drivers) {
                Ok(mut driver) => {
                    let enabled = report.enabled.clone();
                    let outcome = drive(&mut runtime, driver.as_mut(), plan, &enabled, &mut report);
                    match outcome {
                        Ok(ending) => {
                            report.ending = Some(ending);
                            let stopped_on_violation = ending == Ending::Violation;
                            if !stopped_on_violation {
                                if let Err(failure) = resolve_end(&mut runtime, &mut report) {
                                    report.failure = Some(failure);
                                }
                            }
                        }
                        Err(failure) => report.failure = Some(failure),
                    }
                    report.trace.header.coverage_symbols = driver.coverage_symbols();
                    if let Err(error) = driver.shutdown() {
                        if report.failure.is_none() {
                            report.failure = Some(Failure::Error(format!(
                                "the driver did not shut down cleanly: {error}"
                            )));
                        }
                    }
                }
                Err(error) => report.failure = Some(Failure::Error(error.to_string())),
            }
        }
        Err(message) => report.failure = Some(Failure::Error(message)),
    }
    let outcome = if report.failure.is_some() {
        "error"
    } else if report.violations.is_empty() {
        "passed"
    } else {
        "violation"
    };
    report.trace.summary = Some(TraceSummary {
        outcome: outcome.to_string(),
        states: report.trace.states.len() as u64,
        violations: report.violations.clone(),
        message: match (&report.failure, report.ending) {
            (Some(failure), _) => Some(failure.message()),
            (None, Some(ending)) => Some(ending.describe().to_string()),
            (None, None) => None,
        },
    });
    report
}

fn resolve_end(runtime: &mut SpecRuntime, report: &mut RunReport) -> Result<(), Failure> {
    let Some(last) = report.trace.states.last_mut() else {
        return Ok(());
    };
    let violations = runtime
        .finish(last.index, last.timestamp_ms)
        .map_err(Failure::Error)?;
    last.violations.extend(violations.iter().cloned());
    report.violations.extend(violations);
    Ok(())
}

fn driver_failure(error: DriverError) -> Failure {
    Failure::Error(error.to_string())
}

fn drive(
    runtime: &mut SpecRuntime,
    driver: &mut dyn Driver,
    plan: &RunPlan<'_>,
    enabled: &[String],
    report: &mut RunReport,
) -> Result<Ending, Failure> {
    driver
        .prepare(&SpecSetup {
            custom_actions: runtime.custom_actions.clone(),
        })
        .map_err(driver_failure)?;
    let mut observation = driver.start().map_err(driver_failure)?;
    let started = Instant::now();
    let deadline = plan.time_limit.map(|limit| started + limit);
    let mut action: Option<Action> = None;
    let mut replayed = 0usize;
    loop {
        let index = report.trace.states.len() as u64;
        fill_cells(runtime, driver, &observation)?;
        let violations = runtime
            .step(index, observation.timestamp_ms)
            .map_err(Failure::Error)?;
        let cells = runtime.exported_cells().map_err(Failure::Error)?;
        report.violations.extend(violations.iter().cloned());
        let ended = observation.ended;
        let timestamp_ms = observation.timestamp_ms;
        report.trace.states.push(TraceState {
            index,
            timestamp_ms,
            action: action.take(),
            state: std::mem::take(&mut observation.state),
            cells,
            violations: violations.clone(),
            screenshot: observation.screenshot.take(),
        });
        if ended {
            return Ok(Ending::TargetEnded);
        }
        if plan.exit_on_violation && !violations.is_empty() {
            return Ok(Ending::Violation);
        }
        if let Some(property) = plan.stop_on_property {
            if violations
                .iter()
                .any(|violation| violation.property == property)
            {
                return Ok(Ending::Violation);
            }
        }
        if interrupted() {
            return Ok(Ending::Interrupted);
        }
        if plan.stop.is_some_and(|stop| stop.load(Ordering::SeqCst)) {
            return Ok(Ending::Stopped);
        }
        let now = Instant::now();
        if deadline.is_some_and(|deadline| now >= deadline)
            || plan.hard_deadline.is_some_and(|deadline| now >= deadline)
        {
            return Ok(Ending::TimeLimit);
        }
        if plan
            .actions_max
            .is_some_and(|limit| report.actions >= limit)
        {
            return Ok(Ending::ActionLimit);
        }
        let next = match plan.replay {
            Some(recorded) => {
                let Some(wanted) = recorded.get(replayed) else {
                    return Ok(Ending::ReplayDone);
                };
                replayed += 1;
                let offered = runtime.offered().map_err(Failure::Error)?;
                match driver.match_recorded(wanted, &offered) {
                    Some(matched) => matched,
                    None => {
                        return Err(Failure::Divergence(format!(
                            "the replay diverged in state {index}: the recorded action {wanted} is not offered by the generators"
                        )))
                    }
                }
            }
            None => runtime
                .sample(enabled)
                .map_err(Failure::Error)?
                .ok_or(Failure::NoActions(index))?,
        };
        let next = attach_cells(next, runtime)?;
        observation = driver.apply(&next).map_err(driver_failure)?;
        report.actions += 1;
        action = Some(next);
    }
}

fn attach_cells(mut action: Action, runtime: &mut SpecRuntime) -> Result<Action, Failure> {
    if action.get("kind").and_then(Value::as_str) == Some("custom") {
        let cells = runtime.exported_cells().map_err(Failure::Error)?;
        if let Value::Object(fields) = &mut action {
            fields.insert("cells".into(), cells);
        }
    }
    Ok(action)
}

fn fill_cells(
    runtime: &mut SpecRuntime,
    driver: &mut dyn Driver,
    observation: &Observation,
) -> Result<(), Failure> {
    if !driver.extracts_remotely() {
        return runtime
            .extract_local(&observation.state)
            .map_err(Failure::Error);
    }
    let outcomes = driver
        .extract(&runtime.extractors)
        .map_err(driver_failure)?;
    if outcomes.len() != runtime.extractors.len() {
        return Err(Failure::Error(format!(
            "the driver returned {} extractor results for {} extractors",
            outcomes.len(),
            runtime.extractors.len()
        )));
    }
    let mut values = Vec::with_capacity(outcomes.len());
    for (position, outcome) in outcomes.into_iter().enumerate() {
        match outcome {
            ExtractOutcome::Value(value) => values.push(value),
            ExtractOutcome::Thrown { message, stack } => {
                let name = runtime
                    .extractor_names
                    .get(position)
                    .cloned()
                    .unwrap_or_else(|| format!("cell#{position}"));
                return Err(Failure::Error(format!(
                    "the extractor {name} threw {message}\n{stack}"
                )));
            }
        }
    }
    runtime.set_cell_values(&values).map_err(Failure::Error)
}

pub fn fail(io: &mut Io<'_>, message: &str) -> i32 {
    let _ = writeln!(io.err, "ui-prop-tester: error: {message}");
    EXIT_ERROR
}

pub fn default_trace_path(seed: u64, label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ui-prop-tester-{seed}-{}{label}.trace", unix_ms()))
}

pub fn write_report(trace: &Trace, path: &Path) -> Result<(), String> {
    write_trace_file(path, trace)
        .map_err(|error| format!("cannot write the trace {}: {error}", path.display()))
}

pub fn print_violations(io: &mut Io<'_>, violations: &[Violation], indent: &str) {
    for violation in violations {
        let message = violation
            .message
            .as_deref()
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        let _ = writeln!(
            io.out,
            "{indent}violation of {} at state {} ({}){message}",
            violation.property, violation.state_index, violation.path
        );
        let cells = serde_json::to_string(&violation.cells).unwrap_or_default();
        if cells != "{}" {
            let _ = writeln!(io.out, "{indent}  cells: {}", shortened(&cells));
        }
    }
}

pub fn test(
    target: Target,
    run: &RunArgs,
    reproduce: &ReproduceArgs,
    io: &mut Io<'_>,
    drivers: &dyn DriverFactory,
) -> i32 {
    install_interrupt_handler();
    reset_interrupt();
    let recorded = match &reproduce.reproduce {
        Some(path) => match read_trace_file(path) {
            Ok(trace) => Some(trace),
            Err(error) => {
                return fail(
                    io,
                    &format!("cannot read the trace {}: {error}", path.display()),
                )
            }
        },
        None => None,
    };
    if let Some(trace) = &recorded {
        if trace.header.driver != target.kind() {
            return fail(
                io,
                &format!(
                    "the trace was recorded with the {} driver, not the {} driver",
                    kind_name(trace.header.driver),
                    kind_name(target.kind())
                ),
            );
        }
    }
    let specification = match (&run.specification, &recorded) {
        (Some(path), _) => match Specification::from_file(path) {
            Ok(spec) => Some(spec),
            Err(message) => return fail(io, &message),
        },
        (None, Some(trace)) => trace
            .header
            .specification
            .as_ref()
            .map(Specification::from_record),
        (None, None) => None,
    };
    let seed = run
        .seed
        .or(recorded.as_ref().map(|trace| trace.header.seed))
        .unwrap_or_else(seed_from_clock);
    let output = match &run.output_path {
        Some(path) => {
            if let Some(input) = &reproduce.reproduce {
                if let Err(message) = check_distinct_from_input(path, input) {
                    return fail(io, &message);
                }
            }
            if let Err(message) = check_output_path(path, run.output_path_overwrite) {
                return fail(io, &message);
            }
            path.clone()
        }
        None => default_trace_path(seed, ""),
    };
    let actions = recorded.as_ref().map(Trace::actions);
    let mut plan = RunPlan::new(&target, specification.as_ref(), seed);
    plan.time_limit = run.time_limit;
    plan.actions_max = run.actions_max;
    plan.exit_on_violation = run.exit_on_violation;
    plan.replay = actions.as_deref();
    let _ = writeln!(io.out, "ui-prop-tester: seed {seed}");
    let report = execute(&plan, drivers);
    let written = write_report(&report.trace, &output);
    print_summary(io, &report);
    if let Some(failure) = &report.failure {
        fail(io, &failure.message());
    }
    match written {
        Ok(()) => {
            let _ = writeln!(io.out, "ui-prop-tester: trace {}", output.display());
        }
        Err(message) => return fail(io, &message),
    }
    report.exit_code()
}

fn print_summary(io: &mut Io<'_>, report: &RunReport) {
    let states = report.trace.states.len();
    let ending = report
        .ending
        .map(|ending| format!("; {}", ending.describe()))
        .unwrap_or_default();
    let _ = writeln!(
        io.out,
        "ui-prop-tester: {states} states, {} actions{ending}",
        report.actions
    );
    if report.violations.is_empty() {
        if report.failure.is_none() {
            let _ = writeln!(io.out, "ui-prop-tester: no violations");
        }
    } else {
        print_violations(io, &report.violations, "ui-prop-tester: ");
    }
}

const SUMMARY_CELLS_MAX: usize = 600;

fn shortened(cells: &str) -> String {
    let total = cells.chars().count();
    if total <= SUMMARY_CELLS_MAX {
        return cells.to_string();
    }
    let head: String = cells.chars().take(SUMMARY_CELLS_MAX).collect();
    format!("{head}... ({total} characters; the trace holds the full values)")
}
