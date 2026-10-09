// SPDX-License-Identifier: EUPL-1.2

use std::io::{self, Write};

use crate::cleanup::{CleanupFailure, CleanupFailures, CleanupStage, staged};

const ENTER_ALTERNATE: &[u8] = b"\x1b[?1049h";
const LEAVE_ALTERNATE: &[u8] = b"\x1b[?1049l";
const ENABLE_BRACKETED_PASTE: &[u8] = b"\x1b[?2004h";
const DISABLE_BRACKETED_PASTE: &[u8] = b"\x1b[?2004l";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const CLEAR_INLINE: &[u8] = b"\r\x1b[2K";
const CANCEL_SEQUENCE: &[u8] = b"\x18";
const STATE_ACTIVE: u8 = 1 << 0;
const STATE_ALTERNATE: u8 = 1 << 1;
const STATE_PASTE: u8 = 1 << 2;
const STATE_CURSOR: u8 = 1 << 3;
const STATE_UNCERTAIN: u8 = 1 << 4;

/// Where a session draws.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScreenKind {
    /// Below the current cursor line, leaving scrollback intact.
    #[default]
    Inline,
    /// On the alternate screen buffer, which the terminal restores on leave.
    Alternate,
}

/// What happens to the cursor while the screen is held.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorPolicy {
    /// Leave the cursor as the terminal had it.
    Preserve,
    /// Hide the cursor until the guard leaves.
    #[default]
    Hide,
}

/// Modes a [`ScreenGuard`] negotiates, built from [`ScreenOptions::inline`] or
/// [`ScreenOptions::full_screen`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenOptions {
    kind: ScreenKind,
    cursor: CursorPolicy,
    bracketed_paste: bool,
    clear_inline_on_leave: bool,
}

impl ScreenOptions {
    /// Inline session with the cursor hidden, bracketed paste on and the line
    /// cleared on leave.
    #[must_use]
    pub const fn inline() -> Self {
        Self {
            kind: ScreenKind::Inline,
            cursor: CursorPolicy::Hide,
            bracketed_paste: true,
            clear_inline_on_leave: true,
        }
    }

    /// Alternate screen with the cursor hidden and bracketed paste off.
    #[must_use]
    pub const fn full_screen() -> Self {
        Self {
            kind: ScreenKind::Alternate,
            cursor: CursorPolicy::Hide,
            bracketed_paste: false,
            clear_inline_on_leave: false,
        }
    }

    /// Choose whether the cursor is hidden while the screen is held.
    #[must_use]
    pub const fn cursor(mut self, cursor: CursorPolicy) -> Self {
        self.cursor = cursor;
        self
    }

    /// Ask the terminal to bracket pasted text so it arrives as one event.
    #[must_use]
    pub const fn bracketed_paste(mut self, enabled: bool) -> Self {
        self.bracketed_paste = enabled;
        self
    }

    /// Erase the inline line when the guard leaves. Alternate screens ignore it.
    #[must_use]
    pub const fn clear_inline_on_leave(mut self, clear: bool) -> Self {
        self.clear_inline_on_leave = clear;
        self
    }
}

impl Default for ScreenOptions {
    fn default() -> Self {
        Self::inline()
    }
}

/// Negotiates screen modes on one writer and restores every enabled mode.
pub struct ScreenGuard<'a, W>
where
    W: Write + ?Sized,
{
    output: &'a mut W,
    options: ScreenOptions,
    state: u8,
}

