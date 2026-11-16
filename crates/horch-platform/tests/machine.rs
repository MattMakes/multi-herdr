//! The machine probe: load, memory, disk, the process limits and the host
//! name (PL-2), with the gap of Windows as `NotSupported` (row S12).

use horch_platform::machine::{self, Tool};

/// Runs a probe command as `runtime/machine.rs` does, without its timeout.
#[cfg(unix)]
fn run(tool: Tool, args: &[&str]) -> Option<String> {
    let bin = match tool {
        Tool::Uname => "uname",
        Tool::Sysctl => "sysctl",
        Tool::VmStat => "vm_stat",
        Tool::NvidiaSmi => "nvidia-smi",
    };
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(unix)]
#[test]
fn load_memory_disk_and_limit_are_known() {
    let load = machine::load_average().unwrap();
    assert!(load.is_some_and(|l| l >= 0.0), "{load:?}");

    let memory = machine::memory(&run).unwrap();
    let total = memory.total.expect("total memory");
    let available = memory.available.expect("available memory");
    assert!(total > 0 && available > 0, "{memory:?}");

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
    let arch = machine::uname_arch(&run).unwrap().expect("uname -m");
    assert!(!arch.is_empty() && arch == arch.trim(), "{arch:?}");
    assert_ne!(machine::gpu(&run, &arch).unwrap(), machine::Gpu::Unknown);
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
