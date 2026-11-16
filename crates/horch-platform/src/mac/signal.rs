//! The concern signal for macOS. macOS has no pidfd for a process that is
//! not a child, so a signal is the start-time check and then `kill`.

pub(crate) use crate::unix::signal::check_and_kill as signal_same;
pub(crate) use crate::unix::signal::{
    kill_group_on_termination, on_termination_set, outlive_hangup, stop_same, terminate_politely,
};
