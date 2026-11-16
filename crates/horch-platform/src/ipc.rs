//! The concern ipc: local sockets and pipes, and the detach of a child process.
//!
//! [`LocalStream`] and [`LocalListener`] are a local stream socket and its
//! listener: a Unix socket on macOS and Linux. The `_by` functions share 1
//! deadline: the connect, each write and the read of a line each get only
//! the time that is left.
//!
//! On Windows each function gives [`NotSupported`](crate::NotSupported), as
//! an `std::io::Error` of kind `Unsupported`. That gap is the row "Sockets
//! and pipes" of `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
//! section 6, and its S11: the worker channel relay and the report socket
//! of `horch_access: report` need Unix sockets. Phase 5 deletes the worker
//! sockets (S11), and then these functions with their callers.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::os;

/// A connected local stream socket.
#[derive(Debug)]
pub struct LocalStream(os::ipc::Stream);

/// A local socket that accepts [`LocalStream`]s.
#[derive(Debug)]
pub struct LocalListener(os::ipc::Listener);

impl LocalStream {
    /// Connect to the socket at `path` and block until the connect ends.
    /// The connect has no timeout. Use [`LocalStream::connect_by`] for one.
    pub fn connect(path: &Path) -> io::Result<LocalStream> {
        os::ipc::connect(path).map(LocalStream)
    }

    /// Connect to the socket at `path`, but wait only until `deadline`.
    /// The stream is nonblocking and close-on-exec. A path that does not
    /// fit the socket address gives `InvalidInput`.
    pub fn connect_by(path: &Path, deadline: Instant) -> io::Result<LocalStream> {
        os::ipc::connect_by(path, deadline).map(LocalStream)
    }

    /// Write all of `bytes`, but wait only until `deadline`. The stream
    /// comes from [`LocalStream::connect_by`].
    pub fn write_by(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        os::ipc::write_by(&mut self.0, bytes, deadline)
    }

    /// Read 1 line, up to and with its newline, or what came before EOF,
    /// but wait only until `deadline`. The read stops after the newline or
    /// at `limit` bytes. Bytes that are not UTF-8 give `InvalidData`.
    pub fn read_line_by(&mut self, deadline: Instant, limit: usize) -> io::Result<String> {
        os::ipc::read_line_by(&mut self.0, deadline, limit)
    }

    /// Set the read timeout and the write timeout to `timeout`.
    pub fn set_timeouts(&self, timeout: Duration) -> io::Result<()> {
        os::ipc::set_timeouts(&self.0, timeout)
    }

    /// Set only the read timeout to `timeout`.
    pub fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        os::ipc::set_read_timeout(&self.0, timeout)
    }
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Read for &LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.0).read(buf)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Write for &LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.0).flush()
    }
}

impl LocalListener {
    /// Bind a new socket file at `path`. The caller makes the folder,
    /// removes an old socket file and sets the mode of the new one.
    pub fn bind(path: &Path) -> io::Result<LocalListener> {
        os::ipc::bind(path).map(LocalListener)
    }

    /// Accept 1 connection. A nonblocking listener with no connection
    /// waiting gives `WouldBlock`.
    pub fn accept(&self) -> io::Result<LocalStream> {
        os::ipc::accept(&self.0).map(LocalStream)
    }

    /// Make [`LocalListener::accept`] nonblocking, or blocking again.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        os::ipc::set_nonblocking(&self.0, nonblocking)
    }
}

/// Whether `error` of a connect means that the file at the path is not a
/// socket (`ENOTSOCK`). Always false on Windows.
pub fn is_not_a_socket(error: &io::Error) -> bool {
    os::ipc::is_not_a_socket(error)
}
