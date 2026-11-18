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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineSnapshot {
    /// `"macos"`, `"linux"`, `"windows"` or another `std::env::consts::OS` value.
    pub os: String,
    pub arch: String,
    pub cpus: Known<u32>,
    /// One minute host load; absent on platforms without getloadavg.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_average_1m: Option<f64>,
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
    let parts = probe_parts(path, bins);
    MachineSnapshot {
        os: os.to_string(),
        arch: parts.arch,
        cpus: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32)
            .into(),
        load_average_1m: horch_platform::machine::load_average().ok().flatten(),
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
        load_average_1m: None,
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

use horch_platform::machine::{Gpu, Tool};

const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

fn probe_parts(path: &Path, bins: &ProbeBins) -> Parts {
    let run_tool = |tool: Tool, args: &[&str]| {
        let bin = match tool {
            Tool::Uname => &bins.uname,
            Tool::Sysctl => &bins.sysctl,
            Tool::VmStat => &bins.vm_stat,
            Tool::NvidiaSmi => &bins.nvidia_smi,
        };
        run(bin, args)
    };
    let arch = horch_platform::machine::uname_arch(&run_tool)
        .ok()
        .flatten()
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
    let disk = horch_platform::machine::disk(path).ok().flatten();
    let memory = horch_platform::machine::memory(&run_tool).ok();
    let gpu = match horch_platform::machine::gpu(&run_tool, &arch) {
        Ok(Gpu::AppleSilicon) => GpuClass::AppleSilicon,
        Ok(Gpu::Nvidia { count }) => GpuClass::Nvidia { count },
        Ok(Gpu::None) => GpuClass::None,
        Ok(Gpu::Unknown) | Err(_) => GpuClass::Unknown,
    };
    Parts {
        arch,
        mem_total: memory.and_then(|m| m.total),
        mem_available: memory.and_then(|m| m.available),
        disk_free: disk.map(|d| d.free),
        disk_total: disk.map(|d| d.total),
        gpu,
        max_open_files: horch_platform::machine::max_open_files().ok().flatten(),
        max_processes: horch_platform::machine::max_processes().ok().flatten(),
    }
}

/// Run a command with a 2 s timeout, through the platform start function,
/// which removes each forbidden name. Return stdout on exit code 0. Any
/// failure gives `None`.
fn run(bin: &Path, args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = horch_platform::start::command(bin)
        .args(args)
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
    let deadline = std::time::Instant::now() + COMMAND_TIMEOUT;
    let status = match crate::runtime::process::wait_until(
        &mut child,
        deadline,
        std::time::Duration::from_millis(10),
    ) {
        Ok(Some(status)) => Some(status),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    };
    let text = reader.join().ok()?;
    status.filter(|s| s.success()).map(|_| text)
}

/// The OS host name (gethostname), trimmed, if it has one.
fn os_host_name() -> Option<String> {
    let name = horch_platform::machine::host_name().ok().flatten()?;
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// This machine's name: `$HORCH_HOSTNAME` (tests), else the OS host name,
/// else `COMPUTERNAME` or `HOSTNAME`, else `localhost`. The task ledger
/// records it beside every PID: a PID names a process only on its host.
pub fn host_name() -> String {
    env_nonempty("HORCH_HOSTNAME")
        .or_else(os_host_name)
        .or_else(|| {
            ["COMPUTERNAME", "HOSTNAME"]
                .into_iter()
                .find_map(env_nonempty)
        })
        .unwrap_or(String::from("localhost"))
}

/// The value of an environment variable, or `None` when it is unset or empty.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
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
        assert!(
            !code.contains("libc::"),
            "libc:: in the probe: the OS calls belong to horch_platform::machine"
        );
        let map_start = code
            .find("let bin = match tool {")
            .expect("the map from a probe tool to its command");
        let map_end = map_start + code[map_start..].find("};").expect("end of the map");
        for command in [
            "bins.sysctl",
            "bins.vm_stat",
            "bins.nvidia_smi",
            "bins.uname",
        ] {
            for (offset, _) in code.match_indices(command) {
                assert!(
                    (map_start..map_end).contains(&offset),
                    "{command} used outside the map of probe tools at byte {offset}"
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

    /// A whole snapshot parses; the same snapshot with one extra key does not.
    /// A partial snapshot would fail on its missing fields either way.
    #[test]
    fn machine_fixture_rejects_unknown_fields() {
        let text = std::fs::read_to_string(fixture_dir().join("mac-m5-128g.json")).unwrap();
        serde_json::from_str::<MachineSnapshot>(&text).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["bogus"] = serde_json::json!(1);
        let err = serde_json::from_value::<MachineSnapshot>(value).unwrap_err();
        assert!(err.to_string().contains("unknown field `bogus`"), "{err}");
    }

    #[test]
    fn machine_parsers() {
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                  Pages free:                               10.\n\
                  Pages inactive:                           20.\n\
                  Pages speculative:                         5.\n";
        assert_eq!(horch_platform::machine::parse_vm_stat(vm), Some(35 * 16384));
        let mi = "MemTotal:       2048 kB\nMemAvailable:   1024 kB\n";
        assert_eq!(
            horch_platform::machine::parse_meminfo(mi, "MemTotal"),
            Some(2048 * 1024)
        );
        assert_eq!(
            horch_platform::machine::parse_meminfo(mi, "MemAvailable"),
            Some(1024 * 1024)
        );
        assert_eq!(horch_platform::machine::parse_meminfo(mi, "MemFree"), None);
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
