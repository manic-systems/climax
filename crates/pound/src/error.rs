// SPDX-License-Identifier: EUPL-1.2

//! the parse error type and early-exit signals

use core::fmt;

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;

/// what a parse attempt ran into, or which early exit it was asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// unrecognized `--flag` or `-x`
    Unknown {
        arg: String,
        closest: Option<String>,
    },
    /// an option that takes a value got none
    MissingValue(String),
    /// a required arg or positional was absent
    MissingRequired(String),
    /// a bare value with nowhere to go
    UnexpectedPositional(String),
    /// a flag was given a value (e.g. `--verbose=3`) but takes none
    UnexpectedValue(String),
    /// first positional named a subcommand that does not exist
    UnknownSubcommand {
        name: String,
        closest: Option<String>,
    },
    /// a subcommand was required but none given
    MissingSubcommand,
    /// a value failed to parse into its target type
    Value {
        arg: String,
        value: String,
        msg: String,
    },
    /// two members of a mutually-exclusive group were both set
    Conflict {
        group: String,
        first: String,
        second: String,
    },
    /// an arg was set without the other arg it obliges
    Requires { arg: String, needs: String },
    /// a required group had none of its members set
    MissingGroup { group: String, options: String },
    /// `-h` / `--help`, payload is rendered help
    Help(String),
    /// `--version`, payload is the version line
    Version(String),
}

impl ErrorKind {
    /// the known spelling a mistyped one was probably meant to be
    #[must_use]
    pub fn closest(&self) -> Option<&str> {
        match self {
            Self::Unknown { closest, .. } | Self::UnknownSubcommand { closest, .. } => {
                closest.as_deref()
            },
            _ => None,
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { arg, .. } => write!(f, "unrecognized argument '{arg}'"),
            Self::MissingValue(a) => write!(f, "'{a}' needs a value"),
            Self::MissingRequired(a) => write!(f, "missing required argument {a}"),
            Self::UnexpectedPositional(v) => write!(f, "unexpected argument '{v}'"),
            Self::UnexpectedValue(a) => write!(f, "'{a}' does not take a value"),
            Self::UnknownSubcommand { name, .. } => write!(f, "unknown subcommand '{name}'"),
            Self::MissingSubcommand => write!(f, "a subcommand is required"),
            Self::Value { arg, value, msg } => {
                write!(f, "invalid value '{value}' for {arg}: {msg}")
            },
            Self::Conflict {
                group,
                first,
                second,
            } => {
                if group.is_empty() {
                    write!(f, "{first} and {second} cannot be used together")
                } else {
                    write!(f, "{first} and {second} cannot be used together ({group})")
                }
            },
            Self::Requires { arg, needs } => write!(f, "{arg} requires {needs}"),
            Self::MissingGroup { group, options } => {
                write!(f, "one of {options} is required ({group})")
            },
            Self::Help(text) | Self::Version(text) => write!(f, "{text}"),
        }
    }
}

/// a parse outcome that is not a value, carrying the usage line of whichever
/// command in the tree raised it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub usage: Option<String>,
}

impl Error {
    /// for non-failure signals
    #[must_use]
    pub const fn is_exit(&self) -> bool {
        matches!(self.kind, ErrorKind::Help(_) | ErrorKind::Version(_))
    }

    /// remember the usage line of the command being parsed, unless a nested
    /// command already claimed the failure as its own
    pub(crate) fn or_usage(mut self, usage: impl FnOnce() -> String) -> Self {
        if self.usage.is_none() && !self.is_exit() {
            self.usage = Some(usage());
        }
        self
    }

    /// the whole report: the message, a spelling suggestion, the usage line,
    /// and where to look next
    #[must_use]
    pub fn render(&self) -> String {
        if let ErrorKind::Help(text) | ErrorKind::Version(text) = &self.kind {
            return text.clone();
        }

        let mut out = format!("error: {}", self.kind);
        if let Some(closest) = self.kind.closest() {
            out.push_str("\n\n  tip: did you mean '");
            out.push_str(closest);
            out.push('\'');
        }
        if let Some(usage) = &self.usage {
            out.push_str("\n\n");
            out.push_str(usage);
        }
        out.push_str("\n\nFor more information, try '--help'.");
        out
    }

    /// print and exit
    #[cfg(feature = "std")]
    pub fn exit(self) -> ! {
        if self.is_exit() {
            println!("{}", self.render());
            std::process::exit(0);
        }
        eprintln!("{}", self.render());
        std::process::exit(2);
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self { kind, usage: None }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl core::error::Error for Error {}
