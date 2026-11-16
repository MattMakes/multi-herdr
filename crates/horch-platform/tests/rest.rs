//! The small rest (PL-2): the release archive, the restart, the user PATH,
//! the executable extensions, the install facts and the host tools line.

use std::path::Path;

use horch_platform::rest;

/// Builds a release archive of the host format with 1 file `horch`.
#[cfg(unix)]
fn archive(name: &str, body: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, name, body).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

#[cfg(windows)]
fn archive(name: &str, body: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(body).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn extract_release_unpacks_the_archive_of_this_os() {
    let dir = tempfile::tempdir().unwrap();
    rest::extract_release(&archive("horch", b"binary"), dir.path()).unwrap();
    let out = dir.path().join("horch");
    assert_eq!(std::fs::read(&out).unwrap(), b"binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let expected = if cfg!(windows) { "zip" } else { "tar.gz" };
    assert_eq!(rest::RELEASE_ARCHIVE_SUFFIX, expected);
}

#[test]
fn extract_release_refuses_a_path_and_a_bad_archive() {
    let dir = tempfile::tempdir().unwrap();
    let err = rest::extract_release(b"not an archive", dir.path()).unwrap_err();
    let kind = if cfg!(windows) { "zip" } else { "tar.gz" };
    assert!(
        err.to_string().contains(kind) || err.to_string().contains("reading the archive"),
        "{err}"
    );
    #[cfg(unix)]
    {
        // `tar::Builder` refuses `..` itself, so the path test uses a separator.
        let err = rest::extract_release(&archive("sub/horch", b"x"), dir.path()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the release archive holds an unexpected entry \"sub/horch\""
        );
    }
}

#[cfg(unix)]
#[test]
fn replace_process_gives_the_error_of_a_start_that_failed() {
    let err = rest::replace_process(std::process::Command::new("/no/such/horch"));
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn path_extensions_are_empty_off_windows() {
    assert!(rest::path_extensions(None).is_empty());
    assert!(rest::path_extensions(Some(".EXE;.CMD")).is_empty());
}

#[cfg(windows)]
#[test]
fn path_extensions_follow_pathext_on_windows() {
    assert_eq!(
        rest::path_extensions(None),
        [".exe", ".cmd", ".bat", ".com"]
    );
    assert_eq!(rest::path_extensions(Some(".PY;;.EXE")), [".py", ".exe"]);
}

#[cfg(unix)]
#[test]
fn ensure_user_path_is_not_supported_off_windows() {
    let prepend = |_: &str, _: &str| -> Option<String> { panic!("no registry here") };
    let err = rest::ensure_user_path(Path::new("/x"), &prepend).unwrap_err();
    assert_eq!(err.concern, "rest");
}

#[test]
fn install_facts_match_today() {
    assert_eq!(rest::RUNNING_EXE_IS_LOCKED, cfg!(windows));
    assert_eq!(rest::SHELL_LAUNCHERS, cfg!(unix));
    assert_eq!(rest::USER_PATH_IN_REGISTRY, cfg!(windows));
    assert_eq!(rest::STATUS_HAS_START_TIMES, !cfg!(windows));
    assert_eq!(
        rest::exe_name("herdr"),
        format!("herdr{}", std::env::consts::EXE_SUFFIX)
    );
    let help = rest::path_help(Path::new("/opt/bin"));
    if cfg!(windows) {
        assert!(
            help.contains("$env:Path = \"/opt/bin;$env:Path\""),
            "{help}"
        );
        assert!(help.contains("setx PATH"), "{help}");
    } else {
        assert_eq!(
            help,
            "  Add to ~/.zshrc (or ~/.bashrc):\n    export PATH=\"/opt/bin:$PATH\""
        );
    }
}

#[test]
fn host_tools_line_matches_today() {
    let expected = if cfg!(target_os = "macos") {
        "BSD tools; no GNU timeout; sed -i requires a backup suffix"
    } else if cfg!(target_os = "linux") {
        "GNU tools; check availability before using timeout or sed -i"
    } else {
        "host tools vary; check command support before use"
    };
    assert_eq!(rest::HOST_TOOLS, expected);
}
