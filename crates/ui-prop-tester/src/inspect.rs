use crate::cli::InspectArgs;
use crate::{Io, EXIT_ERROR};

pub fn run(_args: &InspectArgs, io: &mut Io<'_>) -> i32 {
    let _ = writeln!(io.err, "ui-prop-tester: inspect is not built yet");
    EXIT_ERROR
}
