use std::{
    error,
    fmt,
    io,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// argument parsing failed
    Parse(pound::Error),
    /// terminal or stream I/O failed
    Io(io::Error),
    /// prompt was cancelled
    Cancelled,
    /// input ended before value submission
    InputEnded,
    /// interrupted by signal
    Signalled(i32),
    /// invalid value
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
            Self::Parse(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::InputEnded => f.write_str("input ended before submit"),
            Self::Signalled(signal) => write!(f, "interrupted by signal {signal}"),
            Self::UnexpectedValue { expected, actual } => {
                write!(f, "expected {expected}, got {actual}")
            },
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Cancelled
            | Self::InputEnded
            | Self::Signalled(_)
            | Self::UnexpectedValue { .. }
            | Self::Message(_) => None,
        }
    }
}

impl From<pound::Error> for Error {
    fn from(value: pound::Error) -> Self {
        Self::Parse(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
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
