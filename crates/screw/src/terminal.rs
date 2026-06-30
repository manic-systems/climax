use std::io::{
    self,
    IsTerminal as _,
};

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

#[cfg(not(unix))]
fn terminal_width_from_stderr() -> Option<usize> {
    None
}

fn terminal_width_from_cols(cols: u16) -> Option<usize> {
    (cols > 0).then_some(usize::from(cols))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_columns_are_unknown() {
        assert_eq!(terminal_width_from_cols(0), None);
    }

    #[test]
    fn nonzero_columns_are_width() {
        assert_eq!(terminal_width_from_cols(120), Some(120));
    }

    #[test]
    fn default_width_is_available_without_tty() {
        assert!(terminal_width_or_default() >= 1);
    }
}
