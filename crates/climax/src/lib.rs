//! batteries-included CLI facade over `pound`, `screw`, and `bang`

pub mod app;
pub mod error;
pub mod output;
pub mod prelude;
pub mod terminal;

#[cfg(feature = "interactive")] pub mod prompt;

#[cfg(feature = "render")] pub mod status;

#[cfg(feature = "interactive")] pub use bang_core as bang;
pub use app::{
    Context,
    run_with,
};
#[cfg(feature = "interactive")]
pub use app::{
    OutputContext,
    PromptContext,
};
#[cfg(feature = "parse")]
pub use app::run;
pub use error::{
    Error,
    Result,
};
#[cfg(feature = "parse")]
pub use pound;
#[cfg(feature = "render")] pub use screw;
