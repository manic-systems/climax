// SPDX-License-Identifier: EUPL-1.2

//! translate terminal byte streams into bang input events
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
mod mode;
mod screen;
mod signal;
#[doc(hidden)] pub mod testing;

pub use cleanup::{
    CleanupFailure,
    CleanupFailures,
    CleanupStage,
};
pub use decoder::{
    Decoder,
    decode_all,
};
pub use mode::{
    RawModeOptions,
    TerminalModeGuard,
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
