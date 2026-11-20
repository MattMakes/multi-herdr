//! The concern channel for Windows: a named pipe that only this user can
//! open. `GetNamedPipeClientProcessId` gives the peer process, and the
//! token of that process gives the peer user. On a client handle,
//! `GetNamedPipeServerProcessId` gives the server process.

use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{
    DuplicateHandle, LocalFree, DUPLICATE_SAME_ACCESS, ERROR_BROKEN_PIPE, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeServerProcessId, PeekNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::System::IO::CancelSynchronousIo;

use crate::channel::{AtName, Peer, User};
use crate::NotSupported;

/// A named pipe server with 1 instance only. The listener holds that
/// instance for its whole life and reuses it for each client, so no other
/// process can make an instance of the name and catch a client (SR-8). A
/// client that finds the instance busy waits for it (`connect`).
#[derive(Debug)]
pub(crate) struct Listener(Arc<Instance>);

/// The only instance of the pipe.
#[derive(Debug)]
pub(crate) struct Instance {
    pipe: OwnedHandle,
    /// True while an accepted [`Stream`] holds the instance.
    busy: Mutex<bool>,
    /// Signals that `busy` became false.
    free: Condvar,
}

/// 1 connection of the channel.
#[derive(Debug)]
pub(crate) struct Stream {
    end: End,
    /// The longest wait of each read, or `None` for no limit. A pipe that
    /// is not overlapped has no read timeout, so a read with a limit first
    /// polls [`PeekNamedPipe`] until bytes wait or the limit passes.
    read_timeout: Mutex<Option<Duration>>,
    /// The longest wait of each write, or `None` for no limit. A pipe that
    /// is not overlapped has no write timeout, so a write with a limit is
    /// cancelled with [`CancelSynchronousIo`] when the limit passes.
    write_timeout: Mutex<Option<Duration>>,
    cut: AtomicBool,
}

/// The side of 1 connection.
#[derive(Debug)]
enum End {
    /// The client side owns its handle.
    Client(File),
    /// The server side uses the handle of the instance and never closes
    /// it. The drop ends the connection, and the listener can accept the
    /// next client.
    Server {
        view: ManuallyDrop<File>,
        instance: Arc<Instance>,
    },
}

/// The longest time that the end of a server connection waits for the
/// client to read the rest of the reply. It is below the 5 s request
/// deadline of the channel, so a client that never reads holds the only
/// instance for a short time only.
const DRAIN_LIMIT: Duration = Duration::from_secs(2);

/// The time between 2 polls of a read with a timeout.
const READ_POLL: Duration = Duration::from_millis(10);

/// The time between 2 cancels of a write that passed its timeout. A cancel
/// that comes before the write starts in the OS cancels nothing, so the
/// watchdog cancels again until the write returns.
const CANCEL_RETRY: Duration = Duration::from_millis(10);

/// The longest full pipe name, in UTF-16 units.
const MAX_PIPE_NAME: usize = 256;

/// The size of the in and out buffer of each instance, in bytes.
const BUFFER: u32 = 4096;

pub(crate) fn bind(name: &Path) -> io::Result<Listener> {
    let pipe = create_instance(&pipe_name(name)?)?;
    Ok(Listener(Arc::new(Instance {
        pipe,
        busy: Mutex::new(false),
        free: Condvar::new(),
    })))
}

/// `\\.\pipe\<name>`, with each `\` of `name` as `/`: a pipe name holds no
/// `\` after the prefix.
fn pipe_name(name: &Path) -> io::Result<Vec<u16>> {
    let invalid = |reason| io::Error::new(io::ErrorKind::InvalidInput, reason);
    let text: Vec<u16> = name
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'\\') {
                u16::from(b'/')
            } else {
                unit
            }
        })
        .collect();
    if text.is_empty() || text.contains(&0) {
        return Err(invalid("the pipe name is empty or holds a NUL"));
    }
    let mut full: Vec<u16> = r"\\.\pipe\".encode_utf16().chain(text).collect();
    if full.len() > MAX_PIPE_NAME {
        return Err(invalid("the pipe name is too long"));
    }
    full.push(0);
    Ok(full)
}

