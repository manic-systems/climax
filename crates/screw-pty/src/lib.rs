// SPDX-License-Identifier: EUPL-1.2

//! screw-pty: a strict model of the terminal output screw emits, for testing
//!
//! [`EmittedScreen`] consumes the bytes a screw renderer writes and rebuilds
//! the screen, so a test can assert on cells, styles and cursor state. it
//! understands the sequences screw emits, plus bracketed paste and the
//! alternate screen that terminal sessions add, and rejects the rest with a
//! [`ScreenError`]
//!
//! not a general terminal emulator

#![cfg_attr(
    test,
    expect(
        clippy::non_ascii_literal,
        reason = "terminal glyphs read better as themselves than as escapes"
    )
)]

#[cfg(not(unix))]
compile_error!("screw-pty supports unix targets only");

mod ansi;
mod width;

pub use ansi::{
    EmittedScreen,
    ScreenCell,
    ScreenError,
};
