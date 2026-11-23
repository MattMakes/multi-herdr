use crate::os::awake as os;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwakeInstant {
    pub boot_id: String,
    pub awake_ms: u64,
}

pub fn now() -> io::Result<AwakeInstant> {
    Ok(AwakeInstant {
        boot_id: os::boot_id()?,
        awake_ms: os::awake_ms()?,
    })
}

impl AwakeInstant {
    pub fn elapsed_since(&self, earlier: &AwakeInstant) -> Option<Duration> {
        if self.boot_id != earlier.boot_id {
            return None;
        }
        self.awake_ms
            .checked_sub(earlier.awake_ms)
            .map(Duration::from_millis)
    }
}
