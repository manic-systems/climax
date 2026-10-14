use core::{
    error,
    fmt,
    result::Result as CoreResult,
};
#[cfg(any(feature = "render", feature = "interactive"))]
use std::io;

pub type Result<T> = CoreResult<T, Error>;

pub enum Error {
    /// argument parsing failed
    #[cfg(feature = "parse")]
    ArgParse(pound::Error),
    /// terminal drawing or stream output failed
    #[cfg(any(feature = "render", feature = "interactive"))]
    Draw(io::Error),
    /// interactive terminal session failed
    #[cfg(feature = "interactive")]
    Interact(bang_screw::LiveSessionError),
    /// prompt was cancelled
    #[cfg(feature = "interactive")]
    Cancelled,
    /// input ended before value submission
    #[cfg(feature = "interactive")]
    InputEnded,
    /// invalid value
    #[cfg(feature = "interactive")]
    UnexpectedValue {
        expected: &'static str,
        actual:   &'static str,
    },
    Message(String),
}

impl Error {
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "parse")]
            Self::ArgParse(error) => write!(f, "{error}"),
            #[cfg(any(feature = "render", feature = "interactive"))]
            Self::Draw(error) => write!(f, "{error}"),
            #[cfg(feature = "interactive")]
            Self::Interact(error) => write!(f, "{error}"),
            #[cfg(feature = "interactive")]
            Self::Cancelled => f.write_str("cancelled"),
            #[cfg(feature = "interactive")]
            Self::InputEnded => f.write_str("input ended before submit"),
            #[cfg(feature = "interactive")]
            Self::UnexpectedValue { expected, actual } => {
                write!(f, "expected {expected}, got {actual}")
            },
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl fmt::Debug for Error {
    // std prints an error returned from main through Debug
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "parse")]
            Self::ArgParse(error) => {
                let report = error.render(pound::Term::plain());
                f.write_str(report.strip_prefix("error: ").unwrap_or(&report))
            },
            _ => fmt::Display::fmt(self, f),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            #[cfg(feature = "parse")]
            Self::ArgParse(error) => Some(error),
            #[cfg(any(feature = "render", feature = "interactive"))]
            Self::Draw(error) => Some(error),
            #[cfg(feature = "interactive")]
            Self::Interact(error) => Some(error),
            #[cfg(feature = "interactive")]
            Self::Cancelled | Self::InputEnded | Self::UnexpectedValue { .. } => None,
            Self::Message(_) => None,
        }
    }
}

#[cfg(feature = "parse")]
impl From<pound::Error> for Error {
    fn from(value: pound::Error) -> Self {
        Self::ArgParse(value)
    }
}

#[cfg(feature = "interactive")]
impl From<bang_screw::LiveSessionError> for Error {
    fn from(value: bang_screw::LiveSessionError) -> Self {
        match value {
            bang_screw::LiveSessionError::Cancelled => Self::Cancelled,
            bang_screw::LiveSessionError::InputEnded => Self::InputEnded,
            other @ (bang_screw::LiveSessionError::RawMode(_)
            | bang_screw::LiveSessionError::Signals(_)
            | bang_screw::LiveSessionError::TerminalIo(_)
            | bang_screw::LiveSessionError::Signalled(_)
            | bang_screw::LiveSessionError::ReraiseSignal { .. }) => Self::Interact(other),
        }
    }
}

#[cfg(any(feature = "render", feature = "interactive"))]
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Draw(value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
