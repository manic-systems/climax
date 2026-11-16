// SPDX-License-Identifier: EUPL-1.2

//! climax: application facade over `pound`, `screw` and `bang`
//!
//! the crate root and [`prelude`] hold the ordinary application workflow. each
//! component is also re-exported whole behind the feature that enables it, as
//! `climax::pound` under `parse`, `climax::bang` under `interactive`,
//! `climax::screw` under `render`, `climax::caress` under `complete` and
//! `climax::serde` under `structured`. an application needs no direct
//! dependency on any of them, and derives with `climax::Parse` and
//! `climax::ValueEnum`, since the derives under `climax::pound` expand to
//! `::pound` paths

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
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

extern crate alloc;

mod app;
/// the application error type and its categories
pub mod error;
/// what a package installs beside the binary, completion scripts and man pages
#[cfg(feature = "complete")]
pub mod install;
#[cfg(feature = "structured")] mod json;
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
/// runs an application that takes no arguments and reports its process outcome
///
/// errors are reported and exit codes chosen as in `main`. with `parse`,
/// `--help` and `--version` answer with the calling package's version and any
/// other argument fails with exit 2, and without it arguments are ignored
///
/// ```no_run
/// use climax::prelude::*;
///
/// fn main() -> std::process::ExitCode {
///     climax::main_with!(|cx: Context| -> climax::Result<()> {
///         cx.output().write_message("hello")
///     })
/// }
/// ```
#[macro_export]
macro_rules! main_with {
    ($application:expr $(,)?) => {{
        struct __ClimaxPackage;
        impl $crate::__private::Package for __ClimaxPackage {
            const HASH: ::core::option::Option<&'static str> =
                ::core::option_env!("POUND_GIT_HASH");
            const NAME: &'static str = ::core::env!("CARGO_PKG_NAME");
            const VERSION: &'static str = ::core::env!("CARGO_PKG_VERSION");
        }
        $crate::__private::main_with::<__ClimaxPackage, _>($application)
    }};
}
#[doc(hidden)]
pub mod __private {
    pub use crate::app::{
        Package,
        main_with,
    };
}
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
#[cfg(feature = "interactive")]
pub use bang::{
    ConfirmPrompt,
    Date,
    DatePrompt,
    MultiSelectPrompt,
    NumberPrompt,
    PasswordPrompt,
    PromptOutcome,
    ReviewExit,
    ReviewOutcome,
    ReviewPrompt,
    ReviewPromptWithActions,
    ReviewState,
    Reviewed,
    SearchPrompt,
    SelectPrompt,
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
