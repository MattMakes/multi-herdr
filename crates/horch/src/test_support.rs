use std::path::Path;

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