/// The only instance of the pipe `name`, with the access rule of this user
/// only. It refuses clients of other computers.
/// `FILE_FLAG_FIRST_PIPE_INSTANCE` refuses a pipe that exists, so no other
/// process can own the name before fleetd. `nMaxInstances` 1 refuses each
/// instance after it.
fn create_instance(name: &[u16]) -> io::Result<OwnedHandle> {
    let descriptor = OwnerOnly::new()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: `name` ends with a NUL, and `attributes` and the descriptor
    // it points to live until the call returns.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a new open handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// A security descriptor whose access rule allows only this user, freed
/// on drop.
struct OwnerOnly(PSECURITY_DESCRIPTOR);

impl OwnerOnly {
    fn new() -> io::Result<OwnerOnly> {
        let sid = current_sid()?;
        // The SDDL text of the access rule. `D:P` is a protected DACL, so
        // the pipe takes no rule from a parent. `(A;;GA;;;<sid>)` is 1 rule
        // that allows all access to this user. No other user has a rule.
        let text: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `text` ends with a NUL, and `descriptor` is valid for 1
        // write. The size pointer may be null.
        let made = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if made == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnerOnly(descriptor))
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: the system allocated the descriptor with LocalAlloc, and
        // only this drop frees it.
        unsafe { LocalFree(self.0) };
    }
}

/// Wait until the previous [`Stream`] of the server ends, then connect the
/// instance to the next client. An error ends only this connection: the
/// instance is disconnected, so it waits for the next client again.
pub(crate) fn accept(listener: &Listener) -> io::Result<(Stream, Peer)> {
    let instance = &listener.0;
    let mut busy = instance.busy.lock().unwrap_or_else(PoisonError::into_inner);
    while *busy {
        busy = instance
            .free
            .wait(busy)
            .unwrap_or_else(PoisonError::into_inner);
    }
    let pipe = instance.pipe.as_raw_handle();
    // SAFETY: `pipe` is open while `instance` holds it. A null OVERLAPPED
    // is valid for a pipe that was not opened for overlapped I/O.
    if unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            // SAFETY: `pipe` is open while `instance` holds it.
            unsafe { DisconnectNamedPipe(pipe) };
            return Err(error);
        }
    }
    *busy = true;
    drop(busy);
    let stream = Stream::new(End::Server {
        // SAFETY: `pipe` is open while `instance` lives, and the stream
        // holds `instance`. `ManuallyDrop` never closes the handle.
        view: ManuallyDrop::new(unsafe { File::from_raw_handle(pipe) }),
        instance: Arc::clone(instance),
    });
    // On an error, the drop of `stream` ends the connection.
    let peer = peer(pipe)?;
    Ok((stream, peer))
}

/// End the connection of the server side, and free the instance for the
/// next client. `DisconnectNamedPipe` discards the bytes that the client did
/// not read, so a helper thread first waits with `FlushFileBuffers` until
/// the client reads them, for at most [`DRAIN_LIMIT`].
fn end_connection(instance: &Arc<Instance>, cut: bool) {
    let (flushed, drained) = mpsc::channel();
    let pipe = instance.pipe.as_raw_handle();
    let flush = (!cut).then(|| {
        let instance = Arc::clone(instance);
        thread::Builder::new()
            .name("horch-pipe-drain".into())
            .spawn(move || {
                // SAFETY: the pipe is open while `instance` holds it.
                unsafe { FlushFileBuffers(instance.pipe.as_raw_handle()) };
                let _ = flushed.send(());
            })
            .ok()
    });
    let flush = flush.flatten();
    // Without a thread, the sender is gone and this returns at once.
    let _ = drained.recv_timeout(DRAIN_LIMIT);
    // SAFETY: `pipe` is open while `instance` holds it.
    unsafe { DisconnectNamedPipe(pipe) };
    if let Some(flush) = flush {
        // A flush that starts after the disconnect fails at once. A flush
        // that started before it and still waits stops here.
        // SAFETY: the thread handle is open until the join.
        unsafe { CancelSynchronousIo(flush.as_raw_handle()) };
        let _ = flush.join();
    }
    *instance.busy.lock().unwrap_or_else(PoisonError::into_inner) = false;
    instance.free.notify_one();
}

