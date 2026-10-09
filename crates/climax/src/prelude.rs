// SPDX-License-Identifier: EUPL-1.2

//! Imports for the ordinary `climax` application path.

#[cfg(feature = "parse")]
pub use pound::{
    FromArg,
    Parse as ParseTrait,
};
#[cfg(feature = "derive")]
pub use pound::{
    Parse,
    ValueEnum,
};
#[cfg(feature = "render")]
pub use screw::{
    Color,
    Role,
    Style,
    Theme,
};

#[cfg(feature = "interactive")] pub use crate::output;
#[cfg(feature = "render")] pub use crate::status;
#[cfg(feature = "interactive")]
pub use crate::{
    Configurable, ConfirmConfig, DateConfig, MultiSelectConfig, NumberConfig, PasswordConfig,
    PromptOutcome, ReviewConfig, ReviewExit, ReviewOutcome,
    ReviewState, Reviewed, SearchConfig, SelectConfig, TextConfig,
};
pub use crate::{
    Context,
    Error,
    Result,
    run_with,
    terminal::{InteractionMode, StatusMode, TerminalCapabilities, TerminalPolicy},
};
#[cfg(feature = "parse")]
pub use crate::{try_run, try_run_from};
