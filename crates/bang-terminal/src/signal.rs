// SPDX-License-Identifier: EUPL-1.2

use std::{
    fmt, io,
    os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
    sync::atomic::{AtomicI32, Ordering},
};

static SIGNAL_WRITE_FD: AtomicI32 = AtomicI32::new(-1);
const INSTALLING: libc::c_int = -2;

pub const TERMINAL_SIGNALS: &[libc::c_int] =
    &[libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

/// Multiple failures observed while restoring process signal handlers.
#[derive(Debug)]
pub struct SignalFailures {
    failures: Vec<io::Error>,
}

impl SignalFailures {
    #[must_use]
    pub fn failures(&self) -> &[io::Error] {
        &self.failures
    }
}

impl fmt::Display for SignalFailures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, failure) in self.failures.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{failure}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SignalFailures {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.failures
            .first()
            .map(|failure| failure as &(dyn std::error::Error + 'static))
    }
}

/// convert signals into events
#[derive(Debug)]
pub struct SignalGuard {
    read_fd: OwnedFd,
    _write_fd: OwnedFd,
    previous: Vec<(libc::c_int, libc::sigaction)>,
    active: bool,
}

impl SignalGuard {
    pub fn install_terminal_handlers() -> io::Result<Self> {
        install_terminal_handlers()
    }

    pub fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        poll_signal(self)
    }

    /// Restore the previous process handlers before releasing the self-pipe.
    pub fn restore(mut self) -> io::Result<()> {
        self.restore_active()
    }

    fn restore_active(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        SIGNAL_WRITE_FD.store(-1, Ordering::SeqCst);
        let mut failures = Vec::new();
        for (signal, previous) in &self.previous {
            // SAFETY: previous was returned by sigaction for this signal.
            if unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()) } != 0 {
                failures.push(io::Error::last_os_error());
            }
        }
        self.active = false;
        match failures.len() {
            0 => Ok(()),
            1 => Err(failures.pop().expect("one failure remains")),
            _ => {
                let kind = failures[0].kind();
                Err(io::Error::new(kind, SignalFailures { failures }))
            },
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        let _result = self.restore_active();
    }
}

fn install_terminal_handlers() -> io::Result<SignalGuard> {
    SIGNAL_WRITE_FD
        .compare_exchange(-1, INSTALLING, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "terminal signal handlers are already installed",
            )
        })?;
    let installed = install_terminal_handlers_exclusive();
    if installed.is_err() {
        SIGNAL_WRITE_FD.store(-1, Ordering::SeqCst);
    }
    installed
}

fn install_terminal_handlers_exclusive() -> io::Result<SignalGuard> {
    let (read_fd, write_fd) = pipe()?;
    set_nonblocking(&read_fd)?;
    set_nonblocking(&write_fd)?;
    set_cloexec(&read_fd)?;
    set_cloexec(&write_fd)?;

    SIGNAL_WRITE_FD.store(write_fd.as_raw_fd(), Ordering::SeqCst);

    let mut previous = Vec::new();
    for signal in TERMINAL_SIGNALS {
        match install_handler(*signal) {
            Ok(old) => previous.push((*signal, old)),
            Err(error) => {
                for (signal, old) in &previous {
                    // SAFETY: old was returned by sigaction
                    let _result = unsafe { libc::sigaction(*signal, old, std::ptr::null_mut()) };
                }
                SIGNAL_WRITE_FD.store(-1, Ordering::SeqCst);
                return Err(error);
            },
        }
    }

    Ok(SignalGuard {
        read_fd,
        _write_fd: write_fd,
        previous,
        active: true,
    })
}

fn poll_signal(guard: &mut SignalGuard) -> io::Result<Option<i32>> {
    let mut byte = 0_u8;

    loop {
        // SAFETY: byte is valid and read_fd is owned by guard.
        let read = unsafe {
            libc::read(
                guard.read_fd.as_raw_fd(),
                (&raw mut byte).cast::<libc::c_void>(),
                1,
            )
        };
        if read > 0 {
            if byte != 0 {
                return Ok(Some(i32::from(byte)));
            }
            continue;
        }
        if read == 0 {
            return Ok(None);
        }

        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => return Ok(None),
            Some(libc::EINTR) => {},
            _ => return Err(error),
        }
    }
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds are valid
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe returned two new descriptors owned by the caller.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let fd = fd.as_raw_fd();
    // SAFETY: fd is owned by guard
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd and flags are valid
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_cloexec(fd: &OwnedFd) -> io::Result<()> {
    let fd = fd.as_raw_fd();
    // SAFETY: fd is owned by guard
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd and flags are valid
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn install_handler(signal: libc::c_int) -> io::Result<libc::sigaction> {
    // SAFETY: zeroed sigaction is immediately initialized
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = signal_handler as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: sa_mask points to valid memory
    if unsafe { libc::sigemptyset(&raw mut action.sa_mask) } != 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: zeroed storage will be written by sigaction
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    // SAFETY: both pointers are valid
    if unsafe { libc::sigaction(signal, &raw const action, &raw mut previous) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(previous)
}

extern "C" fn signal_handler(signal: libc::c_int) {
    let fd = SIGNAL_WRITE_FD.load(Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    let byte = u8::try_from(signal).unwrap_or(0);
    // SAFETY: write is safe
    let _result = unsafe { libc::write(fd, (&raw const byte).cast::<libc::c_void>(), 1) };
}

/// Install the platform default disposition for `signal`, then raise it again.
///
/// This deliberately does not restore or invoke a disposition that was active
/// before Bang installed its terminal handlers. In particular, a previous
/// custom handler or `SIG_IGN` disposition is not called after terminal
/// cleanup. Callers should use this only to resume the conventional terminal
/// signal outcome after restoring process-visible terminal state.
pub fn restore_default_and_raise(signal: i32) -> io::Result<()> {
    let signal = libc::c_int::try_from(signal)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "signal out of range"))?;
    // SAFETY: SIG_DFL is a valid value
    if unsafe { libc::signal(signal, libc::SIG_DFL) } == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raising a signal is safe
    if unsafe { libc::raise(signal) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_signal_handlers_are_a_process_singleton() {
        let guard = SignalGuard::install_terminal_handlers().unwrap();
        let error = SignalGuard::install_terminal_handlers().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        drop(guard);

        SignalGuard::install_terminal_handlers()
            .unwrap()
            .restore()
            .unwrap();
    }
}
