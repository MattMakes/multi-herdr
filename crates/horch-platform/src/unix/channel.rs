use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::channel::{AtName, User};

pub(crate) type Stream = UnixStream;
pub(crate) type Listener = UnixListener;

pub(crate) fn bind(name: &Path) -> io::Result<Listener> {
    let folder = match name.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    owner_only_folder(folder)?;
    let listener = UnixListener::bind(name)?;
    if let Err(error) = fs::set_permissions(name, Permissions::from_mode(0o600)) {
        let _ = fs::remove_file(name);
        return Err(error);
    }
    Ok(listener)
}

fn owner_only_folder(folder: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    let metadata = fs::metadata(folder)?;
    // SAFETY: plain call with no arguments.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "the folder {} of the channel must have mode 0700 and the owner uid {uid}",
                folder.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept().map(|(stream, _)| stream)
}

pub(crate) fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
    let stream = crate::unix::ipc::connect_by(name, deadline)?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

pub(crate) fn at_name(name: &Path) -> io::Result<AtName> {
    match fs::symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_socket() => Ok(AtName::OldSocket),
        Ok(_) => Ok(AtName::Other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AtName::Free),
        Err(error) => Err(error),
    }
}

pub(crate) fn set_read_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))
}

pub(crate) fn set_write_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    stream.set_write_timeout(Some(timeout))
}

pub(crate) fn current_user() -> io::Result<User> {
    // SAFETY: plain call with no arguments.
    Ok(User::Uid(unsafe { libc::geteuid() }))
}
