// SPDX-License-Identifier: EUPL-1.2

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

#[must_use]
pub fn terminal_size() -> Option<TerminalSize> {
    terminal_size_from_stderr()
}

#[cfg(unix)]
fn terminal_size_from_stderr() -> Option<TerminalSize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: writes winsize if stderr is a tty
    let result = unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: successful return means initialisation
    let size = unsafe { size.assume_init() };
    terminal_size_from_parts(size.ws_col, size.ws_row)
}

const fn terminal_size_from_parts(cols: u16, rows: u16) -> Option<TerminalSize> {
    if cols == 0 || rows == 0 {
        None
    } else {
        Some(TerminalSize { cols, rows })
    }
}
