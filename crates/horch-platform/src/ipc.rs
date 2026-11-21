use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::os;

#[derive(Debug)]
pub struct LocalStream(os::ipc::Stream);

#[derive(Debug)]
pub struct LocalListener(os::ipc::Listener);

impl LocalStream {
    pub fn connect(path: &Path) -> io::Result<LocalStream> {
        os::ipc::connect(path).map(LocalStream)
    }

    pub fn connect_by(path: &Path, deadline: Instant) -> io::Result<LocalStream> {
        os::ipc::connect_by(path, deadline).map(LocalStream)
    }

    pub fn write_by(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        os::ipc::write_by(&mut self.0, bytes, deadline)
    }

    pub fn read_line_by(&mut self, deadline: Instant, limit: usize) -> io::Result<String> {
        os::ipc::read_line_by(&mut self.0, deadline, limit)
    }

    pub fn set_timeouts(&self, timeout: Duration) -> io::Result<()> {
        os::ipc::set_timeouts(&self.0, timeout)
    }

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
    pub fn bind(path: &Path) -> io::Result<LocalListener> {
        os::ipc::bind(path).map(LocalListener)
    }

    pub fn accept(&self) -> io::Result<LocalStream> {
        os::ipc::accept(&self.0).map(LocalStream)
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        os::ipc::set_nonblocking(&self.0, nonblocking)
    }
}

pub fn is_supported() -> bool {
    os::ipc::is_supported()
}

pub fn is_not_a_socket(error: &io::Error) -> bool {
    os::ipc::is_not_a_socket(error)
}
