use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

use crate::driver::{BrowserConfig, TerminalConfig};
use crate::duration::parse_duration;

#[derive(Debug, Parser)]
#[command(
    name = "ui-prop-tester",
    version,
    about = "Property-based exploration of browser and terminal user interfaces"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(subcommand, about = "Test a web page in Chromium")]
    Browser(BrowserCommand),
    #[command(subcommand, about = "Test a program in a pseudo-terminal")]
    Terminal(TerminalCommand),
    #[command(about = "Print the states of a recorded trace")]
    Trace(TraceArgs),
}

#[derive(Debug, Subcommand)]
pub enum BrowserCommand {
    #[command(about = "Run one random walk against a page")]
    Test(BrowserTestArgs),
    #[command(
        name = "test-external",
        about = "Run one random walk in a running browser"
    )]
    TestExternal(BrowserExternalArgs),
    #[command(about = "Run many short random walks")]
    Fuzz(BrowserFuzzArgs),
    #[command(about = "Reduce a failing trace to its shortest failing action list")]
    Shrink(ShrinkArgs),
    #[command(about = "Serve a local viewer for a trace")]
    Inspect(InspectArgs),
}

#[derive(Debug, Subcommand)]
pub enum TerminalCommand {
    #[command(about = "Run one random walk against a program")]
    Test(TerminalTestArgs),
    #[command(about = "Run many short random walks")]
    Fuzz(TerminalFuzzArgs),
    #[command(about = "Reduce a failing trace to its shortest failing action list")]
    Shrink(ShrinkArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    #[arg(
        long,
        value_name = "FILE",
        help = "ES module that exports properties and action generators"
    )]
    pub specification: Option<PathBuf>,
    #[arg(
        long,
        env = "UI_PROP_TESTER_SEED",
        value_name = "SEED",
        help = "Seed for every random choice"
    )]
    pub seed: Option<u64>,
    #[arg(long, value_name = "DURATION", value_parser = parse_duration, help = "Stop after this long, for example 30s or 5m")]
    pub time_limit: Option<Duration>,
    #[arg(long, value_name = "COUNT", help = "Stop after this many actions")]
    pub actions_max: Option<u64>,
    #[arg(long, help = "Stop at the first property violation")]
    pub exit_on_violation: bool,
    #[arg(long, value_name = "FILE", help = "Where to write the trace")]
    pub output_path: Option<PathBuf>,
    #[arg(long, help = "Replace an existing file at --output-path")]
    pub output_path_overwrite: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ReproduceArgs {
    #[arg(
        long,
        value_name = "TRACE_FILE",
        help = "Replay the actions of a recorded trace in order"
    )]
    pub reproduce: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct FuzzArgs {
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 1,
        help = "Runs to execute at the same time"
    )]
    pub parallelism: usize,
    #[arg(long, value_name = "DURATION", value_parser = parse_duration, help = "Stop the whole campaign after this long")]
    pub time_limit_fuzz: Option<Duration>,
    #[arg(long, value_name = "DURATION", value_parser = parse_duration, help = "Stop each run after this long")]
    pub time_limit_run: Option<Duration>,
    #[arg(
        long,
        value_name = "COUNT",
        help = "Stop the campaign after this many runs"
    )]
    pub runs_max: Option<u64>,
    #[arg(long, help = "Give each run a random subset of the action categories")]
    pub swarm: bool,
    #[arg(
        long,
        value_name = "FILE",
        help = "ES module that exports properties and action generators"
    )]
    pub specification: Option<PathBuf>,
    #[arg(
        long,
        env = "UI_PROP_TESTER_SEED",
        value_name = "SEED",
        help = "Seed for the campaign; each run derives its own seed from it"
    )]
    pub seed: Option<u64>,
    #[arg(
        long,
        value_name = "COUNT",
        help = "Stop each run after this many actions"
    )]
    pub actions_max: Option<u64>,
    #[arg(long, help = "Stop the campaign at the first property violation")]
    pub exit_on_violation: bool,
    #[arg(
        long,
        value_name = "FILE",
        help = "Base path; run N writes a sibling file with -N before the extension"
    )]
    pub output_path: Option<PathBuf>,
    #[arg(long, help = "Replace existing files at the numbered output paths")]
    pub output_path_overwrite: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ShrinkArgs {
    #[arg(long, value_name = "TRACE_FILE", help = "The failing trace to reduce")]
    pub trace: PathBuf,
    #[arg(long, value_name = "FILE", help = "Where to write the reduced trace")]
    pub output_path: PathBuf,
    #[arg(long, help = "Replace an existing file at --output-path")]
    pub output_path_overwrite: bool,
    #[arg(
        long,
        value_name = "NAME",
        help = "The property to keep failing; required when the trace violates several"
    )]
    pub property: Option<String>,
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 3,
        help = "Replays that must all fail before the search starts"
    )]
    pub calibration_runs: u32,
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 5,
        help = "Most replays used to confirm one candidate"
    )]
    pub confirmation_runs_max: u32,
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = 1,
        help = "Candidates to replay at the same time"
    )]
    pub parallelism: usize,
    #[arg(long, value_name = "DURATION", value_parser = parse_duration, help = "Stop searching after this long and keep the best result")]
    pub time_limit: Option<Duration>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Use this specification instead of the one stored in the trace"
    )]
    pub specification: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ChromeArgs {
    #[arg(long, help = "Run Chromium without a window")]
    pub headless: bool,
    #[arg(long, help = "Run Chromium without its sandbox")]
    pub no_sandbox: bool,
    #[arg(long, value_name = "PIXELS", default_value_t = 1024)]
    pub width: u32,
    #[arg(long, value_name = "PIXELS", default_value_t = 768)]
    pub height: u32,
    #[arg(long, value_name = "FACTOR", default_value_t = 1.0)]
    pub device_scale_factor: f64,
    #[arg(
        long,
        value_name = "PERMISSIONS",
        value_delimiter = ',',
        help = "Comma-separated permissions to grant before the first navigation"
    )]
    pub chrome_grant_permissions: Vec<String>,
    #[arg(
        long,
        value_name = "POLICY",
        help = "Virtual time policy: advance, pause or pauseIfNetworkFetchesPending"
    )]
    pub chrome_virtual_time_policy: Option<String>,
    #[arg(long, value_name = "KEY=VALUE", value_parser = parse_header, help = "Extra request header; repeat for more")]
    pub header: Vec<(String, String)>,
    #[arg(
        long,
        value_name = "SET-COOKIE",
        help = "Cookie in Set-Cookie form; repeat for more"
    )]
    pub cookie: Vec<String>,
    #[arg(
        long,
        value_name = "MATCH_PATTERN",
        help = "URL match pattern the walk may visit; replaces the default boundary"
    )]
    pub allow_url: Vec<String>,
    #[arg(long, help = "Record JavaScript branch coverage")]
    pub instrument_javascript: bool,
    #[arg(
        long,
        value_name = "FILE",
        help = "Write the covered script symbols to this file"
    )]
    pub coverage_symbols: Option<PathBuf>,
    #[arg(
        long,
        env = "UI_PROP_TESTER_CHROME",
        value_name = "FILE",
        help = "Chromium executable to launch"
    )]
    pub chrome: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BrowserTestArgs {
    #[command(flatten)]
    pub run: RunArgs,
    #[command(flatten)]
    pub reproduce: ReproduceArgs,
    #[command(flatten)]
    pub chrome: ChromeArgs,
    #[arg(
        value_name = "ORIGIN",
        help = "http or https URL, or a local file path"
    )]
    pub origin: String,
}

