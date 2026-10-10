// SPDX-License-Identifier: EUPL-1.2

//! the parse error type and early-exit signals

use core::fmt;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;

/// what a parse attempt ran into, or which early exit it was asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// unrecognized `--flag` or `-x`
    Unknown {
        /// the spelling that was not recognized
        arg:     String,
        /// the known spelling it was probably meant to be, when one is close
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
        /// the word that named no subcommand
        name:    String,
        /// the known subcommand it was probably meant to be, when one is close
        closest: Option<String>,
    },
    /// a subcommand was required but none given. the parser attaches the
    /// command's help text, so it reaches stderr with status 2 like any other
    /// failure
    MissingSubcommand,
    /// a value failed to parse into its target type
    Value {
        /// the argument the value was given to, as usage spells it
        arg:   String,
        /// the text that failed to parse
        value: String,
        /// why it failed, with the possible values appended for a choice type
        msg:   String,
    },
    /// two members of a mutually-exclusive group were both set
    Conflict {
        /// the group both were members of, empty for a `conflicts_with` pair
        group:  String,
        /// the first of the two arguments
        first:  String,
        /// the second of the two arguments
        second: String,
    },
    /// an arg was set without the other arg it obliges
    Requires {
        /// the argument that was set
        arg:   String,
        /// the argument it obliges, which was absent
        needs: String,
    },
    /// a list arg got fewer values than it accepts
    TooFewValues {
        /// the list argument
        arg: String,
        /// the fewest values it accepts
        min: usize,
        /// how many it got
        got: usize,
    },
    /// a list arg got more values than it accepts
    TooManyValues {
        /// the list argument
        arg: String,
        /// the most values it accepts
        max: usize,
        /// how many it got
        got: usize,
    },
    /// a required group had none of its members set
    MissingGroup {
        /// the group name
        group:   String,
        /// the members to choose from, comma separated
        options: String,
    },
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
            Self::TooFewValues { arg, min, got } => {
                write!(f, "{arg} takes at least {min} values, got {got}")
            },
            Self::TooManyValues { arg, max, got } => {
                write!(f, "{arg} takes at most {max} values, got {got}")
            },
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
    /// what happened
    pub kind:      ErrorKind,
    /// the usage line of the command that raised it, when it came from a parse.
    /// for [`ErrorKind::MissingSubcommand`] raised by the parser it is the
    /// whole help text, so the report lists the subcommands to pick from
    pub usage:     Option<String>,
    /// the spelling that still reaches the generated help, if any does
    pub help_flag: Option<&'static str>,
}

impl Error {
    /// for non-failure signals
    #[must_use]
    pub const fn is_exit(&self) -> bool {
        matches!(self.kind, ErrorKind::Help(_) | ErrorKind::Version(_))
    }

    /// remember the usage line and help spelling of the command being parsed,
    /// unless a nested command already claimed the failure as its own
    pub(crate) fn or_usage(
        mut self,
        usage: impl FnOnce() -> (String, Option<&'static str>),
    ) -> Self {
        if self.usage.is_none() && !self.is_exit() {
            let (usage, help_flag) = usage();
            self.usage = Some(usage);
            self.help_flag = help_flag;
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
        if let Some(flag) = self.help_flag {
            out.push_str("\n\nFor more information, try '");
            out.push_str(flag);
            out.push_str("'.");
        }
        out
    }

    /// print and exit
    #[cfg(feature = "std")]
    pub fn exit(self) -> ! {
        use std::io::Write as _;

        let code = if self.is_exit() {
            match writeln!(std::io::stdout(), "{}", self.render()) {
                Ok(()) => 0,
                // 128 + SIGPIPE, what a shell reports for a killed writer
                Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => 141,
                Err(_) => 1,
            }
        } else {
            let _ = writeln!(std::io::stderr(), "{}", self.render());
            2
        };
        std::process::exit(code);
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self {
            kind,
            usage: None,
            help_flag: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl core::error::Error for Error {}
