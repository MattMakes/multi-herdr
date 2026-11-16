use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{fcntl, FcntlArg, FdFlag, OFlag};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::pty::{openpty, Winsize};
use nix::sys::signal::{kill, killpg, Signal};
use nix::unistd::Pid;

use crate::driver::DriverError;

const TERMINATE_GRACE: Duration = Duration::from_millis(500);

pub enum ReadOutcome {
    Data(usize),
    Empty,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitReport {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl From<ExitStatus> for ExitReport {
    fn from(status: ExitStatus) -> Self {
        ExitReport {
            code: status.code(),
            signal: status.signal(),
        }
    }
}

pub struct Pty {
    master: OwnedFd,
    child: Child,
    exit: Option<ExitReport>,
}

fn winsize(columns: u16, rows: u16) -> Winsize {
    Winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn setup_error(context: &str, error: impl std::fmt::Display) -> DriverError {
    DriverError::Setup(format!("{context}: {error}"))
}

fn timeout(duration: Duration) -> PollTimeout {
    let milliseconds = duration
        .as_nanos()
        .div_ceil(1_000_000)
        .min(i32::MAX as u128) as i32;
    PollTimeout::try_from(milliseconds).unwrap_or(PollTimeout::MAX)
}

impl Pty {
    pub fn spawn(
        program: &str,
        args: &[String],
        columns: u16,
        rows: u16,
    ) -> Result<Pty, DriverError> {
        let pair = openpty(&winsize(columns, rows), None)
            .map_err(|error| setup_error("cannot open a pseudo-terminal", error))?;
        for fd in [&pair.master, &pair.slave] {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))
                .map_err(|error| setup_error("cannot configure the pseudo-terminal", error))?;
        }
        let flags = fcntl(&pair.master, FcntlArg::F_GETFL)
            .map_err(|error| setup_error("cannot configure the pseudo-terminal", error))?;
        fcntl(
            &pair.master,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map_err(|error| setup_error("cannot configure the pseudo-terminal", error))?;
        let stdio = || -> Result<Stdio, DriverError> {
            pair.slave
                .try_clone()
                .map(Stdio::from)
                .map_err(|error| setup_error("cannot share the pseudo-terminal", error))
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .env("TERM", "xterm-256color")
            .stdin(stdio()?)
            .stdout(stdio()?)
            .stderr(stdio()?);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|error| setup_error(&format!("cannot start {program}"), error))?;
        drop(pair.slave);
        Ok(Pty {
            master: pair.master,
            child,
            exit: None,
        })
    }

    pub fn exit(&mut self) -> Option<ExitReport> {
        if self.exit.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.exit = Some(status.into());
            }
        }
        self.exit
    }

    pub fn wait_readable(&self, want_write: bool, duration: Duration) -> Result<bool, DriverError> {
        let mut events = PollFlags::POLLIN;
        if want_write {
            events |= PollFlags::POLLOUT;
        }
        let mut fds = [PollFd::new(self.master.as_fd(), events)];
        match poll(&mut fds, timeout(duration)) {
            Ok(_) | Err(Errno::EINTR) => {}
            Err(error) => {
                return Err(DriverError::Failed(format!(
                    "cannot poll the pseudo-terminal: {error}"
                )))
            }
        }
        let returned = fds[0].revents().unwrap_or(PollFlags::empty());
        let closed = PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL;
        Ok(returned.intersects(PollFlags::POLLIN | closed))
    }

    pub fn read(&self, buffer: &mut [u8]) -> Result<ReadOutcome, DriverError> {
        loop {
            return match nix::unistd::read(&self.master, buffer) {
                Ok(0) => Ok(ReadOutcome::Closed),
                Ok(count) => Ok(ReadOutcome::Data(count)),
                Err(Errno::EAGAIN) => Ok(ReadOutcome::Empty),
                Err(Errno::EINTR) => continue,
                Err(Errno::EIO) => Ok(ReadOutcome::Closed),
                Err(error) => Err(DriverError::Failed(format!(
                    "cannot read the pseudo-terminal: {error}"
                ))),
            };
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<Option<usize>, Errno> {
        loop {
            return match nix::unistd::write(&self.master, bytes) {
                Ok(count) => Ok(Some(count)),
                Err(Errno::EAGAIN) => Ok(None),
                Err(Errno::EINTR) => continue,
                Err(error) => Err(error),
            };
        }
    }

    pub fn resize(&self, columns: u16, rows: u16) -> Result<(), DriverError> {
        let size = winsize(columns, rows);
        let result = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
        if result == -1 {
            return Err(DriverError::Failed(format!(
                "cannot resize the pseudo-terminal: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    fn signal(&self, signal: Signal) {
        let pid = Pid::from_raw(self.child.id() as i32);
        let _ = killpg(pid, signal);
        let _ = kill(pid, signal);
    }

    pub fn terminate(&mut self) {
        if self.exit().is_some() {
            return;
        }
        self.signal(Signal::SIGHUP);
        self.signal(Signal::SIGCONT);
        self.signal(Signal::SIGTERM);
        if self.discard_output_until_exit(Some(Instant::now() + TERMINATE_GRACE)) {
            return;
        }
        self.signal(Signal::SIGKILL);
        self.discard_output_until_exit(None);
    }

    fn discard_output_until_exit(&mut self, deadline: Option<Instant>) -> bool {
        let mut buffer = [0u8; 4096];
        let mut open = true;
        loop {
            if self.exit().is_some() {
                return true;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return false;
            }
            while open {
                match self.read(&mut buffer) {
                    Ok(ReadOutcome::Data(_)) => {}
                    Ok(ReadOutcome::Empty) => break,
                    Ok(ReadOutcome::Closed) | Err(_) => open = false,
                }
            }
            let pause = Duration::from_millis(5);
            if !open || self.wait_readable(false, pause).is_err() {
                std::thread::sleep(pause);
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.terminate();
    }
}
