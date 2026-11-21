use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    ReadToken,
    ConnectChannel,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorPaths {
    pub run: PathBuf,
    pub token: PathBuf,
    pub channel: PathBuf,
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

    pub fn of_act(&self, act: Act) -> &Path {
        match act {
            Act::ReadToken => &self.token,
            Act::ConnectChannel => &self.channel,
            Act::WriteRequests => &self.requests,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Boundary {
    pub read_token_denied: bool,
    pub connect_channel_denied: bool,
    pub write_requests_denied: bool,
}

impl Boundary {
    pub fn denies(&self, act: Act) -> bool {
        match act {
            Act::ReadToken => self.read_token_denied,
            Act::ConnectChannel => self.connect_channel_denied,
            Act::WriteRequests => self.write_requests_denied,
        }
    }

    pub fn allowed(&self) -> Vec<Act> {
        Act::ALL.into_iter().filter(|a| !self.denies(*a)).collect()
    }

    pub fn holds(&self) -> bool {
        self.allowed().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotSupported {
    pub adapter: &'static str,
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
