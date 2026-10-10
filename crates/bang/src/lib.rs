// SPDX-License-Identifier: EUPL-1.2

//! Typed interactive prompts.
//!
//! Each prompt is a builder created by a free function such as [`select`],
//! [`text`] or [`confirm`]. Its `interact` method runs it on the terminal and
//! returns a [`PromptOutcome`] carrying the typed answer.
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
//! # Behaviour shared by every prompt
//!
//! - Prompts are `!Send`. They hold `Rc` state, so build and run a prompt on
//!   one thread.
//! - `interact` consumes the prompt. Clone it first to ask again.
//! - A cancel, whether Esc or Ctrl-C, maps to [`PromptOutcome::Leave`] rather
//!   than an error. [`PromptOutcome::or_cancel`] turns it back into a
//!   [`ErrorKind::Cancelled`] error when the caller cannot go on without an
//!   answer.
//! - A submitted prompt leaves a dimmed one-line summary in the scrollback,
//!   such as `Shell › bash`, and leaving leaves nothing. Turn it off per prompt
//!   with `summary(false)` or for a driver with
//!   [`Interaction::with_summaries`].
//! - Ended input, a non-interactive terminal and terminal failures are errors,
//!   see [`ErrorKind`].
//! - The entry point takes the text the user sees. The widget id is optional
//!   and set with `.id(...)`.
//! - List prompts need at least one choice and fail with
//!   [`ErrorKind::InvalidConfiguration`] otherwise.
//!
//! # Review keys
//!
//! A review prompt reserves `j` and `k` to move, space and tab to cycle the
//! state, `y` or `c` to confirm, `x` or `n` to deny, `u` to unset, `r` to
//! toggle removed items, enter to submit and esc to leave. Action keys added
//! with `action` must avoid these, compared case-insensitively, and must not
//! repeat.
//!
//! Lower-level widgets, values, sessions, and replay support are deliberately
//! grouped under [`advanced`].
//!
//! # Layering
//!
//! Bang is an interactive layer over `screw`. A bang widget is a `screw`
//! widget that also handles input, and it draws into the cell grid of a `screw`
//! `Surface`. Any terminal renderer can show a bang widget by copying those
//! cells. `screw` is re-exported whole as [`screw`](mod@screw), so a widget
//! author names `Surface`, `RenderCtx`, `Role` and `Theme` through bang and
//! gets the version bang was built against.
//!
//! # Building your own terminal application
//!
//! Two paths go beyond the prompts, and both are reachable from this crate so
//! one tested set of versions covers them.
//!
//! - [`terminal`] is the terminal session toolkit. It has raw mode, screen
//!   entry and restore, signal handling, a pollable event source and the key
//!   decoder, for a full-screen application that owns its own loop.
//! - [`advanced`] has the widgets and the traits to write your own. Render one
//!   into a `screw` `Surface` you compose and flush yourself, to host it inside
//!   a `screw` layout or event loop of your own rather than as a standalone
//!   prompt.

#![warn(missing_docs)]

mod error;
mod input;
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
/// pollable event source ([`TerminalEvents`](bang_terminal::TerminalEvents))
/// and the byte [`Decoder`](bang_terminal::Decoder). See the crate root of
/// `bang-terminal` for how the pieces fit together.
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
