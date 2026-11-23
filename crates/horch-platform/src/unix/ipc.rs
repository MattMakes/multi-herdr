use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) type Stream = UnixStream;
pub(crate) type Listener = UnixListener;

pub(crate) fn connect(path: &Path) -> io::Result<Stream> {
    UnixStream::connect(path)
}

pub(crate) fn connect_by(path: &Path, deadline: Instant) -> io::Result<Stream> {
    let address = unix_address(path)?;
    let stream = new_nonblocking_socket()?;
    let length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    loop {
        // SAFETY: `address` is a valid sockaddr_un of `length` bytes.
        let connected = unsafe {
            libc::connect(
                stream.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                length,
            )
        };
        if connected == 0 {
            return Ok(stream);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINPROGRESS) => break,
            Some(libc::EAGAIN) => std::thread::sleep(time_left(deadline)?.min(BACKLOG_RETRY)),
            _ => return Err(error),
        }
    }
    wait_ready(&stream, libc::POLLOUT, deadline)?;
    if let Some(error) = stream.take_error()? {
        return Err(error);
    }
    Ok(stream)
}

const BACKLOG_RETRY: Duration = Duration::from_millis(5);

fn unix_address(path: &Path) -> io::Result<libc::sockaddr_un> {
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: sockaddr_un is plain data; all zeroes is a valid value.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= address.sun_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket path is too long",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (to, from) in address.sun_path.iter_mut().zip(bytes) {
        *to = *from as libc::c_char;
    }
    Ok(address)
}

fn new_nonblocking_socket() -> io::Result<Stream> {
    // SAFETY: plain syscall; a negative answer is checked before use.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is a new socket that nothing else owns.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: plain syscall on the socket that `stream` owns.
    if unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    Ok(stream)
}

pub(crate) fn write_by(stream: &mut Stream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        match stream.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) => retry_after(error, stream, libc::POLLOUT, deadline)?,
        }
    }
    Ok(())
}

pub(crate) fn read_line_by(
    stream: &mut Stream,
    deadline: Instant,
    limit: usize,
) -> io::Result<String> {
    let mut answer = Vec::new();
    let mut byte = [0; 1];
    while answer.len() < limit && answer.last() != Some(&b'\n') {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => answer.push(byte[0]),
            Err(error) => retry_after(error, stream, libc::POLLIN, deadline)?,
        }
    }
    String::from_utf8(answer).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn retry_after(
    error: io::Error,
    stream: &Stream,
    events: libc::c_short,
    deadline: Instant,
) -> io::Result<()> {
    match error.kind() {
        io::ErrorKind::WouldBlock => wait_ready(stream, events, deadline),
        io::ErrorKind::Interrupted => time_left(deadline).map(drop),
        _ => Err(error),
    }
}

fn wait_ready(stream: &Stream, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    let mut poll = libc::pollfd {
        fd: stream.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        let left = time_left(deadline)?;
        let millis = left.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as libc::c_int;
        // SAFETY: `poll` points to 1 valid pollfd for the length of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready > 0 {
            return Ok(());
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

fn time_left(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(left)
}

pub(crate) fn set_timeouts(stream: &Stream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

pub(crate) fn set_read_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))
}

pub(crate) fn bind(path: &Path) -> io::Result<Listener> {
    UnixListener::bind(path)
}

pub(crate) fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept().map(|(stream, _)| stream)
}

pub(crate) fn set_nonblocking(listener: &Listener, nonblocking: bool) -> io::Result<()> {
    listener.set_nonblocking(nonblocking)
}

pub(crate) fn is_supported() -> bool {
    true
}

pub(crate) fn is_not_a_socket(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ENOTSOCK)
}
