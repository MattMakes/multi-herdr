//! The concern channel for the code of macOS and Linux: a Unix socket with
//! mode 0600 in a folder of mode 0700. `mac` and `linux` read the peer.

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Instant;

use crate::channel::User;

pub(crate) type Stream = UnixStream;
pub(crate) type Listener = UnixListener;

/// The socket file is made inside a folder that only this user can enter,
/// so no other user can connect between the bind and the change of mode.
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

/// Make `folder` with mode 0700 when it is missing. Refuse a folder that
/// another user owns or that others can enter.
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

/// The connect of the ipc concern waits only until `deadline`. Its stream
/// is nonblocking, and this stream blocks again on each read and write.
pub(crate) fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
    let stream = crate::unix::ipc::connect_by(name, deadline)?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

pub(crate) fn current_user() -> io::Result<User> {
    // SAFETY: plain call with no arguments.
    Ok(User::Uid(unsafe { libc::geteuid() }))
}
