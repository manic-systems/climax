// SPDX-License-Identifier: EUPL-1.2

//! bang: typed interactive prompts
//!
//! each prompt is a builder made by a free function such as [`select`],
//! [`text`] or [`confirm`], and its `interact` method runs it on the terminal
//! and returns a [`PromptOutcome`]
//!
//! ```no_run
//! use bang::{
//!     PromptOutcome,
//!     select,
//! };
//!
//! # fn main() -> bang::Result<()> {
//! let outcome = select("Pick a shell")
//!     .choice("Bash", "bash")
//!     .choice("Zsh", "zsh")
//!     .interact()?;
//! match outcome {
//!     PromptOutcome::Submit(shell) => println!("chose {shell}"),
//!     PromptOutcome::Leave => println!("left without choosing"),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! prompts are `!Send` and `interact` consumes them. esc and ctrl-c give
//! [`PromptOutcome::Leave`], which [`PromptOutcome::or_cancel`] turns into an
//! [`ErrorKind::Cancelled`] error. ended input, a non-interactive terminal and
//! terminal failures are errors, see [`ErrorKind`]
//!
//! a submitted prompt leaves a dimmed summary line such as `Shell › bash` in
//! the scrollback, off with `summary(false)` or
//! [`Interaction::with_summaries`]. a review prompt reserves `j`, `k`,
//! space, tab, `y`, `c`, `x`, `n`, `u`, `r`, enter and esc, so its `action`
//! keys must avoid them
//!
//! [`terminal`] is the session toolkit for a full-screen application,
//! [`advanced`] has the widget traits, and both render through the re-exported
//! [`screw`](mod@screw)

#![warn(missing_docs)]
#![cfg_attr(
    test,
    expect(
        clippy::non_ascii_literal,
        reason = "terminal glyphs read better as themselves than as escapes"
    )
)]

extern crate alloc;

mod error;
mod input;
mod interaction;
mod live;
mod prompt;
mod session;

pub mod advanced;
pub use screw;

/// terminal session toolkit
///
/// raw mode ([`TerminalModeGuard`](bang_terminal::TerminalModeGuard)), screen
/// entry ([`ScreenGuard`](bang_terminal::ScreenGuard)), signal handling, a
/// pollable event source ([`TerminalEvents`](bang_terminal::TerminalEvents))
/// and the byte [`Decoder`](bang_terminal::Decoder). see the crate root of
/// `bang-terminal` for how the pieces fit together
pub mod terminal {
    #[doc(inline)] pub use bang_terminal::*;
}

pub use bang_core::{
    Date,
    widgets::ReviewState,
};
pub use error::{
    Error,
    ErrorKind,
    Result,
};
pub use input::{
    ConfirmConfig,
    ConfirmPrompt,
    DateConfig,
    DatePrompt,
    NumberConfig,
    NumberPrompt,
    PasswordConfig,
    PasswordPrompt,
    confirm,
    date,
    number,
    password,
};
pub use interaction::Interaction;
pub use prompt::{
    Configurable,
    MultiSelectConfig,
    MultiSelectPrompt,
    PromptOutcome,
    ReviewConfig,
    ReviewExit,
    ReviewOutcome,
    ReviewPrompt,
    ReviewPromptWithActions,
    Reviewed,
    SearchConfig,
    SearchPrompt,
    SelectConfig,
    SelectPrompt,
    TextConfig,
    TextPrompt,
    multi_select,
    review,
    search,
    select,
    text,
};
