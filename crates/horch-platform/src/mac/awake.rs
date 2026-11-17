//! The concern awake for macOS.

use std::ffi::CStr;
use std::io;

/// `CLOCK_UPTIME_RAW` is the clock of `mach_absolute_time`: it does not
/// count the time asleep.
pub(crate) fn awake_ms() -> io::Result<u64> {
    crate::unix::awake::clock_ms(libc::CLOCK_UPTIME_RAW)
}

/// `kern.bootsessionuuid`, the UUID that the kernel makes at each boot.
pub(crate) fn boot_id() -> io::Result<String> {
    // A UUID has 36 characters and the NUL.
    let mut buf = [0u8; 64];
    let mut len = buf.len();
    // SAFETY: the name is a NUL-terminated C string. `buf` is writable for
    // `len` bytes, and `len` is a valid, writable `size_t`. No new value is
    // set: the new-value pointer is null with length 0.
    let r = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    let id = CStr::from_bytes_until_nul(&buf[..len.min(buf.len())])
        .map_err(|_| io::Error::other("kern.bootsessionuuid has no NUL"))?
        .to_str()
        .map_err(|_| io::Error::other("kern.bootsessionuuid is not UTF-8"))?;
    if id.is_empty() {
        return Err(io::Error::other("empty kern.bootsessionuuid"));
    }
    Ok(id.to_string())
}
