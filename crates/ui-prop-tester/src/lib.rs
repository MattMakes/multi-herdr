pub mod browser;
pub mod cli;
pub mod commands;
pub mod driver;
pub mod duration;
pub mod engine;
pub mod fake;
pub mod fuzz;
pub mod inspect;
pub mod output_path;
pub mod rng;
pub mod shrink;
pub mod spec;
pub mod terminal;
pub mod trace;
pub mod trace_cmd;

use std::ffi::OsString;
use std::io::Write;

pub use driver::{
    Action, BrowserConfig, Driver, DriverError, DriverFactory, DriverKind, ExtractOutcome,
    Observation, RemoteExtractor, SpecSetup, TerminalConfig,
};

pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_VIOLATION: i32 = 2;

pub struct Io<'a> {
    pub out: &'a mut (dyn Write + Send),
    pub err: &'a mut (dyn Write + Send),
}

pub struct RealDrivers;

impl DriverFactory for RealDrivers {
    fn browser(&self, config: &BrowserConfig) -> Result<Box<dyn Driver>, DriverError> {
        browser::open(config)
    }

    fn terminal(&self, config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError> {
        terminal::open(config)
    }
}

pub fn run(args: Vec<OsString>, io: &mut Io<'_>, drivers: &dyn DriverFactory) -> i32 {
    use clap::Parser;
    match cli::Cli::try_parse_from(args) {
        Ok(cli) => commands::dispatch(cli, io, drivers),
        Err(error) => {
            let informational = !error.use_stderr();
            let rendered = error.render().to_string();
            if informational {
                let _ = write!(io.out, "{rendered}");
                EXIT_OK
            } else {
                let _ = write!(io.err, "{rendered}");
                EXIT_ERROR
            }
        }
    }
}
