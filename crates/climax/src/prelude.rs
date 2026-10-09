// SPDX-License-Identifier: EUPL-1.2

//! prelude: imports for the ordinary `climax` application path

#[cfg(feature = "parse")]
pub use crate::pound::{
    FromArg,
    Parse as _,
    ValueError,
};
#[cfg(feature = "interactive")]
pub use crate::{
    Configurable,
    ConfirmConfig,
    DateConfig,
    MultiSelectConfig,
    NumberConfig,
    PasswordConfig,
    PromptOutcome,
    ReviewConfig,
    ReviewExit,
    ReviewOutcome,
    ReviewState,
    Reviewed,
    SearchConfig,
    SelectConfig,
    TextConfig,
};
pub use crate::{
    Context,
    Error,
    Result,
    run_with,
    terminal::{
        InteractionMode,
        StatusMode,
        TerminalCapabilities,
        TerminalPolicy,
    },
};
#[cfg(feature = "derive")]
pub use crate::{
    Parse,
    ValueEnum,
};
#[cfg(feature = "parse")]
pub use crate::{
    main,
    try_run,
    try_run_from,
};
