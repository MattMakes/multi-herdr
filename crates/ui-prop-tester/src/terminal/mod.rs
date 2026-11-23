mod cell;
#[cfg(unix)]
mod driver;
pub mod emulator;
mod grid;
#[cfg(unix)]
mod pty;
mod sgr;

use crate::driver::{Driver, DriverError, TerminalConfig};

#[cfg(unix)]
pub fn open(config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError> {
    Ok(Box::new(driver::TerminalDriver::new(config)?))
}

#[cfg(not(unix))]
pub fn open(_config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError> {
    Err(DriverError::Setup(
        "the terminal driver needs a Unix pseudo-terminal and is not available on this system"
            .into(),
    ))
}
