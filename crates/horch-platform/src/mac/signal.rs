pub(crate) use crate::unix::signal::check_and_kill as signal_same;
pub(crate) use crate::unix::signal::{
    end_termination_hold, hold_termination, kill_group_on_termination, on_termination_set,
    outlive_hangup, signal_if_same, stop_same, terminate_politely,
};
