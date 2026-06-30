// SPDX-License-Identifier: EUPL-1.2

use std::borrow::Cow;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WidgetId(Cow<'static, str>);

impl WidgetId {
    #[must_use]
    pub const fn borrowed(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    #[must_use]
    pub fn owned(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

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

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ViewId(Cow<'static, str>);

impl ViewId {
    #[must_use]
    pub const fn borrowed(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    #[must_use]
    pub fn owned(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for ViewId {
    fn from(value: &'static str) -> Self {
        Self::borrowed(value)
    }
}

impl From<String> for ViewId {
    fn from(value: String) -> Self {
        Self::owned(value)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CursorAnchor(Cow<'static, str>);

impl CursorAnchor {
    #[must_use]
    pub const fn borrowed(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    #[must_use]
    pub fn owned(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for CursorAnchor {
    fn from(value: &'static str) -> Self {
        Self::borrowed(value)
    }
}

impl From<String> for CursorAnchor {
    fn from(value: String) -> Self {
        Self::owned(value)
    }
}
