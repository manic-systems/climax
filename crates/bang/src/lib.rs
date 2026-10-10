// SPDX-License-Identifier: EUPL-1.2

//! Interactive terminal prompts layered over `screw`.

#![warn(missing_docs)]

mod error;
mod interaction;
mod live;
mod prompt;
mod session;

pub mod advanced;
pub use screw;

/// Terminal session toolkit.
///
/// Raw mode ([`TerminalModeGuard`](bang_terminal::TerminalModeGuard)), screen
/// entry ([`ScreenGuard`](bang_terminal::ScreenGuard)), signal handling, a
/// pollable event source ([`TerminalEvents`](bang_terminal::TerminalEvents)) and
/// the byte [`Decoder`](bang_terminal::Decoder). See the crate root of
/// `bang-terminal` for how the pieces fit together.
pub mod terminal {
    #[doc(inline)]
    pub use bang_terminal::*;
}

pub use bang_core::{Date, widgets::ReviewState};
pub use error::{Error, ErrorKind, Result};
pub use interaction::Interaction;
pub use prompt::{
    Configurable, MultiSelectConfig, MultiSelectPrompt, PromptOutcome, SearchConfig, SearchPrompt,
    SelectConfig, SelectPrompt, multi_select, search, select,
};