impl<'a, W> ScreenGuard<'a, W>
where
    W: Write + ?Sized,
{
    /// Write the escape sequences `options` asks for. A failure part way
    /// through undoes what was already entered.
    ///
    /// When the rollback fails as well, the returned error wraps
    /// [`CleanupFailures`] holding the original failure first.
    pub fn enter(output: &'a mut W, options: ScreenOptions) -> io::Result<Self> {
        let mut guard = Self {
            output,
            options,
            state: STATE_ACTIVE,
        };
        let entered = guard.enter_modes();
        if let Err(error) = entered {
            return match guard.leave_active() {
                Ok(()) => Err(error),
                Err(rollback) => {
                    let kind = error.kind();
                    let mut failures = vec![CleanupFailure::new(CleanupStage::Screen, error)];
                    failures.extend(rollback.into_failures());
                    Err(io::Error::new(kind, CleanupFailures::new(failures)))
                },
            };
        }
        Ok(guard)
    }

    /// The wrapped writer, for drawing while the modes are active.
    pub const fn writer(&mut self) -> &mut W {
        self.output
    }

    /// Undo every entered mode now and report any failure, which drop cannot.
    pub fn leave(mut self) -> Result<(), CleanupFailures> {
        self.leave_active()
    }

    fn enter_modes(&mut self) -> io::Result<()> {
        if self.options.kind == ScreenKind::Alternate {
            self.enable_mode(STATE_ALTERNATE, ENTER_ALTERNATE)?;
        }
        if self.options.bracketed_paste {
            self.enable_mode(STATE_PASTE, ENABLE_BRACKETED_PASTE)?;
        }
        if self.options.cursor == CursorPolicy::Hide {
            self.enable_mode(STATE_CURSOR, HIDE_CURSOR)?;
        }
        self.output.flush()
    }

    fn enable_mode(&mut self, state: u8, sequence: &[u8]) -> io::Result<()> {
        // write_all may report an error after writing a prefix. Mark the mode
        // conservatively before the attempt so rollback always emits its
        // inverse, even when the terminal's resulting state is unknowable.
        self.state |= state;
        if let Err(error) = self.output.write_all(sequence) {
            self.state |= STATE_UNCERTAIN;
            return Err(error);
        }
        Ok(())
    }

    fn leave_active(&mut self) -> Result<(), CleanupFailures> {
        if self.state & STATE_ACTIVE == 0 {
            return Ok(());
        }
        let mut failures = Vec::new();
        self.cancel_partial(&mut failures);
        if self.options.kind == ScreenKind::Inline && self.options.clear_inline_on_leave {
            attempt(&mut failures, self.output.write_all(CLEAR_INLINE));
        }
        self.disable_mode(STATE_CURSOR, SHOW_CURSOR, &mut failures);
        self.disable_mode(STATE_PASTE, DISABLE_BRACKETED_PASTE, &mut failures);
        self.disable_mode(STATE_ALTERNATE, LEAVE_ALTERNATE, &mut failures);
        attempt(&mut failures, self.output.flush());
        if failures.is_empty() {
            self.state &= !(STATE_ACTIVE | STATE_UNCERTAIN);
        }
        staged(CleanupStage::Screen, failures)
    }

    fn disable_mode(&mut self, state: u8, sequence: &[u8], failures: &mut Vec<io::Error>) {
        if self.state & state == 0 {
            return;
        }
        self.cancel_partial(failures);
        if self.state & STATE_UNCERTAIN != 0 {
            return;
        }
        match self.output.write_all(sequence) {
            Ok(()) => self.state &= !state,
            Err(error) => {
                self.state |= STATE_UNCERTAIN;
                attempt(failures, Err(error));
            },
        }
    }

    fn cancel_partial(&mut self, failures: &mut Vec<io::Error>) {
        if self.state & STATE_UNCERTAIN == 0 {
            return;
        }
        match self.output.write_all(CANCEL_SEQUENCE) {
            Ok(()) => self.state &= !STATE_UNCERTAIN,
            Err(error) => attempt(failures, Err(error)),
        }
    }
}

impl<W> Drop for ScreenGuard<'_, W>
where
    W: Write + ?Sized,
{
    fn drop(&mut self) {
        let _result = self.leave_active();
    }
}

