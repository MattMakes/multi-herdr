use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::sandbox::{Network, Rule};
use crate::NotSupported;

pub(crate) fn program(_path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    Err(NotSupported::new("sandbox", "windows")
        .because("Windows has no Seatbelt and no bubblewrap"))
}

pub(crate) fn args(
    _rules: &[Rule<'_>],
    _temp: bool,
    _network: Network,
    _own_pids: bool,
) -> Vec<OsString> {
    Vec::new()
}
