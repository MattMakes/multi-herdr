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
