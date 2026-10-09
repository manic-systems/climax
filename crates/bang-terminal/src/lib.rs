// SPDX-License-Identifier: EUPL-1.2

//! Terminal session toolkit behind the bang prompts.
//!
//! Bang is an interactive layer over `screw`, and this crate is the terminal
//! half of it. It owns raw mode, the screen, signals and input decoding, and
//! knows nothing about how a widget draws. Driving a widget over these pieces
//! is the job of the `bang` crate.
//!
//! The pieces compose in a fixed order. Install a [`SignalGuard`] first so a
//! terminal signal is queued instead of killing the process mid-session. Take
//! the terminal out of line mode with a [`TerminalModeGuard`], claim the screen
//! with a [`ScreenGuard`], then pull decoded input from [`TerminalEvents`],
//! handing it the guard's [`SignalPoller`]. Each guard restores what it changed
//! on drop, or earlier through its `restore` or `leave` method when the caller
//! wants to see the failure. Drop in the reverse order, so the signal handlers
//! come back last.
//!
//! ```no_run
//! use std::io::{self, Write as _};
//!
//! use bang_terminal::{
//!     RawModeOptions, ScreenGuard, ScreenOptions, TerminalEvents, TerminalModeGuard,
//!     TerminalPoll,
//! };
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let stdin = io::stdin();
//!     let raw = TerminalModeGuard::activate(&stdin, RawModeOptions::blocking())?;
//!     let mut stdout = io::stdout();
//!     let mut screen = ScreenGuard::enter(&mut stdout, ScreenOptions::full_screen())?;
//!     let mut events = TerminalEvents::pollable(stdin)?;
//!
//!     if let TerminalPoll::Event(event) = events.next_event()? {
//!         writeln!(screen.writer(), "{event:?}\r")?;
//!     }
//!
//!     screen.leave()?;
//!     raw.restore()?;
//!     Ok(())
//! }
//! ```
//!
//! [`Decoder`] decodes bytes without any terminal at all.

#![warn(missing_docs)]

mod cleanup;
mod decoder;
mod events;
mod mode;
mod screen;
mod signal;
#[doc(hidden)]
pub mod testing;

pub use cleanup::{CleanupFailure, CleanupFailures, CleanupStage};
pub use decoder::Decoder;
pub use events::{TerminalEvents, TerminalPoll, resize_event};
pub use mode::{RawModeOptions, TerminalModeGuard};
pub use screen::{CursorPolicy, ScreenGuard, ScreenKind, ScreenOptions};
pub use signal::{Signal, SignalGuard, SignalPoller};
