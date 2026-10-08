// SPDX-License-Identifier: EUPL-1.2

//! A strict model of the terminal output screw emits, for testing.
//!
//! [`EmittedScreen`] consumes the bytes a screw renderer writes and rebuilds the screen they
//! would produce, so a test can assert on cells, styles and cursor state. It understands only the
//! sequences screw emits and rejects everything else with a [`ScreenError`], which means an
//! unexpected escape sequence fails a test and is never silently ignored.
//!
//! It is not a general terminal emulator.

mod ansi;

pub use ansi::{EmittedScreen, ScreenCell, ScreenError};