#[derive(Debug, Clone, Args)]
pub struct BrowserExternalArgs {
    #[command(flatten)]
    pub run: RunArgs,
    #[command(flatten)]
    pub reproduce: ReproduceArgs,
    #[command(flatten)]
    pub chrome: ChromeArgs,
    #[arg(
        long,
        value_name = "URL",
        help = "Debugging endpoint of the running browser"
    )]
    pub remote_debugger: String,
    #[arg(
        long,
        help = "Open a new target instead of attaching to an existing page"
    )]
    pub create_target: bool,
    #[arg(
        value_name = "ORIGIN",
        help = "http or https URL, or a local file path"
    )]
    pub origin: String,
}

#[derive(Debug, Clone, Args)]
pub struct BrowserFuzzArgs {
    #[command(flatten)]
    pub fuzz: FuzzArgs,
    #[command(flatten)]
    pub chrome: ChromeArgs,
    #[arg(
        value_name = "ORIGIN",
        help = "http or https URL, or a local file path"
    )]
    pub origin: String,
}

#[derive(Debug, Clone, Args)]
pub struct PtyArgs {
    #[arg(long, value_name = "COUNT", default_value_t = 100)]
    pub columns: u16,
    #[arg(long, value_name = "COUNT", default_value_t = 40)]
    pub rows: u16,
    #[arg(long, value_name = "COUNT", default_value_t = 100)]
    pub scrollback_lines_max: usize,
    #[arg(
        long,
        value_name = "MILLISECONDS",
        default_value_t = 5,
        help = "Output must stay quiet this long before a state is captured"
    )]
    pub quiescence_timeout_ms: u64,
}

