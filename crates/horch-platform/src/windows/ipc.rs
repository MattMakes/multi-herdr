//! The concern ipc for Windows: no local stream socket.
//!
//! Each function gives [`NotSupported`]. That gap is the row "Sockets and
//! pipes" of `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
//! section 6, and its S11: the worker channel relay and the report socket
//! need Unix sockets. No value of [`Stream`] or [`Listener`] can exist.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::NotSupported;

#[derive(Debug)]
pub(crate) enum Stream {}

#[derive(Debug)]
pub(crate) enum Listener {}

fn not_supported() -> io::Error {
    NotSupported {
        concern: "ipc",
        os: "windows",
    }
    .into()
}

pub(crate) fn connect(_: &Path) -> io::Result<Stream> {
    Err(not_supported())
}

pub(crate) fn connect_by(_: &Path, _: Instant) -> io::Result<Stream> {
    Err(not_supported())
}

pub(crate) fn write_by(stream: &mut Stream, _: &[u8], _: Instant) -> io::Result<()> {
    match *stream {}
}

pub(crate) fn read_line_by(stream: &mut Stream, _: Instant, _: usize) -> io::Result<String> {
    match *stream {}
}

pub(crate) fn set_timeouts(stream: &Stream, _: Duration) -> io::Result<()> {
    match *stream {}
}

pub(crate) fn set_read_timeout(stream: &Stream, _: Duration) -> io::Result<()> {
    match *stream {}
}

pub(crate) fn bind(_: &Path) -> io::Result<Listener> {
    Err(not_supported())
}

pub(crate) fn accept(listener: &Listener) -> io::Result<Stream> {
    match *listener {}
}

pub(crate) fn set_nonblocking(listener: &Listener, _: bool) -> io::Result<()> {
    match *listener {}
}

pub(crate) fn is_supported() -> bool {
    false
}

pub(crate) fn is_not_a_socket(_: &io::Error) -> bool {
    false
}

impl Read for Stream {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        match *self {}
    }
}

impl Read for &Stream {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        match **self {}
    }
}

impl Write for Stream {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        match *self {}
    }

    fn flush(&mut self) -> io::Result<()> {
        match *self {}
    }
}

impl Write for &Stream {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        match **self {}
    }

    fn flush(&mut self) -> io::Result<()> {
        match **self {}
    }
}
