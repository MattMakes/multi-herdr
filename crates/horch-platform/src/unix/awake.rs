use std::io;

pub(crate) fn clock_ms(clock: libc::clockid_t) -> io::Result<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable `timespec` that lives for the call.
    if unsafe { libc::clock_gettime(clock, &mut ts) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let secs = u64::try_from(ts.tv_sec).map_err(|_| io::Error::other("negative clock"))?;
    let nanos = u64::try_from(ts.tv_nsec).map_err(|_| io::Error::other("negative clock"))?;
    Ok(secs * 1000 + nanos / 1_000_000)
}
