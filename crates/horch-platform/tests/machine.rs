//! The machine probe: load, memory, disk, the process limits and the host
//! name (PL-2), with the gap of Windows as `NotSupported` (row S12).

use horch_platform::machine::{self, Tool};

/// A fixed probe command output, so the test does not depend on the
/// commands of the host. A failed command is `None` in the `Run` contract.
#[cfg(unix)]
fn run(tool: Tool, args: &[&str]) -> Option<String> {
    let text = match (tool, args) {
        (Tool::Uname, ["-m"]) => " arm64\n",
        (Tool::Sysctl, ["-n", "hw.memsize"]) => "4096\n",
        (Tool::Sysctl, ["-n", "machdep.cpu.brand_string"]) => "Apple M2\n",
        (Tool::VmStat, []) => {
            "Mach Virtual Memory Statistics: (page size of 4 bytes)\nPages free: 8.\n"
        }
        (Tool::NvidiaSmi, ["-L"]) => "GPU 0: A\nGPU 1: B\n",
        _ => return None,
    };
    Some(text.to_string())
}

#[cfg(unix)]
#[test]
fn load_memory_disk_and_limit_are_known() {
    let load = machine::load_average().unwrap();
    assert!(load.is_some_and(|l| l >= 0.0), "{load:?}");

    // macOS reads the fixed command output, and Linux reads /proc/meminfo.
    let memory = machine::memory(&run).unwrap();
    let total = memory.total.expect("total memory");
    let available = memory.available.expect("available memory");
    assert!(total > 0 && available > 0, "{memory:?}");
    if cfg!(target_os = "macos") {
        assert_eq!((total, available), (4096, 32));
    }

    let dir = tempfile::tempdir().unwrap();
    let disk = machine::disk(dir.path()).unwrap().expect("disk");
    assert!(disk.total > 0 && disk.free <= disk.total, "{disk:?}");
    assert_eq!(
        machine::disk(&dir.path().join("missing")).unwrap(),
        None,
        "statvfs of a missing path gives no reading"
    );

    assert!(machine::max_processes().is_ok());
    assert!(machine::max_open_files().is_ok());
    let arch = machine::uname_arch(&run).unwrap();
    assert_eq!(arch.as_deref(), Some("arm64"));
    let gpu = machine::gpu(&run, "arm64").unwrap();
    let expected = if cfg!(target_os = "macos") {
        machine::Gpu::AppleSilicon
    } else {
        machine::Gpu::Nvidia { count: 2 }
    };
    assert_eq!(gpu, expected);
    let failed = |_: Tool, _: &[&str]| -> Option<String> { None };
    assert_eq!(machine::uname_arch(&failed).unwrap(), None);
    let host = machine::host_name().unwrap().expect("host name");
    assert!(!host.contains('\0'), "{host:?}");
}

#[cfg(windows)]
#[test]
fn probe_is_not_supported_on_windows() {
    let run = |_: Tool, _: &[&str]| -> Option<String> { panic!("Windows runs no probe command") };
    let dir = std::env::temp_dir();
    assert!(machine::load_average().is_err());
    assert!(machine::host_name().is_err());
    assert!(machine::memory(&run).is_err());
    assert!(machine::disk(&dir).is_err());
    assert!(machine::max_processes().is_err());
    assert!(machine::max_open_files().is_err());
    assert!(machine::gpu(&run, "x86_64").is_err());
    let err = machine::uname_arch(&run).unwrap_err();
    assert_eq!(err.concern, "machine");
    assert_eq!(err.os, "windows");
}

#[test]
fn parse_vm_stat_and_parse_meminfo_read_a_sample() {
    let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
              Pages free:                               10.\n\
              Pages active:                             99.\n\
              Pages inactive:                           20.\n\
              Pages speculative:                         5.\n";
    assert_eq!(machine::parse_vm_stat(vm), Some(35 * 16384));
    assert_eq!(machine::parse_vm_stat("no header"), None);

    let mi = "MemTotal:        2048 kB\nMemFree:  x kB\nMemAvailable:    1024 kB\n";
    assert_eq!(machine::parse_meminfo(mi, "MemTotal"), Some(2048 * 1024));
    assert_eq!(
        machine::parse_meminfo(mi, "MemAvailable"),
        Some(1024 * 1024)
    );
    assert_eq!(machine::parse_meminfo(mi, "MemFree"), None);
    assert_eq!(machine::parse_meminfo(mi, "Mem"), None);
}
