//! The concern path for Linux.

pub use crate::unix::path::{
    clear_link, copy_symlink, is_reparse_point, passwd_home, private_link_home_supported,
    set_junction, symlink, CONFIG_BASE_VAR, HOME_VAR, INSTALL_BASE_VAR,
};

pub const CLAUDE_MANAGED_SETTINGS: &str = "/etc/claude-code/managed-settings.json";
pub const BLENDER_APP: Option<&str> = None;
pub const GODOT_APP: Option<&str> = None;
pub const GODOT_MONO_APP: Option<&str> = None;
