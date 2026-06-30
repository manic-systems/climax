// SPDX-License-Identifier: EUPL-1.2

//! translate terminal byte streams into bang input events

mod decoder;
mod mode;
mod runner;
mod screen;
mod signal;
mod size;

pub use decoder::{
    Decoder,
    decode_all,
};
pub use mode::TerminalModeGuard;
pub use runner::{
    RunOutcome,
    SessionRenderer,
    drive_blocking_session,
    drive_tty_session,
    drive_tty_session_with_signals,
};
pub use screen::{
    InlineScreenGuard,
    enter_inline_screen,
    leave_inline_screen,
};
pub use signal::{
    SignalGuard,
    restore_default_and_raise,
};
pub use size::{
    TerminalSize,
    terminal_size,
};
