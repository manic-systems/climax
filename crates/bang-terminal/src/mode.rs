// SPDX-License-Identifier: EUPL-1.2

use std::io;
use std::os::fd::{AsFd, AsRawFd as _, FromRawFd as _, OwnedFd};

/// Read behavior installed alongside the usual raw terminal flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawModeOptions {
    minimum_bytes: u8,
    timeout_deciseconds: u8,
}

impl RawModeOptions {
    /// Block until at least one byte is available. This is the appropriate
    /// mode when readiness is managed with `poll(2)`.
    #[must_use]
    pub const fn blocking() -> Self {
        Self {
            minimum_bytes: 1,
            timeout_deciseconds: 0,
        }
    }

    /// Allow reads to return zero after a termios timeout.
    #[must_use]
    pub const fn timed(timeout_deciseconds: u8) -> Self {
        Self {
            minimum_bytes: 0,
            timeout_deciseconds,
        }
    }

    #[must_use]
    pub const fn minimum_bytes(self) -> u8 {
        self.minimum_bytes
    }

    #[must_use]
    pub const fn timeout_deciseconds(self) -> u8 {
        self.timeout_deciseconds
    }
}

impl Default for RawModeOptions {
    fn default() -> Self {
        Self::blocking()
    }
}

/// restore terminal state on drop
/// TODO - should we catch job control then drop and reinstate on resume ?
#[derive(Debug)]
pub struct TerminalModeGuard {
    fd: OwnedFd,
    saved: libc::termios,
    active: bool,
}

impl TerminalModeGuard {
    pub fn activate(input: &(impl AsFd + ?Sized), options: RawModeOptions) -> io::Result<Self> {
        activate_fd(input.as_fd().as_raw_fd(), options)
    }

    pub fn restore(mut self) -> io::Result<()> {
        self.restore_active()
    }

    fn restore_active(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        set_termios(self.fd.as_raw_fd(), &self.saved)?;
        self.active = false;
        Ok(())
    }
}

impl TerminalModeGuard {
    pub fn activate_stdin() -> io::Result<Self> {
        activate_fd(libc::STDIN_FILENO, RawModeOptions::blocking())
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _result = self.restore_active();
    }
}

fn activate_fd(fd: libc::c_int, options: RawModeOptions) -> io::Result<TerminalModeGuard> {
    // A duplicate keeps the terminal available for restoration without
    // borrowing the caller's handle for the lifetime of the guard.
    let owned_fd = duplicate(fd)?;
    let saved = termios(owned_fd.as_raw_fd())?;
    let mut raw = saved;
    configure_raw(&mut raw, options);
    set_termios(owned_fd.as_raw_fd(), &raw)?;
    Ok(TerminalModeGuard {
        fd: owned_fd,
        saved,
        active: true,
    })
}

fn duplicate(fd: libc::c_int) -> io::Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC returns a new descriptor on success.
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicated < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: duplicated is a newly owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn termios(fd: libc::c_int) -> io::Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: termios points to valid writable memory for tcgetattr
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
        return Err(terminal_mode_error());
    }
    // SAFETY: tcgetattr succeeded and initialized the termios value
    Ok(unsafe { termios.assume_init() })
}

fn terminal_mode_error() -> io::Error {
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOTTY) {
        return io::Error::new(
            io::ErrorKind::NotConnected,
            "raw mode requires a terminal input handle",
        );
    }
    error
}

fn set_termios(fd: libc::c_int, termios: &libc::termios) -> io::Result<()> {
    // SAFETY: termios is a valid termios struct
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn configure_raw(termios: &mut libc::termios, options: RawModeOptions) {
    // SAFETY: termios is a valid mutable termios struct
    unsafe { libc::cfmakeraw(termios) };
    termios.c_cc[libc::VMIN] = options.minimum_bytes;
    termios.c_cc[libc::VTIME] = options.timeout_deciseconds;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_read_policies_are_explicit() {
        assert_eq!(RawModeOptions::blocking().minimum_bytes(), 1);
        assert_eq!(RawModeOptions::blocking().timeout_deciseconds(), 0);
        assert_eq!(RawModeOptions::timed(3).minimum_bytes(), 0);
        assert_eq!(RawModeOptions::timed(3).timeout_deciseconds(), 3);
    }
}
