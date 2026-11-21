use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::machine::{Disk, Run, Tool};

macro_rules! rlimit {
    ($resource:expr) => {{
        // SAFETY: `limit` is a zeroed out-parameter that `getrlimit` fills in.
        let limit = unsafe {
            let mut limit: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit($resource, &mut limit) != 0 {
                None
            } else {
                Some(limit)
            }
        };
        limit
            .filter(|l| l.rlim_cur != libc::RLIM_INFINITY)
            .map(|l| l.rlim_cur as u64)
    }};
}

pub(crate) fn load_average() -> Option<f64> {
    let mut value = 0.0;
    // SAFETY: getloadavg writes one double to a valid out pointer.
    (unsafe { libc::getloadavg(&mut value, 1) } == 1).then_some(value)
}

pub(crate) fn host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is 256 bytes and gethostname writes at most that.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

fn statvfs(path: &Path) -> Option<libc::statvfs> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` is a
    // zeroed out-parameter that `statvfs` fills in.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c_path.as_ptr(), &mut stat) == 0).then_some(stat)
    }
}

#[allow(clippy::unnecessary_cast)]
pub(crate) fn disk(path: &Path) -> Option<Disk> {
    statvfs(path).map(|stat| {
        let unit = stat.f_frsize as u64;
        Disk {
            free: (stat.f_bavail as u64).saturating_mul(unit),
            total: (stat.f_blocks as u64).saturating_mul(unit),
        }
    })
}

pub(crate) fn max_processes() -> Option<u64> {
    rlimit!(libc::RLIMIT_NPROC)
}

pub(crate) fn max_open_files() -> Option<u64> {
    rlimit!(libc::RLIMIT_NOFILE)
}

pub(crate) fn uname_arch(run: Run<'_>) -> Option<String> {
    run(Tool::Uname, &["-m"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
