//! The concern path: home folders, fixed paths, symlinks and links (PL-2).

use horch_platform::path;

#[cfg(unix)]
#[test]
fn home_var_is_home_off_windows() {
    assert_eq!(path::HOME_VAR, "HOME");
}

#[cfg(windows)]
#[test]
fn home_var_is_userprofile_on_windows() {
    assert_eq!(path::HOME_VAR, "USERPROFILE");
}

#[cfg(unix)]
#[test]
fn passwd_home_is_absolute() {
    let home = path::passwd_home()
        .expect("unix has a passwd database")
        .expect("this user has a passwd entry");
    assert!(home.is_absolute(), "{}", home.display());
}

#[cfg(windows)]
#[test]
fn passwd_home_is_not_supported_on_windows() {
    let err = path::passwd_home().unwrap_err();
    assert_eq!(err.os, "windows");
}

#[cfg(unix)]
#[test]
fn symlink_points_at_its_target() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::write(&target, "x").unwrap();
    let at = tmp.path().join("link");
    path::symlink(&target, &at).unwrap();
    assert!(std::fs::symlink_metadata(&at)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_link(&at).unwrap(), target);
}

#[cfg(unix)]
#[test]
fn copy_symlink_copies_the_target_text() {
    let tmp = tempfile::tempdir().unwrap();
    let from = tmp.path().join("from");
    // A relative target that does not exist: the copy keeps the text as is.
    path::symlink(std::path::Path::new("../elsewhere"), &from).unwrap();
    let to = tmp.path().join("to");
    path::copy_symlink(&from, &to).unwrap();
    assert_eq!(
        std::fs::read_link(&to).unwrap(),
        std::path::Path::new("../elsewhere")
    );
}

#[cfg(windows)]
#[test]
fn copy_symlink_is_not_supported_on_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let to = tmp.path().join("to");
    path::set_junction(&tmp.path().join("from"), &target).unwrap();
    let err = path::copy_symlink(&tmp.path().join("from"), &to).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(!to.exists());
}

#[cfg(unix)]
#[test]
fn private_link_home_is_supported_off_windows() {
    assert!(path::private_link_home_supported());
}

#[cfg(unix)]
#[test]
fn junctions_are_not_supported_off_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let meta = std::fs::metadata(tmp.path()).unwrap();
    assert!(path::is_reparse_point(&meta).is_err());
    let link = tmp.path().join("link");
    let err = path::set_junction(&link, tmp.path()).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    let err = path::clear_link(&link).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    assert!(!link.exists());
}

#[cfg(windows)]
#[test]
fn set_junction_then_clear_link() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("release");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("horch.exe"), "x").unwrap();
    let link = tmp.path().join("current");
    path::set_junction(&link, &target).unwrap();
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(path::is_reparse_point(&meta).unwrap());
    assert!(link.join("horch.exe").exists());
    path::clear_link(&link).unwrap();
    assert!(std::fs::symlink_metadata(&link).is_err());
    assert!(target.join("horch.exe").exists(), "the target survives");
}

#[test]
fn junction_plan_covers_every_state() {
    use path::{plan_junction, JunctionAction};
    assert_eq!(plan_junction(false, false, false), JunctionAction::Create);
    assert_eq!(plan_junction(true, true, false), JunctionAction::Replace);
    assert_eq!(plan_junction(true, false, true), JunctionAction::Replace);
    assert_eq!(plan_junction(true, false, false), JunctionAction::Refuse);
}

#[test]
fn fixed_paths_match_today() {
    if cfg!(target_os = "macos") {
        assert_eq!(
            path::CLAUDE_MANAGED_SETTINGS,
            "/Library/Application Support/ClaudeCode/managed-settings.json"
        );
        assert_eq!(
            path::BLENDER_APP,
            Some("/Applications/Blender.app/Contents/MacOS/Blender")
        );
        assert_eq!(
            path::GODOT_APP,
            Some("/Applications/Godot.app/Contents/MacOS/Godot")
        );
        assert_eq!(
            path::GODOT_MONO_APP,
            Some("/Applications/Godot_mono.app/Contents/MacOS/Godot")
        );
    } else {
        assert_eq!(path::BLENDER_APP, None);
        assert_eq!(path::GODOT_APP, None);
        assert_eq!(path::GODOT_MONO_APP, None);
    }
    if cfg!(target_os = "linux") {
        assert_eq!(
            path::CLAUDE_MANAGED_SETTINGS,
            "/etc/claude-code/managed-settings.json"
        );
    }
    if cfg!(windows) {
        assert_eq!(
            path::CLAUDE_MANAGED_SETTINGS,
            r"C:\Program Files\ClaudeCode\managed-settings.json"
        );
        assert_eq!(path::CONFIG_BASE_VAR, Some("APPDATA"));
        assert_eq!(path::INSTALL_BASE_VAR, Some("LOCALAPPDATA"));
    } else {
        assert_eq!(path::CONFIG_BASE_VAR, None);
        assert_eq!(path::INSTALL_BASE_VAR, None);
    }
}

#[test]
fn inspect_junction_target_reads_the_path() {
    use path::{inspect_junction_target, JunctionAction};
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        inspect_junction_target(&tmp.path().join("absent")),
        JunctionAction::Create
    );
    let dir = tmp.path().join("empty");
    std::fs::create_dir(&dir).unwrap();
    assert_eq!(inspect_junction_target(&dir), JunctionAction::Replace);
    std::fs::write(dir.join("file"), "x").unwrap();
    assert_eq!(inspect_junction_target(&dir), JunctionAction::Refuse);
}
