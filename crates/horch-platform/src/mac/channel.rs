//! The concern channel for macOS: a Unix socket. `getpeereid` gives the
//! peer user and `LOCAL_PEERPID` gives the peer process. On a client
//! socket, `LOCAL_PEERPID` gives the process that made the listener.

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

pub(crate) fn accept(listener: &Listener) -> io::Result<(Stream, Peer)> {
    let stream = unix::accept(listener)?;
    let peer = Peer {
        user: Ok(User::Uid(peer_uid(&stream)?)),
        pid: Ok(peer_pid(&stream)?),
    };
    Ok((stream, peer))
}

/// An error of the OS leaves the server process unknown: the fact is not
/// given, never a guess.
pub(crate) fn server_pid(stream: &Stream) -> Result<u32, NotSupported> {
    match peer_pid(stream) {
        Ok(pid) if pid != 0 => Ok(pid),
        _ => Err(NotSupported {
            concern: "channel",
            os: "macos",
        }),
    }
}

fn peer_uid(stream: &Stream) -> io::Result<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the socket is open while `stream` lives, and both pointers
    // are valid for 1 write each.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

fn peer_pid(stream: &Stream) -> io::Result<u32> {
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the socket is open while `stream` lives, `pid` has room for
    // `length` bytes, and `length` is valid for 1 write.
    let read = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid as u32)
}
