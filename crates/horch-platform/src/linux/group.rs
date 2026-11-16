//! The concern group for Linux.

use std::process::Child;

pub(crate) use crate::unix::group::{
    detach, is_same_group, kill_child_group, kill_group_id, new_group, Contained,
};

pub(crate) fn exited_without_reap(child: &mut Child) -> std::io::Result<bool> {
    let info = crate::unix::group::wait_without_reap(child)?;
    // SAFETY: `waitid` succeeded, so the pid field is initialised.
    Ok(unsafe { info.si_pid() } != 0)
}
