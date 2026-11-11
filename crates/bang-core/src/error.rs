// SPDX-License-Identifier: EUPL-1.2

use core::{
    error,
    fmt,
};

/// a builder was given a value the widget cannot work with, such as a key that
/// is already bound
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidConfiguration(String);

impl InvalidConfiguration {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// what was wrong with the value
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InvalidConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl error::Error for InvalidConfiguration {}
