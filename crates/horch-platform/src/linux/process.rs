use crate::NotSupported;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use crate::unix::process::{alive, group_of, session_of};

pub(crate) fn start_time(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&stat)
}

pub(crate) fn parse_stat(stat: &str) -> Option<u64> {
    stat_field(stat, 22)
}

pub(crate) fn stat_field<T: std::str::FromStr>(stat: &str, n: usize) -> Option<T> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(n - 3)?.parse().ok()
}

fn clock_hz() -> Option<u64> {
    // SAFETY: sysconf reads a constant.
    u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()
}

pub(crate) fn started_at(pid: i32) -> Option<SystemTime> {
    let ticks = start_time(pid)?;
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let boot: u64 = stat
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let hz = clock_hz()?;
    if hz == 0 {
        return None;
    }
    let since_boot = Duration::from_millis(ticks * 1000 / hz);
    Some(UNIX_EPOCH + Duration::from_secs(boot) + since_boot)
}

pub(crate) fn started_by(pid: i32, t: SystemTime) -> Option<bool> {
    let ticks = start_time(pid)?;
    let hz = clock_hz()?;
    // SAFETY: timespec is plain integers; all-zero bytes are a valid value.
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: `ts` is a valid timespec to fill.
    if hz == 0 || unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
        return None;
    }
    let boot_now = Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
    let now = SystemTime::now();
    let t_boot = match now.duration_since(t) {
        Ok(ago) => boot_now.checked_sub(ago)?,
        Err(ahead) => boot_now + ahead.duration(),
    };
    let start = Duration::from_nanos(ticks.checked_mul(1_000_000_000)? / hz);
    Some(start <= t_boot)
}

pub(crate) fn parent(pid: u32) -> Result<Option<u32>, NotSupported> {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return Ok(None);
    };
    Ok(stat_field(&stat, 4))
}

pub(crate) fn group_members(pgid: i32) -> Result<Vec<u32>, NotSupported> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Ok(Vec::new());
    };
    Ok(dir
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| stat_field::<i32>(&stat, 5))
                == Some(pgid)
        })
        .collect())
}

pub(crate) fn cwd_pids_under(dir: &Path) -> Result<io::Result<Vec<u32>>, NotSupported> {
    Ok(cwd_pids(dir))
}

fn cwd_pids(dir: &Path) -> io::Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if std::fs::read_link(entry.path().join("cwd")).is_ok_and(|c| c.starts_with(dir)) {
            pids.push(pid);
        }
    }
    Ok(pids)
}
