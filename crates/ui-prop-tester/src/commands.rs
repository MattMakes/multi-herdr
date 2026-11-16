use crate::cli::{BrowserCommand, Cli, Command, TerminalCommand};
use crate::driver::{DriverFactory, DriverKind};
use crate::engine::{self, Target};
use crate::{fuzz, inspect, shrink, trace_cmd, Io};

pub fn dispatch(cli: Cli, io: &mut Io<'_>, drivers: &dyn DriverFactory) -> i32 {
    match cli.command {
        Command::Trace(args) => trace_cmd::run(&args, io),
        Command::Browser(BrowserCommand::Inspect(args)) => inspect::run(&args, io),
        Command::Browser(BrowserCommand::Test(args)) => {
            let target = Target::Browser(args.chrome.config(&args.origin, None, false));
            engine::test(target, &args.run, &args.reproduce, io, drivers)
        }
        Command::Browser(BrowserCommand::TestExternal(args)) => {
            let target = Target::Browser(args.chrome.config(
                &args.origin,
                Some(&args.remote_debugger),
                args.create_target,
            ));
            engine::test(target, &args.run, &args.reproduce, io, drivers)
        }
        Command::Browser(BrowserCommand::Fuzz(args)) => {
            let target = Target::Browser(args.chrome.config(&args.origin, None, false));
            fuzz::run(target, &args.fuzz, io, drivers)
        }
        Command::Browser(BrowserCommand::Shrink(args)) => {
            shrink::run(DriverKind::Browser, &args, io, drivers)
        }
        Command::Terminal(TerminalCommand::Test(args)) => {
            let target = Target::Terminal(args.pty.config(&args.program, &args.args));
            engine::test(target, &args.run, &args.reproduce, io, drivers)
        }
        Command::Terminal(TerminalCommand::Fuzz(args)) => {
            let target = Target::Terminal(args.pty.config(&args.program, &args.args));
            fuzz::run(target, &args.fuzz, io, drivers)
        }
        Command::Terminal(TerminalCommand::Shrink(args)) => {
            shrink::run(DriverKind::Terminal, &args, io, drivers)
        }
    }
}
