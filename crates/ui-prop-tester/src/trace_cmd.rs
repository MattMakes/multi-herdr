use crate::cli::TraceArgs;
use crate::{Io, EXIT_ERROR};

pub fn run(_args: &TraceArgs, io: &mut Io<'_>) -> i32 {
    let _ = writeln!(io.err, "ui-prop-tester: trace is not built yet");
    EXIT_ERROR
}
