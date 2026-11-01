//! Helpers shared by the unit tests of several modules.

use std::path::Path;

/// Write `script` to `path` as an executable and run it once with `args`,
/// untimed. macOS scans a new executable at its first exec, for seconds:
/// the warm keeps that scan out of the call under test. On Linux a test
/// thread that forks while another writes a script can hold it open for
/// writing, so exec fails with "Text file busy" until that child execs;
/// retry until it does.
pub(crate) fn write_warmed(path: &Path, script: &str, args: &[&str]) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..100 {
        match std::process::Command::new(path).args(args).output() {
            Ok(out) => return out,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("warm {}: {e}", path.display()),
        }
    }
    panic!("{} stayed busy", path.display());
}
