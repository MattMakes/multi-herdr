//! `agent: none`: the smoke teammate, which exercises the machinery and
//! launches no CLI.

use horch_platform::start::Command;
use std::path::Path;

use anyhow::{bail, Result};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv};
use crate::fleetd::worker_profiles::domain::operator_boundary::{Boundary, NotSupported};
use crate::roster::Teammate;
use crate::skills::Bundle;

pub(crate) struct NoAgent;

impl Harness for NoAgent {
    fn kind(&self) -> HarnessKind {
        HarnessKind::None
    }

    /// Nothing is launched, so no field can fail to take effect.
    fn validate(&self, _t: &Teammate) -> Vec<&'static str> {
        Vec::new()
    }

    fn expose_skills(
        &self,
        _teammate: &Teammate,
        _skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        bail!("skills need an agent harness")
    }

    fn command(&self, _env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        bail!(
            "teammate '{}' has agent: none and cannot be launched",
            spec.teammate.name
        )
    }
}

/// The operator boundary of `agent: none` (SR-8): it launches no process,
/// so no process of it can read the token, connect to the command channel
/// or write into the request folder. It holds on each OS.
pub fn operator_boundary(
    _teammate: &Teammate,
    _fleet_root: &Path,
) -> Result<Boundary, NotSupported> {
    Ok(Boundary {
        read_token_denied: true,
        connect_channel_denied: true,
        write_requests_denied: true,
    })
}
