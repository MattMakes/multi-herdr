//! The concern channel for Windows: a named pipe that only this user can
//! open. `GetNamedPipeClientProcessId` gives the peer process, and the
//! token of that process gives the peer user.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::{
    LocalFree, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_SEM_TIMEOUT, GENERIC_READ,
    GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::channel::{Peer, User};

pub(crate) type Stream = File;

/// A named pipe server. Each client takes 1 instance of the pipe, so the
/// listener always holds the instance that waits for the next client.
#[derive(Debug)]
pub(crate) struct Listener {
    /// The full pipe name, `\\.\pipe\...`, as UTF-16 with a NUL at the end.
    name: Vec<u16>,
    /// The instance that waits for the next client.
    next: Mutex<OwnedHandle>,
}

/// The longest full pipe name, in UTF-16 units.
const MAX_PIPE_NAME: usize = 256;

/// The size of the in and out buffer of each instance, in bytes.
const BUFFER: u32 = 4096;

/// `FILE_FLAG_FIRST_PIPE_INSTANCE` refuses a pipe that exists, so no other
/// process can own the name before fleetd.
pub(crate) fn bind(name: &Path) -> io::Result<Listener> {
    let name = pipe_name(name)?;
    let first = create_instance(&name, FILE_FLAG_FIRST_PIPE_INSTANCE)?;
    Ok(Listener {
        name,
        next: Mutex::new(first),
    })
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

/// A new instance of the pipe `name`, with the access rule of this user
/// only. It refuses clients of other computers.
fn create_instance(name: &[u16], flags: FILE_FLAGS_AND_ATTRIBUTES) -> io::Result<OwnedHandle> {
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
            PIPE_ACCESS_DUPLEX | flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
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

/// An error ends only this connection: the instance is disconnected, so
/// it waits for the next client again.
pub(crate) fn accept(listener: &Listener) -> io::Result<(Stream, Peer)> {
    let mut next = listener.next.lock().unwrap_or_else(PoisonError::into_inner);
    let pipe = next.as_raw_handle();
    // SAFETY: `pipe` is open while `next` holds it. A null OVERLAPPED is
    // valid for a pipe that was not opened for overlapped I/O.
    if unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            // SAFETY: `pipe` is open while `next` holds it.
            unsafe { DisconnectNamedPipe(pipe) };
            return Err(error);
        }
    }
    let waiting = match create_instance(&listener.name, 0) {
        Ok(waiting) => waiting,
        Err(error) => {
            // SAFETY: `pipe` is open while `next` holds it.
            unsafe { DisconnectNamedPipe(pipe) };
            return Err(error);
        }
    };
    let connected = std::mem::replace(&mut *next, waiting);
    drop(next);
    let peer = peer(&connected)?;
    Ok((File::from(connected), peer))
}

fn peer(pipe: &OwnedHandle) -> io::Result<Peer> {
    let mut pid = 0u32;
    // SAFETY: the pipe is open while `pipe` lives, and `pid` is valid for 1
    // write.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
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
            return Ok(unsafe { File::from_raw_handle(handle) });
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

pub(crate) fn current_user() -> io::Result<User> {
    current_sid().map(User::Sid)
}

fn current_sid() -> io::Result<String> {
    // SAFETY: plain call; the pseudo handle needs no close.
    process_sid(unsafe { GetCurrentProcess() })
}

/// The user SID of the token of `process`, as text.
fn process_sid(process: HANDLE) -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is open for the call, and `token` is valid for 1
    // write.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `token` is a new open handle that nothing else owns.
    let owned = unsafe { OwnedHandle::from_raw_handle(token) };
    let token = owned.as_raw_handle();
    let mut length = 0u32;
    // SAFETY: a null buffer of length 0 asks only for the length.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 units, so that the buffer has the alignment of TOKEN_USER.
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
    // SAFETY: the call above wrote a TOKEN_USER at the start of `buffer`,
    // and its SID points into `buffer`, which lives to the end of this
    // function.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: PWSTR = ptr::null_mut();
    // SAFETY: `sid` is valid while `buffer` lives, and `text` is valid for
    // 1 write.
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
