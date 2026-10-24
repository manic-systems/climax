// SPDX-License-Identifier: EUPL-1.2

//! bang-terminal: terminal session toolkit behind the bang prompts
//!
//! take the terminal out of line mode with a [`TerminalModeGuard`], claim the
//! screen with a [`ScreenGuard`] and pull input from [`TerminalEvents`], each
//! guard restores on drop or through `restore` or `leave`, so restore them in
//! reverse order. a [`SignalGuard`] is optional, install it first when signals
//! must be handled and drop it last
//!
//! ```no_run
//! use std::io::{
//!     self,
//!     Write as _,
//! };
//!
//! use bang_terminal::{
//!     RawModeOptions,
//!     ScreenGuard,
//!     ScreenOptions,
//!     TerminalEvents,
//!     TerminalModeGuard,
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
//! [`Decoder`] decodes bytes without any terminal at all

#![warn(missing_docs)]
#![cfg_attr(
    test,
    expect(
        clippy::non_ascii_literal,
        reason = "terminal glyphs read better as themselves than as escapes"
    )
)]

extern crate alloc;

mod cleanup;
mod decoder;
mod events;
mod mode;
mod screen;
mod signal;
#[doc(hidden)] pub mod testing;

pub use cleanup::{
    CleanupFailure,
    CleanupFailures,
    CleanupStage,
};
pub use decoder::Decoder;
pub use events::{
    TerminalEvents,
    TerminalPoll,
    resize_event,
};
pub use mode::{
    RawModeOptions,
    TerminalModeGuard,
};
pub use screen::{
    CursorPolicy,
    ScreenGuard,
    ScreenKind,
    ScreenOptions,
};
pub use signal::{
    Signal,
    SignalGuard,
    SignalPoller,
};
