// SPDX-License-Identifier: EUPL-1.2

use std::io;

/// restore terminal state on drop
/// TODO - should we catch job control then drop and reinstate on resume ?
#[derive(Debug)]
pub struct TerminalModeGuard {
    #[cfg(unix)]
    fd:    libc::c_int,
    #[cfg(unix)]
    saved: libc::termios,
}

impl TerminalModeGuard {
    pub fn activate_stdin() -> io::Result<Self> {
        activate_fd(stdin_fd())
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // SAFETY: self.saved was captured from this fd by tcgetattr
            let _result = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &raw const self.saved) };
        }
    }
}

#[cfg(unix)]
fn activate_fd(fd: libc::c_int) -> io::Result<TerminalModeGuard> {
    let saved = termios(fd)?;
    let mut raw = saved;
    configure_raw_timeout(&mut raw);
    set_termios(fd, &raw)?;
    Ok(TerminalModeGuard { fd, saved })
}

#[cfg(unix)]
const fn stdin_fd() -> libc::c_int {
    libc::STDIN_FILENO
}

#[cfg(unix)]
fn termios(fd: libc::c_int) -> io::Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: termios points to valid writable memory for tcgetattr
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
        return Err(terminal_mode_error());
    }
    // SAFETY: tcgetattr succeeded and initialized the termios value
    Ok(unsafe { termios.assume_init() })
}

#[cfg(unix)]
fn terminal_mode_error() -> io::Error {
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOTTY) {
        return io::Error::new(
            io::ErrorKind::NotConnected,
            "live mode requires terminal stdin; pass --input-bytes for deterministic non-TTY \
             execution",
        );
    }
    error
}

#[cfg(unix)]
fn set_termios(fd: libc::c_int, termios: &libc::termios) -> io::Result<()> {
    // SAFETY: termios is a valid termios struct
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn configure_raw_timeout(termios: &mut libc::termios) {
    // SAFETY: termios is a valid mutable termios struct
    unsafe { libc::cfmakeraw(termios) };
    termios.c_cc[libc::VMIN] = 0;
    termios.c_cc[libc::VTIME] = 1;
}
