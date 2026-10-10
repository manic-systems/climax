// SPDX-License-Identifier: EUPL-1.2

//! Batteries-included application facade over `pound`, `screw`, and `bang`.
//!
//! The crate root and [`prelude`] contain the ordinary application workflow.
//! Each component is also re-exported whole behind the feature that enables
//! it, as `climax::pound` under `parse`, `climax::bang` under `interactive`,
//! `climax::screw` under `render`, and `climax::serde` under `structured`. An
//! application needs no direct dependency on any of them, and the versions it
//! sees are the ones `climax` was tested against.
//!
//! The prompt builders on [`Context`] return the prompt types, which are
//! re-exported at the root next to their config types, `PromptOutcome` and
//! the review types, so a signature such as `fn ask(cx: &Context) -> SelectPrompt<String>`
//! needs no path through `climax::bang`.
//!
//! The `derive` feature provides `#[derive(climax::Parse)]` and
//! `#[derive(climax::ValueEnum)]`, whose expansion refers to
//! `::climax::pound`. With `structured` as well, `#[climax::serde(Serialize)]`
//! derives serde's traits rooted at `::climax::serde` for the same reason.
//!
//! `main` and [`main_with`] report errors and choose the process exit code,
//! and [`ResultExt`] adds `.context("reading the count")` to foreign
//! `Result`s. The exit-code mapping is listed below.

#![cfg_attr(
    all(
        feature = "derive",
        feature = "interactive",
        feature = "render",
        feature = "structured"
    ),
    doc = include_str!("../README.md")
)]
#![doc = include_str!("../docs/facade.md")]
#![warn(missing_docs)]

mod app;
/// The application error type and its categories.
pub mod error;
pub mod output;
pub mod prelude;
mod sync;
/// Terminal capability facts, policy overrides and terminal applications.
pub mod terminal;

/// Transient status lines drawn on the shared renderer.
#[cfg(feature = "render")]
pub mod status;

pub use app::{
    Context,
    main_with,
    run_with,
};
#[cfg(feature = "parse")]
pub use app::{main, try_run, try_run_from};
#[cfg(feature = "interactive")]
pub use bang::{
    Configurable, ConfirmConfig, ConfirmPrompt, Date, DateConfig, DatePrompt, MultiSelectConfig,
    MultiSelectPrompt, NumberConfig, NumberPrompt, PasswordConfig, PasswordPrompt, PromptOutcome,
    ReviewExit, ReviewOutcome, ReviewPrompt, ReviewPromptWithActions, ReviewState, Reviewed,
    SearchConfig, SearchPrompt, SelectConfig, SelectPrompt, TextConfig, TextPrompt,
};
/// Presentation settings for a review prompt, passed to `with_config`.
///
/// ```no_run
/// use climax::{Configurable as _, ReviewConfig, ReviewExit, ReviewState};
///
/// # fn handle(context: climax::Context) -> climax::Result<()> {
/// let outcome = context
///     .review::<&str>("changes")
///     .item("a.rs", "a.rs", ReviewState::Unconfirmed)
///     .action('a', "accept", ())
///     .with_config(ReviewConfig::default())
///     .interact()?;
/// if let Some(items) = outcome.accepted_items() {
///     let _: &[climax::Reviewed<&str>] = items;
/// }
/// match outcome.exit() {
///     ReviewExit::Submit | ReviewExit::Leave => {},
///     ReviewExit::Action(()) => {},
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "interactive")]
pub use bang::ReviewConfig;
pub use error::{
    Error,
    ErrorKind,
    Result,
    ResultExt,
};
#[cfg(feature = "interactive")] pub use bang;
#[cfg(feature = "derive")]
pub use climax_derive::{
    Parse,
    ValueEnum,
};
#[cfg(all(feature = "derive", feature = "structured"))]
pub use climax_derive::serde;
#[cfg(feature = "parse")] pub use pound;
#[cfg(feature = "render")] pub use screw;
#[cfg(feature = "structured")] pub use ::serde;
