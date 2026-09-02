use std::{error, fmt};

/// Broad failure category for a prompt interaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    Cancelled,
    InputEnded,
    InteractionBusy,
    InteractionUnavailable,
    InvalidConfiguration,
    Terminal,
    UnexpectedValue,
}

/// Error returned by a Bang prompt.
///
/// Implementation-specific errors remain available through
/// [`error::Error::source`] without becoming part of Bang's public data model.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    source: Option<Box<dyn error::Error + Send + Sync>>,
}

impl Error {
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub(crate) fn unexpected(expected: &'static str) -> Self {
        Self {
            kind: ErrorKind::UnexpectedValue,
            message: format!("prompt returned an unexpected value; expected {expected}"),
            source: None,
        }
    }

    pub(crate) fn invalid_configuration(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidConfiguration,
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn from_live(error: bang_screw::LiveSessionError) -> Self {
        let kind = match error.primary().unwrap_or(&error) {
            bang_screw::LiveSessionError::Unavailable => ErrorKind::InteractionUnavailable,
            bang_screw::LiveSessionError::Cancelled => ErrorKind::Cancelled,
            bang_screw::LiveSessionError::InputEnded => ErrorKind::InputEnded,
            _ => ErrorKind::Terminal,
        };
        Self {
            kind,
            message: error.to_string(),
            source: Some(Box::new(error)),
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self {
            kind: ErrorKind::Cancelled,
            message: "prompt was cancelled".to_owned(),
            source: None,
        }
    }

    pub(crate) fn input_ended() -> Self {
        Self {
            kind: ErrorKind::InputEnded,
            message: "input ended before the prompt was submitted".to_owned(),
            source: None,
        }
    }

    pub fn interaction_busy() -> Self {
        Self {
            kind: ErrorKind::InteractionBusy,
            message: "another interaction already owns the terminal".to_owned(),
            source: None,
        }
    }

    pub fn interaction_unavailable() -> Self {
        Self {
            kind: ErrorKind::InteractionUnavailable,
            message: "interactive terminal input is unavailable".to_owned(),
            source: None,
        }
    }

    /// Preserve a terminal lifecycle or rendering failure at the interaction
    /// boundary.
    pub fn terminal(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind: ErrorKind::Terminal,
            message: source.to_string(),
            source: Some(Box::new(source)),
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

pub type Result<T> = std::result::Result<T, Error>;
