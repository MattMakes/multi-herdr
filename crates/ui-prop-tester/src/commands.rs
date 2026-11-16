use crate::cli::{BrowserCommand, Cli, Command, TerminalCommand};
use crate::driver::DriverFactory;
use crate::{inspect, trace_cmd, Io, EXIT_ERROR};

pub fn dispatch(cli: Cli, io: &mut Io<'_>, _drivers: &dyn DriverFactory) -> i32 {
    match cli.command {
        Command::Trace(args) => trace_cmd::run(&args, io),
        Command::Browser(BrowserCommand::Inspect(args)) => inspect::run(&args, io),
        Command::Browser(_)
        | Command::Terminal(TerminalCommand::Test(_))
        | Command::Terminal(TerminalCommand::Fuzz(_))
        | Command::Terminal(TerminalCommand::Shrink(_)) => {
            let _ = writeln!(io.err, "ui-prop-tester: this command is not built yet");
            EXIT_ERROR
        }
    }
}
