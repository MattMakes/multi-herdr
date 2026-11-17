//! The concern channel for Linux: a Unix socket. `SO_PEERCRED` gives the
//! peer user and the peer process.

use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::Instant;

use crate::channel::{Peer, User};
use crate::unix::channel as unix;
use crate::NotSupported;

pub(crate) use crate::unix::channel::{bind, current_user, Listener, Stream};

pub(crate) fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
    unix::connect(name, deadline)
}

/// The kernel writes the pid 0 when the peer process has no pid in the pid
/// namespace of this process. Then Linux cannot give the peer process.
pub(crate) fn accept(listener: &Listener) -> io::Result<(Stream, Peer)> {
    let stream = unix::accept(listener)?;
    let credentials = peer_credentials(&stream)?;
    let pid = match credentials.pid {
        0 => Err(NotSupported {
            concern: "channel",
            os: "linux",
        }),
        pid => Ok(pid as u32),
    };
    let peer = Peer {
        user: Ok(User::Uid(credentials.uid)),
        pid,
    };
    Ok((stream, peer))
}

fn peer_credentials(stream: &Stream) -> io::Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the socket is open while `stream` lives, `credentials` has
    // room for `length` bytes, and `length` is valid for 1 write.
    let read = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(credentials)
}
