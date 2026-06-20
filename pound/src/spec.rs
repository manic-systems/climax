// SPDX-License-Identifier: EUPL-1.2

//! the static description of a command line.
//!
//! the anti-bloat trick: the derive emits a `&'static [ArgSpec]` table and one
//! non-generic engine interprets it. no per-command parser to monomorphise, so
//! a subcommand costs a table entry, not a code blob.
//!
//! the builders are `const fn`, so generated code reads as a flat const:
//!
//! ```
//! use pound::spec::{
//!     ArgSpec,
//!     Kind,
//! };
//!
//! const ARGS: &[ArgSpec] = &[
//!     ArgSpec::new(Kind::Flag)
//!         .long("force")
//!         .short('f')
//!         .help("overwrite"),
//!     ArgSpec::new(Kind::Positional).value_name("name").required(),
//! ];
//! ```

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;

/// what shape of argument a spec entry describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// boolean switch, presence means true (`bool` field)
    Flag,
    /// repeatable switch counted into an int (`-vvv`, `u8`/`u32` field)
    Count,
    /// named option taking a value (`--opt val`)
    Opt,
    /// bare value matched by position
    Positional,
    /// everything after `--`, or the trailing variadic
    Trailing,
}

/// one argument's full description.
// independent attribute flags, not a state machine, so the bool-count lint is off
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct ArgSpec {
    pub long:       Option<&'static str>,
    /// extra long names that also match this arg, kept out of help
    pub aliases:    &'static [&'static str],
    pub short:      Option<char>,
    pub kind:       Kind,
    pub required:   bool,
    /// `Vec<T>` field, accept the option/positional more than once
    pub multi:      bool,
    pub group:      Option<&'static str>,
    pub default:    Option<&'static str>,
    pub default_missing: Option<&'static str>,
    /// name of an environment variable to fall back to when the arg is not
    /// given on the command line. read only with the `std` feature on.
    pub env:        Option<&'static str>,
    pub value_name: &'static str,
    pub help:       &'static str,
    pub possible:   Option<&'static [&'static str]>,
    /// kept out of help output, still accepted by the parser
    pub hidden:     bool,
    /// named flag/option that descendant subcommands also accept
    pub global:     bool,
}

impl ArgSpec {
    #[must_use]
    pub const fn new(kind: Kind) -> Self {
        Self {
            long: None,
            aliases: &[],
            short: None,
            kind,
            required: false,
            multi: false,
            group: None,
            default: None,
            default_missing: None,
            env: None,
            value_name: "",
            help: "",
            possible: None,
            hidden: false,
            global: false,
        }
    }

    #[must_use]
    pub const fn long(mut self, long: &'static str) -> Self {
        self.long = Some(long);
        self
    }

    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    #[must_use]
    pub const fn short(mut self, short: char) -> Self {
        self.short = Some(short);
        self
    }

    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub const fn multi(mut self) -> Self {
        self.multi = true;
        self
    }

    #[must_use]
    pub const fn group(mut self, group: &'static str) -> Self {
        self.group = Some(group);
        self
    }

    #[must_use]
    pub const fn default(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }

    #[must_use]
    pub const fn default_missing(mut self, default_missing: &'static str) -> Self {
        self.default_missing = Some(default_missing);
        self
    }

    #[must_use]
    pub const fn env(mut self, env: &'static str) -> Self {
        self.env = Some(env);
        self
    }

    #[must_use]
    pub const fn value_name(mut self, value_name: &'static str) -> Self {
        self.value_name = value_name;
        self
    }

    #[must_use]
    pub const fn help(mut self, help: &'static str) -> Self {
        self.help = help;
        self
    }

    #[must_use]
    pub const fn possible(mut self, possible: &'static [&'static str]) -> Self {
        self.possible = Some(possible);
        self
    }

    /// set the possible-value list from an already-optional source, e.g. a
    /// value enum's `POSSIBLE`. a `None` leaves the arg free-form.
    #[must_use]
    pub const fn possible_opt(mut self, possible: Option<&'static [&'static str]>) -> Self {
        self.possible = possible;
        self
    }

    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    #[must_use]
    pub const fn global(mut self) -> Self {
        self.global = true;
        self
    }

    /// true for kinds that consume a following token.
    #[must_use]
    pub const fn takes_value(&self) -> bool {
        matches!(self.kind, Kind::Opt)
    }

    /// true for kinds matched by position rather than by name.
    #[must_use]
    pub const fn is_positional(&self) -> bool {
        matches!(self.kind, Kind::Positional | Kind::Trailing)
    }

    /// name used in diagnostics: `--long`, else `-s`, else the value name.
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(long) = self.long {
            format!("--{long}")
        } else if let Some(short) = self.short {
            format!("-{short}")
        } else if !self.value_name.is_empty() {
            format!("<{}>", self.value_name)
        } else {
            "<value>".to_owned()
        }
    }
}

/// a mutually-exclusive set of args sharing a `group` name.
#[derive(Clone, Copy, Debug)]
pub struct GroupSpec {
    pub name:     &'static str,
    /// exactly one member must be set, not just at most one
    pub required: bool,
}

impl GroupSpec {
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            required: false,
        }
    }

    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// a child command plus the name that selects it.
#[derive(Clone, Copy, Debug)]
pub struct SubSpec {
    pub name:  &'static str,
    /// extra names that also select this subcommand, kept out of help
    pub aliases: &'static [&'static str],
    pub about: &'static str,
    pub spec:  &'static CommandSpec,
    /// kept out of help output, still selectable on the command line
    pub hidden: bool,
}

/// a command or subcommand: identity, args, groups, children.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    pub name:    &'static str,
    pub version: &'static str,
    pub about:   &'static str,
    pub args:    &'static [ArgSpec],
    pub groups:  &'static [GroupSpec],
    /// pairs of arg indices that cannot be set together (`conflicts_with`)
    pub conflicts: &'static [(usize, usize)],
    pub subs:    &'static [SubSpec],
    /// when true, a missing subcommand is allowed rather than showing help
    pub sub_optional: bool,
}

impl CommandSpec {
    /// whether this command dispatches to subcommands.
    #[must_use]
    pub const fn has_subs(&self) -> bool {
        !self.subs.is_empty()
    }

    /// index of the arg with this long name.
    #[must_use]
    // `contains(&name)` will not type-check: `aliases` holds `&'static str` and
    // `name` is a borrowed `&str`, so the membership test is spelled by hand.
    #[allow(clippy::manual_contains)]
    pub fn find_long(&self, name: &str) -> Option<usize> {
        self.args
            .iter()
            .position(|a| a.long == Some(name) || a.aliases.iter().any(|&al| al == name))
    }

    #[must_use]
    pub fn find_short(&self, ch: char) -> Option<usize> {
        self.args.iter().position(|a| a.short == Some(ch))
    }

    #[must_use]
    #[allow(clippy::manual_contains)]
    pub fn find_sub(&self, name: &str) -> Option<usize> {
        self.subs
            .iter()
            .position(|s| s.name == name || s.aliases.iter().any(|&al| al == name))
    }
}
