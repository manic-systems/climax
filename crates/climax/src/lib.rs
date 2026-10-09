//! batteries-included CLI facade over `pound`, `screw`, and `bang`

extern crate alloc;

pub mod app;
pub mod error;
pub mod output;
pub mod prelude;

#[cfg(feature = "interactive")] pub mod prompt;

#[cfg(feature = "render")] pub mod status;

#[cfg(feature = "parse")] pub use app::run;
pub use app::{
    Context,
    run_with,
};
#[cfg(feature = "interactive")]
pub use app::{
    OutputContext,
    PromptContext,
};
#[cfg(feature = "interactive")] pub use bang_core as bang;
pub use error::{
    Error,
    Result,
};
#[cfg(feature = "parse")] pub use pound;
#[cfg(feature = "render")] pub use screw;
