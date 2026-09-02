use std::{error, fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

/// A stable, high-level category for an application error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Command-line arguments could not be parsed.
    Parse,
    /// Terminal output could not be rendered or written.
    Output,
    /// Filesystem or other general I/O failed.
    Io,
    /// An interactive operation failed.
    Interactive,
    /// The user cancelled an operation.
    Cancelled,
    /// Input ended before an operation completed.
    InputEnded,
    /// Interactive input is unavailable under the current terminal policy.
    InteractionUnavailable,
    /// Another prompt already owns the interactive terminal.
    InteractionBusy,
    /// An application-owned source error.
    Application,
    /// An application-defined error.
    Message,
}

/// An error reported through the `climax` application facade.
///
/// Dependency-specific errors are retained as opaque sources instead of being
/// exposed as variants in the facade contract.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    source: Option<Box<dyn error::Error + Send + Sync + 'static>>,
    related: Vec<Self>,
}

impl Error {
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Message,
            message: message.into(),
            source: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn application(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self::with_source(ErrorKind::Application, source)
    }

    #[must_use]
    pub fn application_context(
        message: impl Into<String>,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        let message = format!("{}: {source}", message.into());
        Self {
            kind: ErrorKind::Application,
            message,
            source: Some(Box::new(source)),
            related: Vec::new(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    pub fn source_error(&self) -> Option<&(dyn error::Error + Send + Sync + 'static)> {
        self.source.as_deref()
    }

    /// Additional failures which occurred while cleaning up the primary
    /// operation. The original error remains the primary kind and source.
    #[must_use]
    pub fn related_errors(&self) -> &[Self] {
        &self.related
    }

    #[cfg(feature = "render")]
    pub(crate) fn with_cleanup(mut self, cleanup: Self) -> Self {
        self.message = format!("{}; cleanup also failed: {cleanup}", self.message);
        self.related.push(cleanup);
        self
    }

    pub(crate) fn with_source(
        kind: ErrorKind,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: source.to_string(),
            source: Some(Box::new(source)),
            related: Vec::new(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}

#[cfg(feature = "parse")]
impl From<pound::Error> for Error {
    fn from(value: pound::Error) -> Self {
        Self::with_source(ErrorKind::Parse, value)
    }
}

#[cfg(feature = "interactive")]
impl From<bang::Error> for Error {
    fn from(value: bang::Error) -> Self {
        let kind = match value.kind() {
            bang::ErrorKind::Cancelled => ErrorKind::Cancelled,
            bang::ErrorKind::InputEnded => ErrorKind::InputEnded,
            bang::ErrorKind::InteractionUnavailable => ErrorKind::InteractionUnavailable,
            bang::ErrorKind::InteractionBusy => ErrorKind::InteractionBusy,
            _ => ErrorKind::Interactive,
        };
        Self::with_source(kind, value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::with_source(ErrorKind::Io, value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::message(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::message(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_errors_have_no_dependency_source() {
        let error = Error::message("boom");
        assert_eq!(error.kind(), ErrorKind::Message);
        assert_eq!(error.to_string(), "boom");
        assert!(error.source_error().is_none());
    }

    #[test]
    fn dependency_errors_are_opaque_sources() {
        let error = Error::from(io::Error::other("closed"));
        assert_eq!(error.kind(), ErrorKind::Io);
        assert_eq!(error.to_string(), "closed");
        assert!(error.source_error().is_some());
    }

    #[test]
    fn application_context_preserves_its_source() {
        let error = Error::application_context("cannot query history", io::Error::other("closed"));
        assert_eq!(error.kind(), ErrorKind::Application);
        assert_eq!(error.to_string(), "cannot query history: closed");
        assert!(error.source_error().is_some());
    }
}
