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

#[test]
fn plain_of_verbatim_drops_the_prefix_of_a_drive_path() {
    assert_eq!(
        path::plain_of_verbatim(r"\\?\C:\Users\a\gate-F-0001").as_deref(),
        Some(r"C:\Users\a\gate-F-0001")
    );
    assert_eq!(path::plain_of_verbatim(r"\\?\d:\").as_deref(), Some(r"d:\"));
    assert_eq!(
        path::plain_of_verbatim(r"\\?\C:\dir\").as_deref(),
        Some(r"C:\dir\")
    );
}

#[test]
fn plain_of_verbatim_gives_a_unc_path_its_two_slashes() {
    assert_eq!(
        path::plain_of_verbatim(r"\\?\UNC\server\share\dir").as_deref(),
        Some(r"\\server\share\dir")
    );
    assert_eq!(path::plain_of_verbatim(r"\\?\UNC\server"), None);
    assert_eq!(path::plain_of_verbatim(r"\\?\UNC\"), None);
}

#[test]
fn plain_of_verbatim_leaves_a_path_with_no_prefix() {
    for text in [r"C:\dir", r"\\server\share", "/tmp/a", r"\\.\pipe\x", ""] {
        assert_eq!(path::plain_of_verbatim(text), None, "{text}");
    }
    assert_eq!(path::plain_of_verbatim(r"\\?\Volume{1234}\dir"), None);
    assert_eq!(path::plain_of_verbatim(r"\\?\C:"), None);
}

#[test]
fn plain_of_verbatim_keeps_the_prefix_of_a_path_at_the_legacy_limit() {
    let at_limit = format!(r"\\?\C:\{}", "a".repeat(260 - 3));
    assert_eq!(path::plain_of_verbatim(&at_limit), None);
    let below = format!(r"\\?\C:\{}", "a".repeat(259 - 3));
    assert_eq!(path::plain_of_verbatim(&below).map(|p| p.len()), Some(259));
}

#[test]
fn plain_of_verbatim_keeps_the_prefix_of_a_name_that_the_plain_form_changes() {
    for name in [
        "NUL",
        "con",
        "Aux.txt",
        "COM1",
        "lpt9.log",
        "COM\u{b9}",
        "CONIN$",
        "nul .txt",
        "dir.",
        "dir ",
        ".",
        "..",
        "a/b",
        "a:b",
        "a*b",
        "a?b",
        "a\u{1}b",
    ] {
        let text = format!(r"\\?\C:\top\{name}\end");
        assert_eq!(path::plain_of_verbatim(&text), None, "{name}");
    }
    assert_eq!(path::plain_of_verbatim(r"\\?\C:\a\\b"), None);
    for name in ["CONSOLE", "nulls", "COM10", "a.b", ".git", "a b"] {
        let text = format!(r"\\?\C:\top\{name}");
        assert!(path::plain_of_verbatim(&text).is_some(), "{name}");
    }
}

#[cfg(unix)]
#[test]
fn for_child_leaves_a_unix_path() {
    let p = std::path::Path::new(r"/tmp/\\?\C:\x");
    assert_eq!(path::for_child(p), p);
}

#[cfg(windows)]
#[test]
fn for_child_gives_a_canonical_folder_in_its_plain_form() {
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let plain = path::for_child(&temp);
    assert!(
        !plain.to_string_lossy().starts_with(r"\\?\"),
        "{}",
        plain.display()
    );
    assert_eq!(plain.canonicalize().unwrap(), temp);
}

#[test]
fn a_started_command_gets_each_verbatim_path_in_its_plain_form() {
    let mut cmd = horch_platform::start::command("git");
    cmd.arg(r"\\?\C:\a")
        .args(["-C", r"\\?\UNC\s\h\b", "--detach"])
        .current_dir(r"\\?\C:\work");
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let dir = cmd
        .get_current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        assert_eq!(args, [r"C:\a", "-C", r"\\s\h\b", "--detach"]);
        assert_eq!(dir, r"C:\work");
    } else {
        assert_eq!(args, [r"\\?\C:\a", "-C", r"\\?\UNC\s\h\b", "--detach"]);
        assert_eq!(dir, r"\\?\C:\work");
    }
}
