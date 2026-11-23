use std::io;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;

pub(crate) fn awake_ms() -> io::Result<u64> {
    let mut units = 0u64;
    // SAFETY: `units` is a valid, writable `u64` that lives for the call.
    if unsafe { QueryUnbiasedInterruptTime(&mut units) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(units / 10_000)
}

pub(crate) fn boot_id() -> io::Result<String> {
    // SAFETY: `GetTickCount64` takes no argument and touches no memory of
    // this process.
    let since_boot_ms = unsafe { GetTickCount64() };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("system time before 1970"))?
        .as_millis();
    let boot_ms = now_ms
        .checked_sub(u128::from(since_boot_ms))
        .ok_or_else(|| io::Error::other("system time before boot"))?;
    Ok(((boot_ms + 500) / 1000).to_string())
}
