// SPDX-License-Identifier: EUPL-1.2

//! the parser's view of a half-typed command line, for shell completion
//!
//! [`Partial::parse`] runs the real parser over the words before the cursor in
//! a lenient mode. a word it cannot apply is skipped rather than failing the
//! parse, and no requirement is checked, since the line is not finished. the
//! result says which command the words reached, what the next word would be,
//! and what every arg was given so far.
//!
//! an arg's [`Completer`], set with `#[pound(complete = ...)]`, receives the
//! [`Partial`] so it can read the rest of the line through [`Partial::arg`],
//! e.g. a secret name completer reading which `--file` was picked.

use core::fmt;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::{
    Matches,
    parse::probe_spec,
    spec::{
        ArgSpec,
        Command,
        CommandSpec,
    },
};

/// lists the values an arg could take, given the line so far and the text
/// already typed for this value
pub type Completer = fn(&Partial<'_, '_>, &str) -> Vec<Candidate>;

/// one value a [`Completer`] offers
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Candidate {
    /// the text inserted on the line
    pub value: String,
    /// what shells that show descriptions print beside it, empty for none
    pub help:  String,
}

impl Candidate {
    /// a candidate with no description
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            help:  String::new(),
        }
    }

    /// describe the candidate
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }
}

impl From<&str> for Candidate {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Candidate {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// what the word after the parsed ones would be
#[derive(Clone, Copy, Debug)]
pub enum Next {
    /// the value of an option whose name ended the words
    Value(&'static ArgSpec),
    /// a fresh word
    #[non_exhaustive]
    Word {
        /// the positional a bare word would fill, if one is open
        positional:  Option<&'static ArgSpec>,
        /// whether a bare word would be looked up as a subcommand name
        subcommands: bool,
        /// whether a word starting with `-` would still be read as an option,
        /// which stops after `--` or once a trailing arg starts
        options:     bool,
    },
}

/// a command line parsed up to the cursor
pub struct Partial<'s, 'a> {
    root:    Command<'s>,
    matches: Matches<'a>,
    next:    Next,
}

impl<'s, 'a> Partial<'s, 'a> {
    /// parse `words`, the arguments before the cursor without the program
    /// name, against `root`
    pub fn parse(root: &'s CommandSpec, words: impl IntoIterator<Item = &'a str>) -> Self {
        let (matches, next) = probe_spec(root, words);
        Self {
            root: Command::root(root),
            matches,
            next,
        }
    }

    /// the deepest command the words selected
    #[must_use]
    pub fn command(&self) -> &Command<'s> {
        self.matches.selected().unwrap_or(&self.root)
    }

    /// what the word after the parsed ones would be
    #[must_use]
    pub const fn next(&self) -> Next {
        self.next
    }

    /// what the line gave the arg `name` reaches, or `None` when no command
    /// on the line has such an arg. `name` is a spelling, `--file` or `-f`, or
    /// a positional's value name, and the deepest command answers first.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<Given<'_, 'a>> {
        let reaches = |a: &ArgSpec| {
            if let Some(long) = name.strip_prefix("--") {
                a.answers_long(long)
            } else if let Some(short) = name.strip_prefix('-') {
                let mut chars = short.chars();
                matches!((chars.next(), chars.next()), (Some(c), None) if a.answers_short(c))
            } else {
                a.is_positional() && a.value_name == name
            }
        };

        let (arg, m, index) = self.matches.find_deepest(self.root.spec(), &reaches)?;
        Some(Given {
            arg,
            values: m.raws(index),
            count: m.count(index),
            negated: m.negated(index),
        })
    }
}

/// what the line gave one arg so far
#[derive(Clone, Copy, Debug)]
pub struct Given<'p, 'a> {
    arg:     &'static ArgSpec,
    values:  &'p [&'a str],
    count:   u32,
    negated: bool,
}

impl<'a> Given<'_, 'a> {
    #[must_use]
    pub const fn arg(&self) -> &'static ArgSpec {
        self.arg
    }

    /// the values typed for it, in order
    #[must_use]
    pub const fn values(&self) -> &[&'a str] {
        self.values
    }

    /// the value typed for it last
    #[must_use]
    pub const fn last(&self) -> Option<&'a str> {
        match self.values {
            [.., last] => Some(*last),
            [] => None,
        }
    }

    /// whether it was given and, for a flag, not negated since
    #[must_use]
    pub const fn present(&self) -> bool {
        self.count > 0
    }

    /// whether a flag's negation was typed after its last enabling spelling
    #[must_use]
    pub const fn negated(&self) -> bool {
        self.negated
    }
}

impl fmt::Debug for Partial<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Partial")
            .field("command", &self.command().spec().name)
            .field("path", &self.command().path())
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        Kind,
        SubSpec,
    };

    const OPEN_ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Opt).long("file").short('f'),
        ArgSpec::new(Kind::Positional)
            .value_name("secret")
            .required(),
    ];
    const OPEN: CommandSpec = CommandSpec::new("open").args(OPEN_ARGS);
    const ROOT_ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("store").global()];
    const ROOT: CommandSpec = CommandSpec::new("vault")
        .args(ROOT_ARGS)
        .subs(&[SubSpec::new("open", &OPEN)]);

    #[test]
    fn an_option_waiting_on_its_value_ends_the_words() {
        let partial = Partial::parse(&ROOT, ["open", "--nope", "--store"]);
        assert_eq!(partial.command().spec().name, "open");
        assert!(matches!(partial.next(), Next::Value(a) if a.long == Some("store")));
    }

    #[test]
    fn reads_every_level_of_the_line() {
        let partial = Partial::parse(&ROOT, ["--store", "s", "open", "-f", "x.nix"]);
        let Next::Word {
            positional: Some(arg),
            subcommands: false,
            options: true,
        } = partial.next()
        else {
            panic!("expected the secret positional, got {:?}", partial.next());
        };
        assert_eq!(arg.value_name, "secret");
        assert_eq!(partial.arg("--file").and_then(|f| f.last()), Some("x.nix"));
        assert_eq!(partial.arg("--store").and_then(|s| s.last()), Some("s"));
        assert!(partial.arg("--nope").is_none());
    }
}
