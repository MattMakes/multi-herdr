use std::path::{Path, PathBuf};

pub use crate::os::path::HOME_VAR;

pub use crate::os::path::CLAUDE_MANAGED_SETTINGS;

pub use crate::os::path::{BLENDER_APP, GODOT_APP, GODOT_MONO_APP};

pub use crate::os::path::CONFIG_BASE_VAR;

pub use crate::os::path::INSTALL_BASE_VAR;

pub use crate::os::path::passwd_home;

pub use crate::os::path::symlink;

pub use crate::os::path::copy_symlink;

pub use crate::os::path::private_link_home_supported;

pub use crate::os::path::is_reparse_point;

pub use crate::os::path::set_junction;

pub use crate::os::path::clear_link;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionAction {
    Create,
    Replace,
    Refuse,
}

pub fn plan_junction(exists: bool, is_link: bool, is_empty_dir: bool) -> JunctionAction {
    match (exists, is_link, is_empty_dir) {
        (false, _, _) => JunctionAction::Create,
        (true, true, _) => JunctionAction::Replace,
        (true, false, true) => JunctionAction::Replace,
        (true, false, false) => JunctionAction::Refuse,
    }
}

pub fn inspect_junction_target(path: &Path) -> JunctionAction {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return JunctionAction::Create;
    };
    let is_link = meta.file_type().is_symlink() || is_reparse_point(&meta).unwrap_or(false);
    let is_empty_dir = meta.is_dir()
        && std::fs::read_dir(path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
    plan_junction(true, is_link, is_empty_dir)
}

pub fn for_child(path: &Path) -> PathBuf {
    PathBuf::from(crate::os::path::for_child(path.as_os_str()))
}

pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(|real| for_child(&real))
}

pub fn plain_of_verbatim(text: &str) -> Option<String> {
    let (plain, rest, least) = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        (format!(r"\\{unc}"), unc, 2)
    } else {
        let local = text.strip_prefix(r"\\?\")?;
        let drive = local.as_bytes();
        if drive.len() < 3 || !drive[0].is_ascii_alphabetic() || &drive[1..3] != b":\\" {
            return None;
        }
        (local.to_string(), &local[3..], 0)
    };
    if plain.encode_utf16().count() >= LEGACY_MAX_PATH {
        return None;
    }
    let rest = rest.strip_suffix('\\').unwrap_or(rest);
    let names: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split('\\').collect()
    };
    (names.len() >= least && names.into_iter().all(plain_component)).then_some(plain)
}

const LEGACY_MAX_PATH: usize = 260;

const RESERVED_NAMES: [&str; 30] = [
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
];

fn plain_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.ends_with(['.', ' ']) {
        return false;
    }
    if name
        .chars()
        .any(|c| c < ' ' || matches!(c, '/' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    !RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_gives_the_child_form_of_the_real_path() {
        let temp = std::env::temp_dir();
        let real = std::fs::canonicalize(&temp).unwrap();
        let got = canonicalize(&temp).unwrap();
        assert_eq!(got, for_child(&real));
        assert!(!got.to_string_lossy().starts_with(r"\\?\"), "{got:?}");
        assert_eq!(std::fs::canonicalize(&got).unwrap(), real);
    }
}
