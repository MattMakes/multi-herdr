use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::cli::FuzzArgs;
use crate::driver::DriverFactory;
use crate::engine::{
    default_trace_path, execute, fail, install_interrupt_handler, interrupted, print_violations,
    reset_interrupt, write_report, RunPlan, Target,
};
use crate::output_path::{check_output_path, numbered_sibling};
use crate::rng::{seed_from_clock, SplitMix64};
use crate::spec::{SpecRuntime, Specification};
use crate::trace::Violation;
use crate::{Io, EXIT_ERROR, EXIT_OK, EXIT_VIOLATION};

pub const DEFAULT_RUN_LIMIT: Duration = Duration::from_secs(30);

pub fn run_seed(campaign_seed: u64, number: u64) -> u64 {
    SplitMix64::new(campaign_seed ^ number.wrapping_mul(0x9E37_79B9_7F4A_7C15)).next_u64()
}

struct Campaign<'a> {
    target: &'a Target,
    specification: Option<&'a Specification>,
    args: &'a FuzzArgs,
    seed: u64,
    run_limit: Duration,
    deadline: Option<Instant>,
    stop: &'a AtomicBool,
    drivers: &'a dyn DriverFactory,
}

struct RunResult {
    number: u64,
    seed: u64,
    enabled: Vec<String>,
    swarm: bool,
    states: usize,
    violations: Vec<Violation>,
    failure: Option<String>,
    trace: Option<Result<PathBuf, String>>,
}

pub fn run(target: Target, args: &FuzzArgs, io: &mut Io<'_>, drivers: &dyn DriverFactory) -> i32 {
    install_interrupt_handler();
    reset_interrupt();
    let campaign_seed = args.seed.unwrap_or_else(seed_from_clock);
    let specification = match &args.specification {
        Some(path) => match Specification::from_file(path) {
            Ok(spec) => Some(spec),
            Err(message) => return fail(io, &message),
        },
        None => None,
    };
    if let Err(message) = SpecRuntime::load(target.kind(), specification.as_ref(), campaign_seed) {
        return fail(io, &message);
    }
    if let Some(base) = &args.output_path {
        let first = numbered_sibling(base, 1);
        if let Err(message) = check_output_path(&first, args.output_path_overwrite) {
            return fail(io, &message);
        }
    }
    let run_limit = args.time_limit_run.unwrap_or(DEFAULT_RUN_LIMIT);
    let campaign_deadline = args.time_limit_fuzz.map(|limit| Instant::now() + limit);
    let parallelism = args.parallelism.max(1);
    let next_number = AtomicU64::new(1);
    let stop = AtomicBool::new(false);
    let _ = writeln!(io.out, "ui-prop-tester: campaign seed {campaign_seed}");
    let mut runs = 0u64;
    let mut violating = 0u64;
    let mut errors = 0u64;
    let campaign = Campaign {
        target: &target,
        specification: specification.as_ref(),
        args,
        seed: campaign_seed,
        run_limit,
        deadline: campaign_deadline,
        stop: &stop,
        drivers,
    };
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel::<RunResult>();
        for _ in 0..parallelism {
            let sender = sender.clone();
            let campaign = &campaign;
            let next_number = &next_number;
            let stop = &stop;
            scope.spawn(move || loop {
                if stop.load(Ordering::SeqCst)
                    || interrupted()
                    || campaign_deadline.is_some_and(|deadline| Instant::now() >= deadline)
                {
                    break;
                }
                let number = next_number.fetch_add(1, Ordering::SeqCst);
                if args.runs_max.is_some_and(|limit| number > limit) {
                    break;
                }
                let result = campaign.run(number);
                let refused = matches!(&result.trace, Some(Err(_)));
                if (args.exit_on_violation && !result.violations.is_empty()) || refused {
                    stop.store(true, Ordering::SeqCst);
                }
                if sender.send(result).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        for result in receiver {
            runs += 1;
            if !result.violations.is_empty() {
                violating += 1;
            }
            if result.failure.is_some() || matches!(&result.trace, Some(Err(_))) {
                errors += 1;
            }
            print_run(io, &result);
        }
    });
    let _ = writeln!(
        io.out,
        "ui-prop-tester: campaign seed {campaign_seed}: {runs} runs, {violating} with violations, {errors} with errors"
    );
    if violating > 0 {
        EXIT_VIOLATION
    } else if errors > 0 {
        EXIT_ERROR
    } else {
        EXIT_OK
    }
}

impl Campaign<'_> {
    fn run(&self, number: u64) -> RunResult {
        let args = self.args;
        let seed = run_seed(self.seed, number);
        let output = match &args.output_path {
            Some(base) => {
                let path = numbered_sibling(base, number);
                if let Err(message) = check_output_path(&path, args.output_path_overwrite) {
                    return RunResult {
                        number,
                        seed,
                        enabled: Vec::new(),
                        swarm: args.swarm,
                        states: 0,
                        violations: Vec::new(),
                        failure: None,
                        trace: Some(Err(message)),
                    };
                }
                Some(path)
            }
            None => None,
        };
        let mut plan = RunPlan::new(self.target, self.specification, seed);
        plan.time_limit = Some(self.run_limit);
        plan.hard_deadline = self.deadline;
        plan.actions_max = args.actions_max;
        plan.exit_on_violation = args.exit_on_violation;
        plan.swarm = args.swarm;
        plan.stop = Some(self.stop);
        let report = execute(&plan, self.drivers);
        let keep = output.is_some() || report.failure.is_some() || !report.violations.is_empty();
        let trace = keep.then(|| {
            let path = output
                .clone()
                .unwrap_or_else(|| default_trace_path(seed, &format!("-run{number}")));
            write_report(&report.trace, &path).map(|()| path)
        });
        RunResult {
            number,
            seed,
            enabled: report.enabled.clone(),
            swarm: args.swarm,
            states: report.trace.states.len(),
            violations: report.violations.clone(),
            failure: report.failure.as_ref().map(|failure| failure.message()),
            trace,
        }
    }
}

fn print_run(io: &mut Io<'_>, result: &RunResult) {
    let categories = if result.swarm {
        format!(", categories {}", result.enabled.join(","))
    } else {
        String::new()
    };
    let verdict = match (&result.failure, result.violations.is_empty()) {
        (Some(message), _) => format!("error: {message}"),
        (None, true) => format!("passed, {} states", result.states),
        (None, false) => format!(
            "{} violations in {} states",
            result.violations.len(),
            result.states
        ),
    };
    let _ = writeln!(
        io.out,
        "run {} (seed {}{categories}): {verdict}",
        result.number, result.seed
    );
    print_violations(io, &result.violations, "  ");
    match &result.trace {
        Some(Ok(path)) => {
            let _ = writeln!(io.out, "  trace {}", path.display());
        }
        Some(Err(message)) => {
            let _ = writeln!(
                io.err,
                "ui-prop-tester: error: run {}: {message}",
                result.number
            );
        }
        None => {}
    }
}
