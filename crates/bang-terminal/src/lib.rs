// SPDX-License-Identifier: EUPL-1.2

//! translate terminal byte streams into bang input events

mod decoder;
mod events;
mod mode;
mod runner;
mod screen;
mod signal;
mod size;

pub use decoder::{Decoder, decode_all};
pub use events::{
    Clock, NoSignals, NoTerminalSize, ProcessTerminalSize, SignalSource, SystemClock,
    TerminalEvents, TerminalPoll, TerminalSizeSource,
};
pub use mode::{RawModeOptions, TerminalModeGuard};
pub use runner::{
    RunOutcome, SessionRenderer, drive_blocking_session, drive_tty_session,
    drive_tty_session_with_signals,
};
pub use screen::{
    CursorPolicy, InlineScreenGuard, ScreenFailures, ScreenGuard, ScreenKind, ScreenOptions,
    enter_inline_screen, leave_inline_screen,
};
pub use signal::{SignalFailures, SignalGuard, restore_default_and_raise};
pub use size::terminal_size_for;
pub use size::{TerminalSize, terminal_size};
