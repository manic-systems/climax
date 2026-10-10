// SPDX-License-Identifier: EUPL-1.2

use std::{
    io,
    os::fd::{
        AsFd,
        OwnedFd,
    },
};

use rustix::{
    io::{
        Errno,
        fcntl_dupfd_cloexec,
    },
    termios::{
        OptionalActions,
        OutputModes,
        SpecialCodeIndex,
        Termios,
        tcgetattr,
        tcsetattr,
    },
};

use crate::cleanup::{
    CleanupFailures,
    CleanupStage,
    staged,
};

/// Read behavior installed alongside the usual raw terminal flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawModeOptions {
    minimum_bytes:       u8,
    timeout_deciseconds: u8,
}

impl RawModeOptions {
    /// Block until at least one byte is available. This is the appropriate
    /// mode when readiness is managed with `poll(2)`.
    #[must_use]
    pub const fn blocking() -> Self {
        Self {
            minimum_bytes:       1,
            timeout_deciseconds: 0,
        }
    }

    /// Bytes a read waits for before returning (`VMIN`).
    #[must_use]
    pub const fn minimum_bytes(self) -> u8 {
        self.minimum_bytes
    }

    /// Read timeout in tenths of a second (`VTIME`), zero for none.
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

/// Holds a terminal in raw mode and restores the saved settings on drop.
///
/// Output post-processing stays on, so a bare newline written while the guard
/// is held, such as a panic message, still returns the carriage.
#[derive(Debug)]
pub struct TerminalModeGuard {
    fd:     OwnedFd,
    saved:  Termios,
    active: bool,
}

impl TerminalModeGuard {
    /// Switch the terminal behind `input` to raw mode, remembering its
    /// settings.
    pub fn activate(input: &(impl AsFd + ?Sized), options: RawModeOptions) -> io::Result<Self> {
        activate_fd(input, options)
    }

    /// Restore the saved settings now and report any failure, which drop
    /// cannot.
    pub fn restore(mut self) -> Result<(), CleanupFailures> {
        self.restore_active()
    }

    fn restore_active(&mut self) -> Result<(), CleanupFailures> {
        if !self.active {
            return Ok(());
        }
        let restored = tcsetattr(&self.fd, OptionalActions::Now, &self.saved);
        staged(
            CleanupStage::RawMode,
            restored.err().map(io::Error::from).into_iter().collect(),
        )?;
        self.active = false;
        Ok(())
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _result = self.restore_active();
    }
}

fn activate_fd(
    terminal: &(impl AsFd + ?Sized),
    options: RawModeOptions,
) -> io::Result<TerminalModeGuard> {
    // A duplicate keeps the terminal available for restoration without
    // borrowing the caller's handle for the lifetime of the guard.
    let owned_fd = fcntl_dupfd_cloexec(terminal, 0)?;
    let saved = tcgetattr(&owned_fd).map_err(terminal_mode_error)?;
    let mut raw = saved.clone();
    raw.make_raw();
    raw.output_modes |= OutputModes::OPOST | OutputModes::ONLCR;
    raw.special_codes[SpecialCodeIndex::VMIN] = options.minimum_bytes;
    raw.special_codes[SpecialCodeIndex::VTIME] = options.timeout_deciseconds;
    tcsetattr(&owned_fd, OptionalActions::Now, &raw)?;
    Ok(TerminalModeGuard {
        fd: owned_fd,
        saved,
        active: true,
    })
}

fn terminal_mode_error(errno: Errno) -> io::Error {
    if errno == Errno::NOTTY {
        return io::Error::new(
            io::ErrorKind::NotConnected,
            "raw mode requires a terminal input handle",
        );
    }
    errno.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_mode_keeps_output_post_processing() {
        let (_master, slave) = crate::testing::pty(80, 24).unwrap();
        let guard = TerminalModeGuard::activate(&slave, RawModeOptions::blocking()).unwrap();

        let raw = tcgetattr(&slave).unwrap();
        assert!(
            raw.output_modes
                .contains(OutputModes::OPOST | OutputModes::ONLCR)
        );
        assert!(!raw.local_modes.contains(rustix::termios::LocalModes::ECHO));
        guard.restore().unwrap();
    }

    #[test]
    fn raw_read_policies_are_explicit() {
        assert_eq!(RawModeOptions::blocking().minimum_bytes(), 1);
        assert_eq!(RawModeOptions::blocking().timeout_deciseconds(), 0);
    }
}
