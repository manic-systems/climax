// SPDX-License-Identifier: EUPL-1.2

//! Test support shared with the `bang` crate. Not part of the public API.

use std::{
    io,
    os::fd::{FromRawFd as _, OwnedFd},
    sync::Mutex,
};

/// Held by every test that installs the process-wide signal handlers.
pub static SIGNAL_LOCK: Mutex<()> = Mutex::new(());

/// A pseudo-terminal with the given size, returned as `(master, slave)`.
pub fn pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master = 0;
    let mut slave = 0;
    // SAFETY: both out pointers are valid and the optional arguments are null.
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if opened != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty returned two descriptors that nothing else owns.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    resize_pty(&master, cols, rows)?;
    Ok((master, slave))
}

pub fn resize_pty(master: &OwnedFd, cols: u16, rows: u16) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: size is a valid winsize for the duration of the call.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
