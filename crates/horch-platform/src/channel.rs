use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{os, NotSupported};

#[derive(Debug)]
pub struct Listener(os::channel::Listener);

#[derive(Debug)]
pub struct Stream(os::channel::Stream);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum User {
    Uid(u32),
    Sid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub user: Result<User, NotSupported>,
    pub pid: Result<u32, NotSupported>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtName {
    Free,
    OldSocket,
    Other,
}

pub fn at_name(name: &Path) -> io::Result<AtName> {
    os::channel::at_name(name)
}

impl Listener {
    pub fn bind(name: &Path) -> io::Result<Listener> {
        os::channel::bind(name).map(Listener)
    }

    pub fn accept(&self) -> io::Result<(Stream, Peer)> {
        let (stream, peer) = os::channel::accept(&self.0)?;
        Ok((Stream(stream), peer))
    }
}

impl Stream {
    pub fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
        os::channel::connect(name, deadline).map(Stream)
    }

    pub fn server_pid(&self) -> Result<u32, NotSupported> {
        os::channel::server_pid(&self.0)
    }

    pub fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        os::channel::set_read_timeout(&self.0, timeout)
    }

    pub fn set_write_timeout(&self, timeout: Duration) -> io::Result<()> {
        os::channel::set_write_timeout(&self.0, timeout)
    }
}

impl User {
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
