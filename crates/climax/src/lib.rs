// SPDX-License-Identifier: EUPL-1.2

//! climax: application facade over `pound`, `screw` and `bang`.
//!
//! the crate root and [`prelude`] hold the ordinary application workflow. each
//! component is also re-exported whole behind the feature that enables it, as
//! `climax::pound` under `parse`, `climax::bang` under `interactive`,
//! `climax::screw` under `render`, `climax::caress` under `complete` and
//! `climax::serde` under `structured`. an application needs no direct
//! dependency on any of them, and the versions it sees are the ones `climax`
//! was tested against

#![cfg_attr(
    all(
        feature = "derive",
        feature = "interactive",
        feature = "render",
        feature = "structured"
    ),
    doc = include_str!("../README.md")
)]
#![cfg_attr(doc, doc = include_str!("../docs/facade.md"))]
#![warn(missing_docs)]

extern crate alloc;

mod app;
/// the application error type and its categories
pub mod error;
/// what a package installs beside the binary, completion scripts and man pages
#[cfg(feature = "complete")]
pub mod install;
pub mod output;
pub mod prelude;
mod sync;
/// terminal capability facts, policy overrides and terminal applications
pub mod terminal;
pub mod testing;

/// transient status lines drawn on the shared renderer
#[cfg(feature = "render")]
pub mod status;

/// the `serde` crate, for applications that depend on `climax` alone
///
/// `#[climax::serde(..)]` also needs `derive`, otherwise this path is only the
/// crate
#[cfg(feature = "structured")]
pub use ::serde;
pub use app::{
    Context,
    main_with,
    run_with,
};
#[cfg(feature = "parse")]
pub use app::{
    main,
    try_run,
    try_run_from,
};
#[cfg(feature = "interactive")] pub use bang;
/// presentation settings for a review prompt, passed to `with_config`
#[cfg(feature = "interactive")]
pub use bang::ReviewConfig;
#[cfg(feature = "interactive")]
pub use bang::{
    Configurable,
    ConfirmConfig,
    ConfirmPrompt,
    Date,
    DateConfig,
    DatePrompt,
    MultiSelectConfig,
    MultiSelectPrompt,
    NumberConfig,
    NumberPrompt,
    PasswordConfig,
    PasswordPrompt,
    PromptOutcome,
    ReviewExit,
    ReviewOutcome,
    ReviewPrompt,
    ReviewPromptWithActions,
    ReviewState,
    Reviewed,
    SearchConfig,
    SearchPrompt,
    SelectConfig,
    SelectPrompt,
    TextConfig,
    TextPrompt,
};
#[cfg(feature = "complete")] pub use caress;
#[cfg(all(feature = "derive", feature = "structured"))]
pub use climax_derive::serde;
#[cfg(feature = "derive")]
pub use climax_derive::{
    Parse,
    ValueEnum,
};
pub use error::{
    Error,
    ErrorKind,
    Result,
    ResultExt,
};
#[cfg(feature = "parse")] pub use pound;
#[cfg(feature = "render")] pub use screw;
