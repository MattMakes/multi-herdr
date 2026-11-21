use crate::os::group as os;
use crate::start::{Start, TOKEN};
use crate::NotSupported;
use std::process::Child;

pub fn new_group(cmd: &mut impl Start) {
    os::new_group(cmd.std_mut(TOKEN));
}

pub fn kill_group_id(pgid: u32) -> Result<bool, NotSupported> {
    os::kill_group_id(pgid)
}

pub fn kill_child_group(child: &mut Child) -> bool {
    os::kill_child_group(child)
}

pub fn term_child_group(child: &mut Child) -> bool {
    os::term_child_group(child)
}

pub fn exited_without_reap(child: &mut Child) -> std::io::Result<bool> {
    os::exited_without_reap(child)
}

pub fn detach(cmd: &mut impl Start) -> Result<(), NotSupported> {
    os::detach(cmd.std_mut(TOKEN))
}

pub fn spawn_detached(cmd: &mut impl Start) -> std::io::Result<Child> {
    let cmd = cmd.std_mut(TOKEN);
    for key in crate::start::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let _ = os::detach(cmd);
    os::spawn_without_inherited_stdio(cmd)
}

pub fn is_same_group(pgid: u32, started: u64) -> Result<bool, NotSupported> {
    os::is_same_group(pgid, started)
}

pub fn is_group_leader(pgid: u32, started: u64) -> Result<bool, NotSupported> {
    os::is_group_leader(pgid, started)
}

pub struct Contained(os::Contained);

impl Contained {
    pub fn prepare(cmd: &mut impl Start) {
        os::Contained::prepare(cmd.std_mut(TOKEN));
    }

    pub fn start(child: &Child) -> std::io::Result<Self> {
        os::Contained::start(child).map(Self)
    }

    pub fn terminate(&self) -> bool {
        self.0.terminate()
    }
}

#[derive(Debug)]
pub struct NotResumed;

impl std::fmt::Display for NotResumed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could not resume the child process")
    }
}

impl std::error::Error for NotResumed {}
