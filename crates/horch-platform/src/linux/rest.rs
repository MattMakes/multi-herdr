pub(crate) use crate::unix::rest::{
    ensure_user_path, extract_release, path_extensions, path_help, replace_process,
    RELEASE_ARCHIVE_SUFFIX, RUNNING_EXE_IS_LOCKED, SHELL_LAUNCHERS, STATUS_HAS_START_TIMES,
    USER_PATH_IN_REGISTRY,
};

pub(crate) const HOST_TOOLS: &str = "GNU tools; check availability before using timeout or sed -i";
