// SPDX-License-Identifier: EUPL-1.2

//! batteries-included CLI facade over `pound`, `screw`, and `bang`

extern crate alloc;

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
    Result,
};
#[cfg(feature = "parse")] pub use pound;
#[cfg(feature = "parse")]
pub use pound::{
    FromArg,
    Parse as ParseTrait,
};
#[cfg(feature = "render")] pub use screw;
#[cfg(feature = "render")]
pub use screw::{
    Color,
    Role,
    Style,
    Theme,
};
#[cfg(feature = "structured")] pub use serde;
