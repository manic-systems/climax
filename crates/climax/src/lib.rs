// SPDX-License-Identifier: EUPL-1.2

//! climax: application facade over `pound`, `screw` and `bang`.
//!
//! The crate root and [`prelude`] contain the ordinary application workflow.
//! Each component is also re-exported whole behind the feature that enables
//! it, as `climax::pound` under `parse`, `climax::bang` under `interactive`,
//! `climax::screw` under `render`, and `climax::serde` under `structured`. An
//! application needs no direct dependency on any of them, and the versions it
//! sees are the ones `climax` was tested against.
//!
//! The `derive` feature provides `#[derive(climax::Parse)]` and
//! `#[derive(climax::ValueEnum)]`, whose expansion refers to
//! `::climax::pound`. Serde's derives need `#[serde(crate = "climax::serde")]`
//! for the same reason.

extern crate alloc;

#[cfg(all(
    doctest,
    feature = "derive",
    feature = "interactive",
    feature = "render",
    feature = "structured"
))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod app;
pub mod error;
#[cfg(feature = "complete")] pub mod install;
pub mod output;
pub mod prelude;
mod sync;
pub mod terminal;

#[cfg(feature = "render")] pub mod status;

pub use app::{
    Context,
    run_with,
};
#[cfg(feature = "parse")]
pub use app::{
    main,
    try_run,
    try_run_from,
};
#[cfg(feature = "interactive")] pub use bang;
/// ```no_run
/// use climax::{
///     Configurable as _,
///     ReviewConfig,
///     ReviewExit,
///     ReviewState,
/// };
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
/// Types needed to build and match review prompts and to call `with_config`,
/// at the crate root, next to the `Context` methods that build them.
#[cfg(feature = "interactive")]
pub use bang::{
    Configurable,
    ConfirmConfig,
    Date,
    DateConfig,
    MultiSelectConfig,
    NumberConfig,
    PasswordConfig,
    PromptOutcome,
    ReviewExit,
    ReviewOutcome,
    ReviewState,
    Reviewed,
    SearchConfig,
    SelectConfig,
    TextConfig,
};
#[cfg(feature = "derive")]
pub use climax_derive::{
    Parse,
    ValueEnum,
};
pub use error::{
    Error,
    ErrorKind,
    Result,
};
#[cfg(feature = "parse")] pub use pound;
#[cfg(feature = "render")] pub use screw;
#[cfg(feature = "structured")] pub use serde;
