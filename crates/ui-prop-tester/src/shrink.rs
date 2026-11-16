use std::time::Instant;

use crate::cli::ShrinkArgs;
use crate::driver::{Action, DriverFactory, DriverKind};
use crate::engine::{
    execute, fail, install_interrupt_handler, interrupted, kind_name, reset_interrupt,
    write_report, Failure, RunPlan, Target,
};
use crate::output_path::{check_distinct_from_input, check_output_path};
use crate::spec::Specification;
use crate::trace::{read_trace_file, Trace};
use crate::{Io, EXIT_OK};

struct Search<'a> {
    target: &'a Target,
    specification: Option<&'a Specification>,
    seed: u64,
    property: &'a str,
    confirmation_runs_max: u32,
    drivers: &'a dyn DriverFactory,
}

enum Attempt {
    Reproduced(Trace),
    Missed(String),
    Invalid(String),
}

impl Search<'_> {
    fn replay(&self, actions: &[Action]) -> Attempt {
        let mut plan = RunPlan::new(self.target, self.specification, self.seed);
        plan.replay = Some(actions);
        plan.stop_on_property = Some(self.property);
        let report = execute(&plan, self.drivers);
        match &report.failure {
            Some(Failure::Divergence(message)) => Attempt::Invalid(message.clone()),
            Some(failure) => Attempt::Missed(failure.message()),
            None if report.violated(self.property) => Attempt::Reproduced(report.trace),
            None => Attempt::Missed(format!("{} held", self.property)),
        }
    }

    fn confirm(&self, actions: &[Action]) -> Option<Trace> {
        for _ in 0..self.confirmation_runs_max.max(1) {
            match self.replay(actions) {
                Attempt::Reproduced(trace) => return Some(trace),
                Attempt::Invalid(_) => return None,
                Attempt::Missed(_) => {}
            }
        }
        None
    }
}

pub fn run(
    kind: DriverKind,
    args: &ShrinkArgs,
    io: &mut Io<'_>,
    drivers: &dyn DriverFactory,
) -> i32 {
    install_interrupt_handler();
    reset_interrupt();
    if let Err(message) = check_distinct_from_input(&args.output_path, &args.trace) {
        return fail(io, &message);
    }
    if let Err(message) = check_output_path(&args.output_path, args.output_path_overwrite) {
        return fail(io, &message);
    }
    let input = match read_trace_file(&args.trace) {
        Ok(trace) => trace,
        Err(error) => {
            return fail(
                io,
                &format!("cannot read the trace {}: {error}", args.trace.display()),
            )
        }
    };
    if input.header.driver != kind {
        return fail(
            io,
            &format!(
                "the trace was recorded with the {} driver; shrink it with `ui-prop-tester {} shrink`",
                kind_name(input.header.driver),
                kind_name(input.header.driver)
            ),
        );
    }
    let target = match Target::from_header(kind, &input.header.target) {
        Ok(target) => target,
        Err(message) => return fail(io, &message),
    };
    let specification = match &args.specification {
        Some(path) => match Specification::from_file(path) {
            Ok(spec) => Some(spec),
            Err(message) => return fail(io, &message),
        },
        None => input
            .header
            .specification
            .as_ref()
            .map(Specification::from_record),
    };
    let violated = input.violated_properties();
    let property = match &args.property {
        Some(name) => name.clone(),
        None => match violated.as_slice() {
            [] => {
                return fail(
                    io,
                    "the trace records no violation, so there is nothing to shrink",
                )
            }
            [only] => only.clone(),
            several => {
                return fail(
                    io,
                    &format!(
                        "the trace violates several properties ({}); choose one with --property",
                        several.join(", ")
                    ),
                )
            }
        },
    };
    let search = Search {
        target: &target,
        specification: specification.as_ref(),
        seed: input.header.seed,
        property: &property,
        confirmation_runs_max: args.confirmation_runs_max,
        drivers,
    };
    let original = input.actions();
    let _ = writeln!(
        io.out,
        "ui-prop-tester: shrinking {property} from {} actions (seed {})",
        original.len(),
        input.header.seed
    );
    let mut best: Option<Trace> = None;
    for run in 1..=args.calibration_runs {
        match search.replay(&original) {
            Attempt::Reproduced(trace) => {
                if best.is_none() {
                    best = Some(trace);
                }
            }
            Attempt::Missed(why) | Attempt::Invalid(why) => {
                return fail(
                    io,
                    &format!(
                        "calibration run {run} of {} did not reproduce {property}: {why}",
                        args.calibration_runs
                    ),
                )
            }
        }
    }
    let mut current = match &best {
        Some(trace) => trace.actions(),
        None => original.clone(),
    };
    let deadline = args.time_limit.map(|limit| Instant::now() + limit);
    let out_of_time =
        || interrupted() || deadline.is_some_and(|deadline| Instant::now() >= deadline);
    let parallelism = args.parallelism.max(1);
    let mut chunk = (current.len() / 2).max(1);
    while !out_of_time() && !current.is_empty() {
        let mut changed = false;
        let mut start = 0;
        while start < current.len() && !out_of_time() {
            let positions: Vec<usize> = (0..parallelism)
                .map(|offset| start + offset * chunk)
                .filter(|&position| position < current.len())
                .collect();
            let candidates: Vec<Vec<Action>> = positions
                .iter()
                .map(|&position| {
                    let end = (position + chunk).min(current.len());
                    let mut candidate = current[..position].to_vec();
                    candidate.extend_from_slice(&current[end..]);
                    candidate
                })
                .collect();
            let results = confirm_all(&search, &candidates);
            let accepted = results
                .into_iter()
                .enumerate()
                .find_map(|(slot, trace)| trace.map(|trace| (slot, trace)));
            match accepted {
                Some((slot, trace)) => {
                    current = trace.actions();
                    best = Some(trace);
                    start = positions[slot];
                    changed = true;
                }
                None => {
                    start = positions.last().copied().unwrap_or(start) + chunk;
                }
            }
        }
        if chunk > 1 {
            chunk = (chunk / 2).max(1);
        } else if !changed {
            break;
        }
    }
    let best = match best {
        Some(trace) => trace,
        None => match search.replay(&current) {
            Attempt::Reproduced(trace) => trace,
            Attempt::Missed(why) | Attempt::Invalid(why) => {
                return fail(
                    io,
                    &format!("the trace did not reproduce {property}: {why}"),
                )
            }
        },
    };
    let shortest = best.actions().len();
    if let Err(message) = write_report(&best, &args.output_path) {
        return fail(io, &message);
    }
    let _ = writeln!(
        io.out,
        "ui-prop-tester: shrunk {property} from {} to {shortest} actions",
        original.len()
    );
    let _ = writeln!(
        io.out,
        "ui-prop-tester: trace {}",
        args.output_path.display()
    );
    EXIT_OK
}

fn confirm_all(search: &Search<'_>, candidates: &[Vec<Action>]) -> Vec<Option<Trace>> {
    if candidates.len() == 1 {
        return vec![search.confirm(&candidates[0])];
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .iter()
            .map(|candidate| scope.spawn(move || search.confirm(candidate)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(None))
            .collect()
    })
}
