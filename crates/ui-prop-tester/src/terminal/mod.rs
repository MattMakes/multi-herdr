use crate::driver::{Driver, DriverError, TerminalConfig};

pub fn open(_config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError> {
    Err(DriverError::Setup(
        "the terminal driver is not built yet".into(),
    ))
}
