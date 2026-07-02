// SPDX-License-Identifier: EUPL-1.2

//! the parse error type and early-exit signals

use core::fmt;

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;

/// anything a parse attempt can produce
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// unrecognised `--flag` or `-x`
    Unknown(String),
    /// an option that takes a value got none
    MissingValue(String),
    /// a required arg or positional was absent
    MissingRequired(String),
    /// a bare value with nowhere to go
    UnexpectedPositional(String),
    /// a flag was given a value (e.g. `--verbose=3`) but takes none
    UnexpectedValue(String),
    /// first positional named a subcommand that does not exist
    UnknownSubcommand(String),
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
    /// a required group had none of its members set
    MissingGroup { group: String, options: String },
    /// `-h` / `--help`, payload is rendered help
    Help(String),
    /// `--version`, payload is the version line
    Version(String),
}

impl Error {
    /// for non-failure signals
    #[must_use]
    pub const fn is_exit(&self) -> bool {
        matches!(*self, Self::Help(_) | Self::Version(_))
    }

    /// print and exit
    #[cfg(feature = "std")]
    pub fn exit(self) -> ! {
        match self {
            Self::Help(text) | Self::Version(text) => {
                println!("{text}");
                std::process::exit(0);
            },
            other => {
                eprintln!("error: {other}");
                std::process::exit(2);
            },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(a) => write!(f, "unrecognised argument '{a}'"),
            Self::MissingValue(a) => write!(f, "'{a}' needs a value"),
            Self::MissingRequired(a) => write!(f, "missing required argument {a}"),
            Self::UnexpectedPositional(v) => write!(f, "unexpected argument '{v}'"),
            Self::UnexpectedValue(a) => write!(f, "'{a}' does not take a value"),
            Self::UnknownSubcommand(s) => write!(f, "unknown subcommand '{s}'"),
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
            Self::MissingGroup { group, options } => {
                write!(f, "one of {options} is required ({group})")
            },
            Self::Help(text) | Self::Version(text) => write!(f, "{text}"),
        }
    }
}

impl core::error::Error for Error {}
