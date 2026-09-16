// SPDX-License-Identifier: EUPL-1.2

//! Typed interactive prompts.
//!
//! The crate root is the normal, typed workflow. Lower-level widgets, values,
//! sessions, and replay support are deliberately grouped under [`advanced`].

mod error;
mod interaction;
mod prompt;

pub mod advanced;
pub mod prelude;

pub use bang_core::widgets::ReviewState;
pub use error::{Error, ErrorKind, Result};
pub use interaction::Interaction;
pub use prompt::{
    Configurable, MultiSelectConfig, MultiSelectPrompt, PromptOutcome, ReviewConfig, ReviewExit,
    ReviewOutcome, ReviewPrompt, ReviewPromptWithActions, Reviewed, SearchConfig, SearchPrompt,
    SelectConfig, SelectPrompt, TextConfig, TextPrompt, multi_select, review, search, select, text,
};
