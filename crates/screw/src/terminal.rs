use std::io::{self, IsTerminal as _};

pub const FALLBACK_WIDTH: usize = 80;

pub fn stderr_is_terminal() -> bool {
    io::stderr().is_terminal()
}

pub fn terminal_width() -> Option<usize> {
    terminal_width_from_stderr()
}

pub fn terminal_width_or_default() -> usize {
    terminal_width().unwrap_or(FALLBACK_WIDTH)
}

#[cfg(unix)]
fn terminal_width_from_stderr() -> Option<usize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: ioctl writes a winsize into the valid out pointer when stderr is tty
    let result = unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) };
    if result == 0 {
        // SAFETY: ioctl returned success, this is init
        let size = unsafe { size.assume_init() };
        terminal_width_from_cols(size.ws_col)
    } else {
        None
    }
}

fn terminal_width_from_cols(cols: u16) -> Option<usize> {
    (cols > 0).then_some(usize::from(cols))
}
