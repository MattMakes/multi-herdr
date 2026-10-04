//! Machine probe for preflight: disk, memory, CPU, GPU class and process limits.
//!
//! Every probe that fails reports `Unknown`; nothing here returns an error.
//! The fixture path is a parameter, so this module never reads the environment.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A measured value, or `Unknown` when the probe could not read it.
/// Serialized as the value or `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    from = "Option<T>",
    into = "Option<T>",
    bound(
        serialize = "T: Clone + Serialize",
        deserialize = "T: Deserialize<'de>"
    )
)]
pub enum Known<T> {
    Known(T),
    Unknown,
}

impl<T> From<Option<T>> for Known<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Known::Unknown, Known::Known)
    }
}

impl<T> From<Known<T>> for Option<T> {
    fn from(value: Known<T>) -> Self {
        match value {
            Known::Known(v) => Some(v),
            Known::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuClass {
    AppleSilicon,
    Nvidia { count: u32 },
    None,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineSnapshot {
    /// `"macos"`, `"linux"`, `"windows"` or another `std::env::consts::OS` value.
    pub os: String,
    pub arch: String,
    pub cpus: Known<u32>,
    pub mem_total_bytes: Known<u64>,
    pub mem_available_bytes: Known<u64>,
    pub disk_free_bytes: Known<u64>,
    pub disk_total_bytes: Known<u64>,
    pub gpu: GpuClass,
    pub max_open_files: Known<u64>,
    pub max_processes: Known<u64>,
}

/// Paths of the external commands the probe runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeBins {
    pub sysctl: PathBuf,
    pub vm_stat: PathBuf,
    pub uname: PathBuf,
    pub nvidia_smi: PathBuf,
}

impl Default for ProbeBins {
    fn default() -> Self {
        Self {
            sysctl: "sysctl".into(),
            vm_stat: "vm_stat".into(),
            uname: "uname".into(),
            nvidia_smi: "nvidia-smi".into(),
        }
    }
}

/// Probe the machine. `path` is the directory whose disk is measured.
/// When `fixture` is `Some`, read the snapshot from that JSON file instead.
pub fn probe(path: &Path, bins: &ProbeBins, fixture: Option<&Path>) -> MachineSnapshot {
    if let Some(file) = fixture {
        // A fixture that cannot be read never falls back to a live probe.
        return std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<MachineSnapshot>(&text).ok())
            .unwrap_or_else(all_unknown);
    }
    let os = std::env::consts::OS;
    let parts = sys::probe_parts(os, path, bins);
    MachineSnapshot {
        os: os.to_string(),
        arch: parts.arch,
        cpus: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32)
            .into(),
        mem_total_bytes: parts.mem_total.into(),
        mem_available_bytes: parts.mem_available.into(),
        disk_free_bytes: parts.disk_free.into(),
        disk_total_bytes: parts.disk_total.into(),
        gpu: parts.gpu,
        max_open_files: parts.max_open_files.into(),
        max_processes: parts.max_processes.into(),
    }
}

fn all_unknown() -> MachineSnapshot {
    MachineSnapshot {
        os: "unknown".to_string(),
        arch: "unknown".to_string(),
        cpus: Known::Unknown,
        mem_total_bytes: Known::Unknown,
        mem_available_bytes: Known::Unknown,
        disk_free_bytes: Known::Unknown,
        disk_total_bytes: Known::Unknown,
        gpu: GpuClass::Unknown,
        max_open_files: Known::Unknown,
        max_processes: Known::Unknown,
    }
}

struct Parts {
    arch: String,
    mem_total: Option<u64>,
    mem_available: Option<u64>,
    disk_free: Option<u64>,
    disk_total: Option<u64>,
    gpu: GpuClass,
    max_open_files: Option<u64>,
    max_processes: Option<u64>,
}

#[cfg(unix)]
mod sys {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{GpuClass, Parts, ProbeBins};

    const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

    // `getrlimit` takes a platform-specific integer type for the resource, so a
    // macro keeps each call site on its own `libc` constant.
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

    pub(super) fn probe_parts(os: &str, path: &Path, bins: &ProbeBins) -> Parts {
        let arch = run(&bins.uname, &["-m"])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::consts::ARCH.to_string());
        let (disk_free, disk_total) = disk(path);
        let (mem_total, mem_available, gpu) = match os {
            "macos" => macos(&arch, bins),
            "linux" => linux(bins),
            _ => (None, None, GpuClass::Unknown),
        };
        Parts {
            arch,
            mem_total,
            mem_available,
            disk_free,
            disk_total,
            gpu,
            max_open_files: rlimit!(libc::RLIMIT_NOFILE),
            max_processes: max_processes(),
        }
    }

