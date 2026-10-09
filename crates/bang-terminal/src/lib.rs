// SPDX-License-Identifier: EUPL-1.2

//! translate terminal byte streams into bang input events

mod cleanup;
mod decoder;
mod mode;
mod screen;
mod signal;
#[doc(hidden)]
pub mod testing;

pub use cleanup::{CleanupFailure, CleanupFailures, CleanupStage};
pub use decoder::{
    Decoder,
    decode_all,
};
pub use mode::{RawModeOptions, TerminalModeGuard};
pub use screen::{CursorPolicy, ScreenGuard, ScreenKind, ScreenOptions};
pub use signal::{Signal, SignalGuard, SignalPoller};