impl Stream {
    fn new(end: End) -> Stream {
        Stream {
            end,
            read_timeout: Mutex::new(None),
            write_timeout: Mutex::new(None),
            cut: AtomicBool::new(false),
        }
    }

    fn write_by(&self, buf: &[u8]) -> io::Result<usize> {
        let timeout = *self
            .write_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(timeout) = timeout else {
            return self.file().write(buf);
        };
        let written = cancel_after(timeout, || self.file().write(buf));
        match written {
            Err(e) if e.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) => {
                self.cut.store(true, Ordering::SeqCst);
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the write passed its timeout",
                ))
            }
            other => other,
        }
    }

    fn file(&self) -> &File {
        match &self.end {
            End::Client(file) => file,
            End::Server { view, .. } => view,
        }
    }

    /// Read into `buf`. With a read timeout, wait first until bytes wait
    /// in the pipe, and read only those, so the read never blocks.
    fn read_by(&self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = *self
            .read_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(timeout) = timeout else {
            return self.file().read(buf);
        };
        match self.wait_for_bytes(Instant::now() + timeout)? {
            // The other side closed: the read reports the end.
            None => self.file().read(buf),
            Some(0) => Err(io::ErrorKind::TimedOut.into()),
            Some(waiting) => {
                let n = buf
                    .len()
                    .min(usize::try_from(waiting).unwrap_or(usize::MAX));
                self.file().read(&mut buf[..n])
            }
        }
    }

    /// The count of bytes that wait in the pipe, polled until it is not 0
    /// or until `deadline`. `None` when the other side closed the pipe.
    fn wait_for_bytes(&self, deadline: Instant) -> io::Result<Option<u32>> {
        loop {
            let mut waiting = 0u32;
            // SAFETY: the stream keeps its pipe handle open. A null buffer
            // with size 0 reads no byte, and `waiting` is valid for 1 write.
            let peeked = unsafe {
                PeekNamedPipe(
                    self.file().as_raw_handle(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut waiting,
                    ptr::null_mut(),
                )
            };
            if peeked == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                    return Ok(None);
                }
                return Err(error);
            }
            if waiting > 0 || Instant::now() >= deadline {
                return Ok(Some(waiting));
            }
            thread::sleep(READ_POLL);
        }
    }
}

/// Run the synchronous I/O `io` on this thread. A watchdog thread cancels it
/// with [`CancelSynchronousIo`] when it runs longer than `limit`, and then
/// again each [`CANCEL_RETRY`] until `io` returns. A cancelled `io` fails
/// with `ERROR_OPERATION_ABORTED`. The scope joins the watchdog before the
/// return, so no cancel reaches a later I/O of this thread.
fn cancel_after<T>(limit: Duration, io: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let this = current_thread()?;
    let this = &this;
    let (done, finished) = mpsc::channel::<()>();
    thread::scope(|scope| {
        thread::Builder::new()
            .name("horch-pipe-watchdog".into())
            .spawn_scoped(scope, move || {
                let mut wait = limit;
                // `done` is dropped when `io` returns: that is `Disconnected`.
                while let Err(RecvTimeoutError::Timeout) = finished.recv_timeout(wait) {
                    // SAFETY: `this` is an open handle of the thread that runs
                    // `io`, and it lives until the scope ends.
                    unsafe { CancelSynchronousIo(this.as_raw_handle()) };
                    wait = CANCEL_RETRY;
                }
            })?;
        let result = io();
        drop(done);
        result
    })
}

