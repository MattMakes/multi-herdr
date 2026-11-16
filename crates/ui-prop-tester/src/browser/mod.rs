use crate::driver::{BrowserConfig, Driver, DriverError};

pub fn open(_config: &BrowserConfig) -> Result<Box<dyn Driver>, DriverError> {
    Err(DriverError::Setup(
        "the browser driver is not built yet".into(),
    ))
}
