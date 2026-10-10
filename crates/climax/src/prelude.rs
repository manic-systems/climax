// SPDX-License-Identifier: EUPL-1.2

//! Imports for the ordinary `climax` application path.

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
    Date,
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
    ErrorKind,
    Result,
    main_with,
    output::Format,
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

#[cfg(all(test, feature = "interactive"))]
mod tests {
    use super::*;

    #[test]
    fn the_prelude_names_dates_and_error_kinds() {
        let date = Date::new(2026, 10, 10).expect("a valid date");
        assert_eq!(date, crate::Date::new(2026, 10, 10).unwrap());
        assert_eq!(Error::message("x").kind(), ErrorKind::Message);
        let _: crate::ErrorKind = ErrorKind::Parse;
    }
}