/// A real handle of this thread, which another thread can use. The handle
/// of `GetCurrentThread` is a pseudo handle that means "the calling thread".
fn current_thread() -> io::Result<OwnedHandle> {
    let mut handle: HANDLE = ptr::null_mut();
    // SAFETY: plain calls; the pseudo handles need no close, and `handle` is
    // valid for 1 write.
    let duplicated = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            GetCurrentThread(),
            GetCurrentProcess(),
            &mut handle,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if duplicated == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a new open handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let End::Server { instance, .. } = &self.end {
            end_connection(instance, self.cut.load(Ordering::SeqCst));
        }
    }
}

impl Read for &Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_by(buf)
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_by(buf)
    }
}

impl Write for &Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_by(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file().flush()
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_by(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file().flush()
    }
}

fn peer(pipe: HANDLE) -> io::Result<Peer> {
    let mut pid = 0u32;
    // SAFETY: the caller keeps `pipe` open, and `pid` is valid for 1
    // write.
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: plain call; a failure answers a null handle.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `process` is a new open handle that nothing else owns.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let sid = process_sid(process.as_raw_handle())?;
    Ok(Peer {
        user: Ok(User::Sid(sid)),
        pid: Ok(pid),
    })
}

/// An error of the OS leaves the server process unknown: the fact is not
/// given, never a guess.
pub(crate) fn server_pid(stream: &Stream) -> Result<u32, NotSupported> {
    let mut pid = 0u32;
    // SAFETY: `stream` keeps its pipe handle open, and `pid` is valid for 1
    // write.
    let read = unsafe { GetNamedPipeServerProcessId(stream.file().as_raw_handle(), &mut pid) };
    if read == 0 || pid == 0 {
        return Err(NotSupported::new("channel", "windows"));
    }
    Ok(pid)
}

/// `SECURITY_IDENTIFICATION` lets the server read who the client is, but
/// not act as the client.
pub(crate) fn connect(name: &Path, deadline: Instant) -> io::Result<Stream> {
    let name = pipe_name(name)?;
    loop {
        // SAFETY: `name` ends with a NUL. The security attributes and the
        // template may be null.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            // SAFETY: `handle` is a new open handle that nothing else owns.
            return Ok(Stream::new(End::Client(unsafe {
                File::from_raw_handle(handle)
            })));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
            return Err(error);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // `u32::MAX` means "wait forever", so the wait stays below it.
        let millis = left.as_millis().clamp(1, u128::from(u32::MAX - 1)) as u32;
        // SAFETY: `name` ends with a NUL.
        if unsafe { WaitNamedPipeW(name.as_ptr(), millis) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                return Err(io::ErrorKind::TimedOut.into());
            }
            // Any other error, for example a pipe that is gone: the next
            // CreateFileW reports it.
        }
    }
}

/// The pipe name is not a file, and the pipe of an old listener ends with
/// its process.
pub(crate) fn at_name(_: &Path) -> io::Result<AtName> {
    Ok(AtName::Free)
}

pub(crate) fn set_read_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a read timeout of 0 is not valid",
        ));
    }
    *stream
        .read_timeout
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(timeout);
    Ok(())
}

/// A pipe that is not overlapped has no write timeout, so each later write
/// is cancelled when it waits longer than `timeout` (`Stream::write_by`).
pub(crate) fn set_write_timeout(stream: &Stream, timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a write timeout of 0 is not valid",
        ));
    }
    *stream
        .write_timeout
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(timeout);
    Ok(())
}

pub(crate) fn current_user() -> io::Result<User> {
    current_sid().map(User::Sid)
}

fn current_sid() -> io::Result<String> {
    // SAFETY: plain call; the pseudo handle needs no close.
    process_sid(unsafe { GetCurrentProcess() })
}