    /// Run a command with a 2 s timeout and `ANTHROPIC_API_KEY` removed.
    /// Return stdout on exit code 0. Any failure gives `None`.
    fn run(bin: &Path, args: &[&str]) -> Option<String> {
        let mut child = Command::new(bin)
            .args(args)
            .env_remove("ANTHROPIC_API_KEY")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() < COMMAND_TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let text = reader.join().ok()?;
        status.filter(|s| s.success()).map(|_| text)
    }

    fn macos(arch: &str, bins: &ProbeBins) -> (Option<u64>, Option<u64>, GpuClass) {
        let total = run(&bins.sysctl, &["-n", "hw.memsize"]).and_then(|s| s.trim().parse().ok());
        let available = run(&bins.vm_stat, &[]).and_then(|s| super::parse_vm_stat(&s));
        let brand = run(&bins.sysctl, &["-n", "machdep.cpu.brand_string"]);
        let gpu = match brand {
            Some(b) if arch == "arm64" && b.trim_start().starts_with("Apple") => {
                GpuClass::AppleSilicon
            }
            Some(_) => GpuClass::None,
            None => GpuClass::Unknown,
        };
        (total, available, gpu)
    }

    fn linux(bins: &ProbeBins) -> (Option<u64>, Option<u64>, GpuClass) {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
        let total = meminfo
            .as_deref()
            .and_then(|t| super::parse_meminfo(t, "MemTotal"));
        let available = meminfo
            .as_deref()
            .and_then(|t| super::parse_meminfo(t, "MemAvailable"));
        let gpu = match run(&bins.nvidia_smi, &["-L"]) {
            Some(list) => {
                let count = list.lines().filter(|l| l.starts_with("GPU ")).count() as u32;
                if count > 0 {
                    GpuClass::Nvidia { count }
                } else {
                    GpuClass::None
                }
            }
            None => GpuClass::None,
        };
        (total, available, gpu)
    }

    fn disk(path: &Path) -> (Option<u64>, Option<u64>) {
        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            return (None, None);
        };
        // SAFETY: `c_path` is a valid NUL-terminated string and `stat` is a
        // zeroed out-parameter that `statvfs` fills in.
        let stat = unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
                return (None, None);
            }
            stat
        };
        let unit = stat.f_frsize as u64;
        (
            Some((stat.f_bavail as u64).saturating_mul(unit)),
            Some((stat.f_blocks as u64).saturating_mul(unit)),
        )
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn max_processes() -> Option<u64> {
        rlimit!(libc::RLIMIT_NPROC)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn max_processes() -> Option<u64> {
        None
    }
}

#[cfg(not(unix))]
mod sys {
    use std::path::Path;

    use super::{GpuClass, Parts, ProbeBins};

    pub(super) fn probe_parts(_os: &str, _path: &Path, _bins: &ProbeBins) -> Parts {
        Parts {
            arch: std::env::consts::ARCH.to_string(),
            mem_total: None,
            mem_available: None,
            disk_free: None,
            disk_total: None,
            gpu: GpuClass::Unknown,
            max_open_files: None,
            max_processes: None,
        }
    }
}

/// Parse `vm_stat` output: (free + inactive + speculative pages) x page size.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_vm_stat(text: &str) -> Option<u64> {
    let header = text.lines().next()?;
    let page_size: u64 = header
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |key: &str| -> u64 {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.trim().trim_end_matches('.').parse().ok())
            .unwrap_or(0)
    };
    let total = pages("Pages free:") + pages("Pages inactive:") + pages("Pages speculative:");
    Some(total * page_size)
}

