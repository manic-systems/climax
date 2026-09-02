//! Batteries-included application facade over `pound`, `screw`, and `bang`.
//!
//! The crate root and [`prelude`] contain the ordinary application workflow.
//! Add `pound`, `screw`, or `bang` as direct dependencies when using their
//! standalone or advanced APIs; `climax` deliberately does not re-export them.
//!
//! Deriving `pound::Parse` also requires a direct `pound` dependency with its
//! `derive` feature. A transitive dependency through `climax` is not sufficient
//! because the generated code refers to `pound` by name.

mod app;
pub mod error;
pub mod output;
pub mod prelude;
pub mod terminal;

#[cfg(feature = "render")]
pub mod status;

#[allow(deprecated)]
#[cfg(feature = "parse")]
pub use app::run;
pub use app::{Context, run_with};
#[cfg(feature = "parse")]
pub use app::{main, try_run, try_run_from};
pub use error::{Error, Result};