/// The user SID of the token of `process`, as text.
fn process_sid(process: HANDLE) -> io::Result<String> {
    let token = open_token(process)?;
    let user = token_user(&token)?;
    // SAFETY: `token_user` wrote a TOKEN_USER at the start of `user`, and
    // its SID points into `user`, which lives to the end of this function.
    let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // SAFETY: `sid` is valid while `user` lives.
    unsafe { sid_text(sid) }
}

/// The token of `process`, open for queries and closed on drop.
fn open_token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is open for the call, and `token` is valid for 1
    // write.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `token` is a new open handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

/// The `TOKEN_USER` of `token` and the SID that it points to. The buffer
/// has u64 units, so that it has the alignment of `TOKEN_USER`.
fn token_user(token: &OwnedHandle) -> io::Result<Vec<u64>> {
    let token = token.as_raw_handle();
    let mut length = 0u32;
    // SAFETY: a null buffer of length 0 asks only for the length.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    // SAFETY: `buffer` has room for `length` bytes.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

/// The text of `sid`, for example `S-1-5-21-...`.
///
/// # Safety
///
/// `sid` points to a valid SID for the length of the call.
pub(crate) unsafe fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text: PWSTR = ptr::null_mut();
    // SAFETY: the caller keeps `sid` valid, and `text` is valid for 1
    // write.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the system wrote a NUL-terminated string at `text`.
    let units = unsafe {
        let length = (0..).take_while(|&i| *text.add(i) != 0).count();
        std::slice::from_raw_parts(text, length)
    };
    let sid = String::from_utf16_lossy(units);
    // SAFETY: the system allocated `text` with LocalAlloc, and nothing
    // reads it after this call.
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::System::Pipes::PIPE_UNLIMITED_INSTANCES;

    use super::*;

    /// A pipe name that no other test and no other run uses.
    fn unique_name(test: &str) -> PathBuf {
        PathBuf::from(format!("horch-test-{}-{test}", std::process::id()))
    }

    /// The upper limit of a connect in a test. No test waits for it: a
    /// connect returns as soon as an instance is free.
    fn far_deadline() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    /// A second instance of the pipe `name`, as another process of this
    /// user can try to make it. `None` when the system refuses it.
    fn other_instance(name: &Path, max_instances: u32) -> Option<OwnedHandle> {
        let name = pipe_name(name).unwrap();
        // SAFETY: `name` ends with a NUL. The security attributes may be
        // null.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                max_instances,
                BUFFER,
                BUFFER,
                0,
                ptr::null(),
            )
        };
        // SAFETY: a valid `handle` is a new open handle that nothing else
        // owns.
        (handle != INVALID_HANDLE_VALUE).then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    /// Accept `count` clients. Each sends 1 line and gets the same line
    /// back. The result is the lines in the order of the clients.
    fn echo(listener: &Listener, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                let (stream, _peer) = accept(listener).unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                (&stream).write_all(line.as_bytes()).unwrap();
                line
            })
            .collect()
    }

    fn ask(stream: &Stream, line: &str) -> String {
        let mut writer = stream;
        writer.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        reply
    }

    #[test]
    fn second_instance_of_the_channel_name_is_refused() {
        let name = unique_name("second-instance");
        let listener = bind(&name).unwrap();
        let refused = |when: &str| {
            for max_instances in [1, PIPE_UNLIMITED_INSTANCES] {
                assert!(
                    other_instance(&name, max_instances).is_none(),
                    "a second instance with nMaxInstances {max_instances} is refused {when}"
                );
            }
        };
        refused("before a client");

        // After a client ends, the listener still holds the only instance.
        thread::scope(|scope| {
            let server = scope.spawn(|| echo(&listener, 1));
            let client = connect(&name, far_deadline()).unwrap();
            assert_eq!(ask(&client, "ping\n"), "ping\n");
            drop(client);
            assert_eq!(server.join().unwrap(), ["ping\n"]);
        });
        refused("after a client");
    }

    #[test]
    fn client_waits_on_pipe_busy_and_connects_after_the_first_client_ends() {
        let name = unique_name("pipe-busy");
        let listener = bind(&name).unwrap();
        thread::scope(|scope| {
            let server = scope.spawn(|| echo(&listener, 2));
            let first = connect(&name, far_deadline()).unwrap();

            // The first client holds the only instance: a connect whose
            // deadline is over gets PIPE_BUSY and stops at once.
            let busy = connect(&name, Instant::now()).unwrap_err();
            assert_eq!(busy.kind(), io::ErrorKind::TimedOut);

            let second = scope.spawn(|| ask(&connect(&name, far_deadline()).unwrap(), "second\n"));
            assert_eq!(ask(&first, "first\n"), "first\n");
            drop(first);

            assert_eq!(second.join().unwrap(), "second\n");
            assert_eq!(server.join().unwrap(), ["first\n", "second\n"]);
        });
    }

    #[test]
    fn client_that_never_reads_the_reply_does_not_block_the_next_client() {
        let name = unique_name("never-reads");
        let listener = bind(&name).unwrap();
        thread::scope(|scope| {
            let server = scope.spawn(|| echo(&listener, 2));
            // This client sends 1 line, never reads the reply and keeps its
            // handle open.
            let silent = connect(&name, far_deadline()).unwrap();
            (&silent).write_all(b"silent\n").unwrap();

            // The server ends that connection after DRAIN_LIMIT. Without
            // that limit, this connect stops at its deadline with TimedOut.
            // The next client gets all of its reply.
            let next = connect(&name, far_deadline()).unwrap();
            assert_eq!(ask(&next, "next\n"), "next\n");
            assert_eq!(server.join().unwrap(), ["silent\n", "next\n"]);
            drop(silent);
        });
    }

    const LARGE_REPLY: usize = 128 * 1024;

    fn serve_large_reply(
        listener: &Listener,
        timeout: Duration,
    ) -> (Stream, String, io::Result<()>) {
        let (stream, _peer) = accept(listener).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        set_write_timeout(&stream, timeout).unwrap();
        let mut reply = vec![b'x'; LARGE_REPLY];
        reply.push(b'\n');
        let written = (&stream).write_all(&reply);
        (stream, line, written)
    }

    #[test]
    fn large_reply_to_non_reading_client_times_out_and_does_not_block() {
        let name = unique_name("large-reply");
        let listener = bind(&name).unwrap();
        let timeout = Duration::from_millis(300);

        let silent = connect(&name, far_deadline()).unwrap();
        (&silent).write_all(b"silent\n").unwrap();
        let started = Instant::now();
        let (stream, line, written) = serve_large_reply(&listener, timeout);
        let error = written.unwrap_err();
        assert_eq!(line, "silent\n");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error:?}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the write ends near its timeout: {:?}",
            started.elapsed()
        );
        let closing = Instant::now();
        drop(stream);
        assert!(
            closing.elapsed() < Duration::from_secs(1),
            "{:?}",
            closing.elapsed()
        );

        thread::scope(|scope| {
            let next = scope.spawn(|| {
                let stream = connect(&name, far_deadline()).unwrap();
                (&stream).write_all(b"next\n").unwrap();
                let mut reply = Vec::new();
                (&stream).read_to_end(&mut reply).unwrap();
                reply.len()
            });
            let (stream, line, written) = serve_large_reply(&listener, timeout * 10);
            assert_eq!(line, "next\n");
            written.unwrap();
            drop(stream);
            assert_eq!(next.join().unwrap(), LARGE_REPLY + 1);
        });

        let closing = Instant::now();
        drop(silent);
        drop(listener);
        assert!(closing.elapsed() < Duration::from_secs(1));
    }
}
