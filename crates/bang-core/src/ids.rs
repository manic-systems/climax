// SPDX-License-Identifier: EUPL-1.2

use std::borrow::Cow;

/// Names a widget so a driver or an action can refer to it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WidgetId(Cow<'static, str>);

impl WidgetId {
    /// An id backed by a static string, with no allocation.
    #[must_use]
    pub const fn borrowed(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// An id that owns its text.
    #[must_use]
    pub fn owned(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for WidgetId {
    fn from(value: &'static str) -> Self {
        Self::borrowed(value)
    }
}

impl From<String> for WidgetId {
    fn from(value: String) -> Self {
        Self::owned(value)
    }
}
