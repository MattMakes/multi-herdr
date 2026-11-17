//! The concern channel: the transport of the fleetd command channel, the
//! peer user and the peer process (PL-2, SR-5, SR-8).
//!
//! [`Listener::bind`] makes the channel and [`Listener::accept`] gives each
//! connection as a [`Stream`] with its [`Peer`]. A client connects with
//! [`Stream::connect`]. [`User::current`] is the user to compare with
//! [`Peer::user`].
//!
//! The transport of each OS:
//! - macOS and Linux: a Unix socket at the path `name`, with mode 0600, in a
//!   folder of mode 0700 that this user owns.
//! - Windows: the named pipe `\\.\pipe\<name>`, where each `\` of `name` is
//!   `/`. Its access rule allows only this user, and it refuses remote
//!   clients.
//!
//! The OS gives the facts of the peer:
//! - macOS: `getpeereid` for the user and `LOCAL_PEERPID` for the process.
//! - Linux: `SO_PEERCRED` for both.
//! - Windows: `GetNamedPipeClientProcessId` for the process, and the user
//!   of the token of that process.
//!
//! A fact that an OS cannot give is [`NotSupported`] (PL-3), never a guess.
//! Each OS of today gives both facts.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Instant;

use crate::{os, NotSupported};

/// The listener of the channel.
#[derive(Debug)]
pub struct Listener(os::channel::Listener);

/// 1 connection of the channel, on the side of the server or of a client.
#[derive(Debug)]
pub struct Stream(os::channel::Stream);

/// An OS user.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum User {
    /// The effective user id on macOS and Linux.
    Uid(u32),
    /// The text of the user SID on Windows, for example `S-1-5-21-...`.
    Sid(String),
}

/// The facts of the peer of 1 accepted connection, as the OS gives them at
/// the connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// The user of the peer process.
    pub user: Result<User, NotSupported>,
    /// The id of the peer process.
    pub pid: Result<u32, NotSupported>,
}

impl Listener {
    /// Make the channel `name`. On unix, `bind` makes a missing folder of
    /// the socket with mode 0700. It refuses with `PermissionDenied` a
    /// folder that another user owns or that others can enter. It refuses
    /// an existing file at `name`, so the caller, which holds the fleetd
    /// lock, removes an old socket file first. On Windows, `bind` refuses a
    /// pipe `name` that exists.
    pub fn bind(name: &Path) -> io::Result<Listener> {
        os::channel::bind(name).map(Listener)
    }

    /// Wait for 1 connection, and give it with the facts of its peer. An
    /// error ends only that connection: the listener accepts the next one.
    pub fn accept(&self) -> io::Result<(Stream, Peer)> {
        let (stream, peer) = os::channel::accept(&self.0)?;
        Ok((Stream(stream), peer))
    }
}

impl Stream {
    /// Connect to the channel `name`, but wait only until `deadline`. A
    /// channel that does not exist gives `NotFound`. The stream blocks on
    /// each read and write.
    pub fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
        os::channel::connect(name, deadline).map(Stream)
    }
}

impl User {
    /// The user of this process: the effective user id on macOS and Linux,
    /// the user of the process token on Windows.
    pub fn current() -> io::Result<User> {
        os::channel::current_user()
    }
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            User::Uid(uid) => write!(f, "uid {uid}"),
            User::Sid(sid) => f.write_str(sid),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Read for &Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.0).read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Write for &Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.0).flush()
    }
}