#[derive(Debug, Clone, Args)]
pub struct TerminalTestArgs {
    #[command(flatten)]
    pub run: RunArgs,
    #[command(flatten)]
    pub reproduce: ReproduceArgs,
    #[command(flatten)]
    pub pty: PtyArgs,
    #[arg(value_name = "PROGRAM")]
    pub program: String,
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TerminalFuzzArgs {
    #[command(flatten)]
    pub fuzz: FuzzArgs,
    #[command(flatten)]
    pub pty: PtyArgs,
    #[arg(value_name = "PROGRAM")]
    pub program: String,
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TraceArgs {
    #[arg(value_name = "TRACE_FILE")]
    pub trace: PathBuf,
    #[arg(
        long,
        value_name = "INDEX|START..END",
        help = "One state or a range; either end of a range may be left out"
    )]
    pub state: Option<String>,
    #[arg(long, help = "Print one JSON object per state")]
    pub json: bool,
    #[arg(long, help = "Draw the terminal screen of each state")]
    pub screen: bool,
    #[arg(
        long,
        help = "Keep colours and text attributes when drawing the screen"
    )]
    pub color: bool,
    #[arg(
        long,
        value_name = "DIRECTORY",
        help = "Write each browser screenshot to this folder"
    )]
    pub screenshots: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct InspectArgs {
    #[arg(long, value_name = "PORT", default_value_t = 1073)]
    pub port: u16,
    #[arg(long, help = "Do not open the viewer in a browser")]
    pub no_open: bool,
    #[arg(value_name = "TRACE_FILE")]
    pub trace: PathBuf,
}

fn parse_header(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.trim().is_empty() => {
            Ok((key.trim().to_string(), value.to_string()))
        }
        _ => Err(format!("'{text}' is not a header; write KEY=VALUE")),
    }
}

impl ChromeArgs {
    pub fn config(
        &self,
        origin: &str,
        remote_debugger: Option<&str>,
        create_target: bool,
    ) -> BrowserConfig {
        BrowserConfig {
            origin: origin.to_string(),
            headless: self.headless,
            no_sandbox: self.no_sandbox,
            width: self.width,
            height: self.height,
            device_scale_factor: self.device_scale_factor,
            grant_permissions: self.chrome_grant_permissions.clone(),
            virtual_time_policy: self.chrome_virtual_time_policy.clone(),
            headers: self.header.clone(),
            cookies: self.cookie.clone(),
            allow_urls: self.allow_url.clone(),
            instrument_javascript: self.instrument_javascript,
            coverage_symbols: self.coverage_symbols.clone(),
            remote_debugger: remote_debugger.map(str::to_string),
            create_target,
            chrome: self.chrome.clone(),
        }
    }
}

impl PtyArgs {
    pub fn config(&self, program: &str, args: &[String]) -> TerminalConfig {
        TerminalConfig {
            program: program.to_string(),
            args: args.to_vec(),
            columns: self.columns,
            rows: self.rows,
            scrollback_lines_max: self.scrollback_lines_max,
            quiescence_timeout_ms: self.quiescence_timeout_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_tree_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn terminal_test_passes_hyphen_arguments_to_the_program() {
        let cli = Cli::try_parse_from([
            "ui-prop-tester",
            "terminal",
            "test",
            "--rows",
            "10",
            "sh",
            "-c",
            "echo hi",
        ])
        .unwrap();
        let Command::Terminal(TerminalCommand::Test(args)) = cli.command else {
            panic!("expected terminal test");
        };
        assert_eq!(args.pty.rows, 10);
        assert_eq!(args.program, "sh");
        assert_eq!(args.args, vec!["-c", "echo hi"]);
    }

    #[test]
    fn browser_defaults_match_the_documented_viewport() {
        let cli = Cli::try_parse_from(["ui-prop-tester", "browser", "test", "http://localhost:1"])
            .unwrap();
        let Command::Browser(BrowserCommand::Test(args)) = cli.command else {
            panic!("expected browser test");
        };
        let config = args.chrome.config(&args.origin, None, false);
        assert_eq!((config.width, config.height), (1024, 768));
        assert_eq!(config.device_scale_factor, 1.0);
    }

    #[test]
    fn headers_need_a_key_and_an_equals_sign() {
        assert!(parse_header("X-Token=abc=def").is_ok());
        assert!(parse_header("novalue").is_err());
        assert!(parse_header("=x").is_err());
    }
}