/// Parse one `/proc/meminfo` key in kB and return bytes.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_meminfo(text: &str, key: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
    let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/machine")
    }

    #[test]
    fn nfr_10_machine_probe_cfg_paths() {
        let source = include_str!("machine.rs");
        let code = source.split("#[cfg(test)]").next().unwrap();
        let unix_start = code
            .find("#[cfg(unix)]\nmod sys {")
            .expect("unix sys module");
        let other_start = code
            .find("#[cfg(not(unix))]\nmod sys {")
            .expect("non-unix sys module");
        assert!(unix_start < other_start);
        for (offset, line) in code.match_indices("libc::") {
            assert!(
                (unix_start..other_start).contains(&offset),
                "libc:: use outside the cfg(unix) module at byte {offset} ({line})"
            );
        }
        for command in [
            "bins.sysctl",
            "bins.vm_stat",
            "bins.nvidia_smi",
            "bins.uname",
        ] {
            for (offset, _) in code.match_indices(command) {
                assert!(
                    (unix_start..other_start).contains(&offset),
                    "{command} used outside the cfg(unix) module at byte {offset}"
                );
            }
        }
    }

    #[test]
    fn machine_fixture_roundtrip() {
        let mac = probe(
            Path::new("."),
            &ProbeBins::default(),
            Some(&fixture_dir().join("mac-m5-128g.json")),
        );
        assert_eq!(mac.os, "macos");
        assert_eq!(mac.arch, "arm64");
        assert_eq!(mac.cpus, Known::Known(18));
        assert_eq!(mac.mem_total_bytes, Known::Known(137_438_953_472));
        assert_eq!(mac.gpu, GpuClass::AppleSilicon);
        assert_eq!(mac.max_processes, Known::Known(8000));
        let again: MachineSnapshot =
            serde_json::from_str(&serde_json::to_string(&mac).unwrap()).unwrap();
        assert_eq!(again, mac);

        let linux = probe(
            Path::new("."),
            &ProbeBins::default(),
            Some(&fixture_dir().join("linux-nvidia-2gpu.json")),
        );
        assert_eq!(linux.os, "linux");
        assert_eq!(linux.gpu, GpuClass::Nvidia { count: 2 });
        assert_eq!(linux.max_processes, Known::Unknown);
        assert_eq!(linux.disk_free_bytes, Known::Known(1_500_000_000_000));
        let again: MachineSnapshot =
            serde_json::from_str(&serde_json::to_string(&linux).unwrap()).unwrap();
        assert_eq!(again, linux);
    }

    #[test]
    fn machine_bad_fixture_is_all_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, r#"{"os":"linux","bogus":1}"#).unwrap();
        let missing = dir.path().join("missing.json");
        for fixture in [&bad, &missing] {
            let snap = probe(dir.path(), &ProbeBins::default(), Some(fixture));
            assert_eq!(snap, all_unknown());
            assert_eq!(snap.os, "unknown");
            assert_eq!(snap.arch, "unknown");
        }
    }

    #[test]
    fn machine_fixture_rejects_unknown_fields() {
        let text = r#"{"os":"linux","bogus":1}"#;
        assert!(serde_json::from_str::<MachineSnapshot>(text).is_err());
    }

    #[test]
    fn machine_parsers() {
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                  Pages free:                               10.\n\
                  Pages inactive:                           20.\n\
                  Pages speculative:                         5.\n";
        assert_eq!(parse_vm_stat(vm), Some(35 * 16384));
        let mi = "MemTotal:       2048 kB\nMemAvailable:   1024 kB\n";
        assert_eq!(parse_meminfo(mi, "MemTotal"), Some(2048 * 1024));
        assert_eq!(parse_meminfo(mi, "MemAvailable"), Some(1024 * 1024));
        assert_eq!(parse_meminfo(mi, "MemFree"), None);
    }

    #[cfg(unix)]
    #[test]
    fn machine_probe_live_is_sane() {
        let dir = tempfile::tempdir().unwrap();
        let snap = probe(dir.path(), &ProbeBins::default(), None);
        if let Known::Known(cpus) = snap.cpus {
            assert!(cpus >= 1);
        }
        if let (Known::Known(free), Known::Known(total)) =
            (snap.disk_free_bytes, snap.disk_total_bytes)
        {
            assert!(free <= total);
        }
        assert!(!snap.os.is_empty());
        assert!(!snap.arch.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn machine_probe_missing_commands_give_unknown() {
        let missing = Path::new("/nonexistent/bin");
        let bins = ProbeBins {
            sysctl: missing.into(),
            vm_stat: missing.into(),
            uname: missing.into(),
            nvidia_smi: missing.into(),
        };
        let snap = probe(Path::new("/nonexistent/dir"), &bins, None);
        assert_eq!(snap.disk_free_bytes, Known::Unknown);
        assert_eq!(snap.arch, std::env::consts::ARCH);
    }
}
