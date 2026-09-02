// SPDX-License-Identifier: EUPL-1.2

use std::io;
use std::os::fd::{AsFd, AsRawFd as _};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[must_use]
pub fn terminal_size() -> Option<TerminalSize> {
    terminal_size_from_stderr().ok()
}

pub fn terminal_size_for(output: &impl AsFd) -> io::Result<TerminalSize> {
    terminal_size_from_fd(output.as_fd().as_raw_fd())
}

fn terminal_size_from_stderr() -> io::Result<TerminalSize> {
    terminal_size_from_fd(libc::STDERR_FILENO)
}

fn terminal_size_from_fd(fd: libc::c_int) -> io::Result<TerminalSize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: writes winsize when fd refers to a terminal.
    let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, size.as_mut_ptr()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful return means initialisation
    let size = unsafe { size.assume_init() };
    terminal_size_from_parts(size.ws_col, size.ws_row).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "terminal reported a zero-sized viewport",
        )
    })
}

const fn terminal_size_from_parts(cols: u16, rows: u16) -> Option<TerminalSize> {
    if cols == 0 || rows == 0 {
        None
    } else {
        Some(TerminalSize { cols, rows })
    }
}
