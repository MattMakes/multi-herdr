//! The operator boundary of a worker sandbox: the 3 operator acts that each
//! adapter denies, and the paths that a worker can write (its task folder
//! and its worktree).
//!
//! Requirements: SR-8, FM-32, RR-5. Each adapter has 1 function
//! `operator_boundary(teammate, fleet_root) -> Result<Boundary, NotSupported>`
//! that reads the config it renders for a worker and says, for each act,
//! whether that config denies it. Profile validation refuses a profile whose
//! boundary allows 1 act, and a profile whose adapter gives [`NotSupported`]
//! on this OS (A37).

use std::path::{Path, PathBuf};

/// 1 of the 3 operator acts that a worker sandbox denies (SR-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Read the operator token.
    ReadToken,
    /// Connect to the command channel of fleetd.
    ConnectChannel,
    /// Write into the request folder.
    WriteRequests,
}

impl Act {
    pub const ALL: [Act; 3] = [Act::ReadToken, Act::ConnectChannel, Act::WriteRequests];
}

impl std::fmt::Display for Act {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Act::ReadToken => "read the operator token",
            Act::ConnectChannel => "connect to the command channel",
            Act::WriteRequests => "write into the request folder",
        })
    }
}

/// The paths of the 3 operator acts under 1 fleet root
/// (`docs/tickets/fleetd-p5/MAP.md`, Q17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorPaths {
    /// `<fleet root>/run`: the token, the channel, the request folder and
    /// the job result files. A worker sandbox denies each write in it.
    pub run: PathBuf,
    /// `<fleet root>/run/operator.token`.
    pub token: PathBuf,
    /// `<fleet root>/run/fleetd.sock`.
    pub channel: PathBuf,
    /// `<fleet root>/run/requests`.
    pub requests: PathBuf,
}

impl OperatorPaths {
    pub fn of(fleet_root: &Path) -> OperatorPaths {
        let run = fleet_root.join("run");
        OperatorPaths {
            token: run.join("operator.token"),
            channel: run.join("fleetd.sock"),
            requests: run.join("requests"),
            run,
        }
    }

    /// The path of `act`.
    pub fn of_act(&self, act: Act) -> &Path {
        match act {
            Act::ReadToken => &self.token,
            Act::ConnectChannel => &self.channel,
            Act::WriteRequests => &self.requests,
        }
    }
}

/// For each operator act, whether the rendered config of a CLI denies it.
/// The default denies nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Boundary {
    pub read_token_denied: bool,
    pub connect_channel_denied: bool,
    pub write_requests_denied: bool,
}

impl Boundary {
    /// Whether the config denies `act`.
    pub fn denies(&self, act: Act) -> bool {
        match act {
            Act::ReadToken => self.read_token_denied,
            Act::ConnectChannel => self.connect_channel_denied,
            Act::WriteRequests => self.write_requests_denied,
        }
    }

    /// The acts that the config does not deny.
    pub fn allowed(&self) -> Vec<Act> {
        Act::ALL.into_iter().filter(|a| !self.denies(*a)).collect()
    }

    /// Whether the config denies all 3 acts.
    pub fn holds(&self) -> bool {
        self.allowed().is_empty()
    }
}

/// The sandbox of an adapter does not exist on this OS, so no worker of
/// that adapter runs here (06 S9, A37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotSupported {
    /// The adapter, for example `"claude"`.
    pub adapter: &'static str,
    /// The OS, for example `"windows"`.
    pub os: String,
    pub reason: &'static str,
}

impl std::fmt::Display for NotSupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the {} sandbox is not supported on {}: {}",
            self.adapter, self.os, self.reason
        )
    }
}

impl std::error::Error for NotSupported {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paths_are_under_the_run_folder_of_the_fleet_root() {
        let paths = OperatorPaths::of(Path::new("/fleet"));
        assert_eq!(paths.run, Path::new("/fleet/run"));
        assert_eq!(paths.token, Path::new("/fleet/run/operator.token"));
        assert_eq!(paths.channel, Path::new("/fleet/run/fleetd.sock"));
        assert_eq!(paths.requests, Path::new("/fleet/run/requests"));
    }

    #[test]
    fn a_boundary_holds_only_when_it_denies_each_act() {
        let all = Boundary {
            read_token_denied: true,
            connect_channel_denied: true,
            write_requests_denied: true,
        };
        assert!(all.holds());
        let open = Boundary {
            connect_channel_denied: false,
            ..all
        };
        assert!(!open.holds());
        assert_eq!(open.allowed(), [Act::ConnectChannel]);
    }
}
