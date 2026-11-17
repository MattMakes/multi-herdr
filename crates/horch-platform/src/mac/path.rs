//! The concern path for macOS.

pub use crate::unix::path::{
    clear_link, copy_symlink, is_reparse_point, passwd_home, private_link_home_supported,
    set_junction, symlink, CONFIG_BASE_VAR, HOME_VAR, INSTALL_BASE_VAR,
};

pub const CLAUDE_MANAGED_SETTINGS: &str =
    "/Library/Application Support/ClaudeCode/managed-settings.json";
pub const BLENDER_APP: Option<&str> = Some("/Applications/Blender.app/Contents/MacOS/Blender");
pub const GODOT_APP: Option<&str> = Some("/Applications/Godot.app/Contents/MacOS/Godot");
pub const GODOT_MONO_APP: Option<&str> = Some("/Applications/Godot_mono.app/Contents/MacOS/Godot");