fn attempt(failures: &mut Vec<io::Error>, result: io::Result<()>) {
    if let Err(error) = result {
        failures.push(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_screen_negotiates_and_restores_every_mode() {
        let mut output = Vec::new();
        {
            let mut guard = ScreenGuard::enter(
                &mut output,
                ScreenOptions::full_screen().bracketed_paste(true),
            )
            .unwrap();
            guard.writer().write_all(b"frame").unwrap();
            guard.leave().unwrap();
        }
        assert_eq!(
            output,
            b"\x1b[?1049h\x1b[?2004h\x1b[?25lframe\x1b[?25h\x1b[?2004l\x1b[?1049l",
        );
    }

    #[test]
    fn partial_entry_rolls_back_modes_that_were_enabled() {
        for fail_at in 0..=3 {
            let mut output = FailingWriter::new(fail_at);
            let failed = ScreenGuard::enter(
                &mut output,
                ScreenOptions::full_screen().bracketed_paste(true),
            )
            .is_err();
            assert!(failed, "write/flush boundary {fail_at} should fail");
            assert!(
                output.attempts > fail_at + 1,
                "rollback should continue after boundary {fail_at}",
            );
        }
    }

    #[test]
    fn inline_screen_enables_paste_by_default_and_can_disable_it() {
        let mut default_output = Vec::new();
        ScreenGuard::enter(&mut default_output, ScreenOptions::inline())
            .unwrap()
            .leave()
            .unwrap();
        assert_eq!(
            default_output,
            b"\x1b[?2004h\x1b[?25l\r\x1b[2K\x1b[?25h\x1b[?2004l",
        );

        let mut disabled = Vec::new();
        ScreenGuard::enter(
            &mut disabled,
            ScreenOptions::inline().bracketed_paste(false),
        )
        .unwrap()
        .leave()
        .unwrap();
        assert_eq!(disabled, b"\x1b[?25l\r\x1b[2K\x1b[?25h");
    }

    #[test]
    fn screen_guard_composes_with_a_type_erased_writer() {
        let mut bytes = Vec::new();
        let output: &mut dyn Write = &mut bytes;
        let mut screen = ScreenGuard::enter(
            output,
            ScreenOptions::inline()
                .cursor(CursorPolicy::Preserve)
                .bracketed_paste(false)
                .clear_inline_on_leave(false),
        )
        .unwrap();
        screen.writer().write_all(b"frame").unwrap();
        screen.leave().unwrap();
        assert_eq!(bytes, b"frame");
    }

    #[test]
    fn partial_control_sequence_is_cancelled_before_rollback() {
        let mut output = PartialWriter::default();
        assert!(ScreenGuard::enter(&mut output, ScreenOptions::full_screen()).is_err());
        assert!(
            output
                .bytes
                .windows(CANCEL_SEQUENCE.len())
                .any(|bytes| bytes == CANCEL_SEQUENCE),
        );
        assert!(
            output
                .bytes
                .windows(LEAVE_ALTERNATE.len())
                .any(|bytes| bytes == LEAVE_ALTERNATE),
        );
    }

    #[test]
    fn leave_attempts_every_restoration_after_a_failure() {
        let mut output = FailingWriter::new(4);
        let guard = ScreenGuard::enter(
            &mut output,
            ScreenOptions::full_screen().bracketed_paste(true),
        )
        .unwrap();
        assert!(guard.leave().is_err());
        assert_eq!(
            output.attempts, 11,
            "the consuming leave call retries unfinished restoration from Drop",
        );
    }

    struct FailingWriter {
        bytes: Vec<u8>,
        attempts: usize,
        fail_at: usize,
    }

    impl FailingWriter {
        const fn new(fail_at: usize) -> Self {
            Self {
                bytes: Vec::new(),
                attempts: 0,
                fail_at,
            }
        }
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let attempt = self.attempts;
            self.attempts += 1;
            if attempt == self.fail_at {
                return Err(io::Error::other("injected write failure"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            let attempt = self.attempts;
            self.attempts += 1;
            if attempt == self.fail_at {
                return Err(io::Error::other("injected flush failure"));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct PartialWriter {
        bytes: Vec<u8>,
        writes: usize,
    }

    impl Write for PartialWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let write = self.writes;
            self.writes += 1;
            if write == 0 {
                let partial = bytes.len().min(3);
                self.bytes.extend_from_slice(&bytes[..partial]);
                return Ok(partial);
            }
            if write == 1 {
                return Err(io::Error::other("injected error after partial write"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
