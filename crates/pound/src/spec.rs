// SPDX-License-Identifier: EUPL-1.2

//! the static description of a command line
//!
//! # introspection
//!
//! this module is pound's introspection surface.
//! [`CommandSpec`] provides all methods necessary to traverse the command tree
//!
//! a walker implementation should:
//! - access root with [`crate::Parse::SPEC`], or take a `&CommandSpec`.
//! - read a command through [`CommandSpec::arguments`] and
//!   [`CommandSpec::subcommands`], which follow flattening, not the raw `args`
//!   and `subs` fields
//! - propagate globals downward
//! - match [`Kind`] with a `_` arm
//! - construct spec types through their `const fn` builders
//!
//! `-h`/`--help` and `-V`/`--version` are generated unless a local arg or an
//! inherited global claims the spelling, and `-V`/`--version` also need version
//! or hash metadata
//!
//! the spec types are `#[non_exhaustive]` for forward compatibility

use alloc::vec::IntoIter;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::value::const_eq;

/// what shape of argument a spec entry describes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// boolean, presence means true
    Flag,
    /// repeatable switch counted into an int (expects u8 or u32)
    Count,
    /// named option taking a value
    Opt,
    /// bare value matched by position
    Positional,
    /// everything after `--`, or the trailing variadic
    Trailing,
}

/// one argument's full description
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ArgSpec {
    pub long: Option<&'static str>,
    /// extra long names that also match this arg, kept out of help
    pub aliases: &'static [&'static str],
    pub short: Option<char>,
    pub kind: Kind,
    pub required: bool,
    /// `Vec<T>` field, accept the option/positional more than once
    pub multi: bool,
    /// fewest values a `multi` arg accepts, waived when a fallback fills it
    pub min_values: Option<usize>,
    /// most values a `multi` arg accepts
    pub max_values: Option<usize>,
    pub group: Option<&'static str>,
    pub default: Option<&'static str>,
    /// value a [`Kind::Opt`] takes when given with no `=value`, which also
    /// stops it consuming the following token
    pub default_missing: Option<&'static str>,
    /// name of an environment variable to fall back to when the arg is not
    /// given on the command line. disabled in nostd.
    pub env: Option<&'static str>,
    /// long name that switches a [`Kind::Flag`] back off, without the `--`
    pub negate: Option<&'static str>,
    pub value_name: &'static str,
    pub help: &'static str,
    /// fuller help shown by `--help`, `None` when it adds nothing
    pub long_help: Option<&'static str>,
    /// section this arg is listed under in help, `Options` when unset
    pub heading: Option<&'static str>,
    pub possible: Option<&'static [&'static str]>,
    /// kept out of help output, but accepted by parser
    pub hidden: bool,
    pub global: bool,
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
            min_values: None,
            max_values: None,
            group: None,
            default: None,
            default_missing: None,
            env: None,
            negate: None,
            value_name: "",
            help: "",
            long_help: None,
            heading: None,
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
    pub const fn min_values(mut self, min_values: usize) -> Self {
        self.min_values = Some(min_values);
        self
    }

    #[must_use]
    pub const fn max_values(mut self, max_values: usize) -> Self {
        self.max_values = Some(max_values);
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
    pub const fn negate(mut self, negate: &'static str) -> Self {
        self.negate = Some(negate);
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
    pub const fn long_help(mut self, long_help: &'static str) -> Self {
        self.long_help = Some(long_help);
        self
    }

    #[must_use]
    pub const fn heading(mut self, heading: &'static str) -> Self {
        self.heading = Some(heading);
        self
    }

    #[must_use]
    pub const fn possible(mut self, possible: &'static [&'static str]) -> Self {
        self.possible = Some(possible);
        self
    }

    /// set the possible-value list (`None` is freeform)
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

    /// true for kinds that consume a following token
    #[must_use]
    pub const fn takes_value(&self) -> bool {
        matches!(self.kind, Kind::Opt)
    }

    /// true for kinds matched by position rather than by name
    #[must_use]
    pub const fn is_positional(&self) -> bool {
        matches!(self.kind, Kind::Positional | Kind::Trailing)
    }

    /// whether `--name` reaches this arg, as its long name, an alias, or its
    /// negation
    #[must_use]
    pub const fn answers_long(&self, name: &str) -> bool {
        if let Some(long) = self.long
            && const_eq(long, name)
        {
            return true;
        }
        if let Some(negate) = self.negate
            && const_eq(negate, name)
        {
            return true;
        }
        let mut i = 0;
        while i < self.aliases.len() {
            if const_eq(self.aliases[i], name) {
                return true;
            }
            i += 1;
        }
        false
    }

    #[must_use]
    pub const fn answers_short(&self, short: char) -> bool {
        matches!(self.short, Some(own) if own == short)
    }

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

/// mutually-exclusive set of args sharing a `group` name
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct GroupSpec {
    pub name: &'static str,
    /// exactly one member must be set
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

/// one entry of [`CommandSpec::argument_order`], an index into `args` or
/// `flattened`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentOrder {
    Direct(usize),
    Flattened(usize),
}

/// a child command plus the name that selects it
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct SubSpec {
    pub name:      &'static str,
    /// extra names that also select this subcommand, kept out of help
    pub aliases:   &'static [&'static str],
    pub about:     &'static str,
    pub spec:      &'static CommandSpec,
    /// kept out of help output, still selectable on the command line
    pub hidden:    bool,
    /// offers `spec`'s subcommands at this level in place of a named command
    pub flattened: bool,
}

impl SubSpec {
    /// child command selected by `name`, dispatching to `spec`
    #[must_use]
    pub const fn new(name: &'static str, spec: &'static CommandSpec) -> Self {
        Self {
            name,
            aliases: &[],
            about: "",
            spec,
            hidden: false,
            flattened: false,
        }
    }

    /// an entry offering every subcommand of `spec` as if declared here. it is
    /// never matched by name, so `aliases`, `about` and `hidden` are ignored.
    #[must_use]
    pub const fn flatten(spec: &'static CommandSpec) -> Self {
        Self {
            flattened: true,
            ..Self::new("", spec)
        }
    }

    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    #[must_use]
    pub const fn about(mut self, about: &'static str) -> Self {
        self.about = about;
        self
    }

    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
}

/// a command or subcommand: identity, args, groups, children
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CommandSpec {
    pub name:           &'static str,
    pub version:        &'static str,
    /// commit hash for the compiled program's source
    pub hash:           Option<&'static str>,
    pub about:          &'static str,
    /// fuller description shown by `--help`, empty when it adds nothing
    pub long_about:     &'static str,
    pub args:           &'static [ArgSpec],
    /// structs embedded with `#[pound(flatten)]`, whose args parse at this
    /// level
    pub flattened:      &'static [&'static Self],
    /// where each direct arg and flattened struct was declared, empty to put
    /// the direct args first
    pub argument_order: &'static [ArgumentOrder],
    pub groups:         &'static [GroupSpec],
    /// pairs of arg indices that cannot be set together
    pub conflicts:      &'static [(usize, usize)],
    /// pairs where setting the first arg obliges the second
    pub requires:       &'static [(usize, usize)],
    /// declared subcommands, including the nameless entries that splice in
    /// another enum, see [`Self::subcommands`] for the commands themselves
    pub subs:           &'static [SubSpec],
    /// when true, a missing subcommand is allowed rather than showing help
    pub sub_optional:   bool,
}

impl CommandSpec {
    /// a command named `name`, with no args, groups, conflicts, or subs yet
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            version: "",
            hash: None,
            about: "",
            long_about: "",
            args: &[],
            flattened: &[],
            argument_order: &[],
            groups: &[],
            conflicts: &[],
            requires: &[],
            subs: &[],
            sub_optional: false,
        }
    }

    #[must_use]
    pub const fn version(mut self, version: &'static str) -> Self {
        self.version = version;
        self
    }

    #[must_use]
    pub const fn hash(mut self, hash: &'static str) -> Self {
        self.hash = Some(hash);
        self
    }

    #[must_use]
    pub const fn has_version_info(&self) -> bool {
        !self.version.is_empty() || self.hash.is_some()
    }

    #[must_use]
    pub const fn about(mut self, about: &'static str) -> Self {
        self.about = about;
        self
    }

    #[must_use]
    pub const fn long_about(mut self, long_about: &'static str) -> Self {
        self.long_about = long_about;
        self
    }

    #[must_use]
    pub const fn args(mut self, args: &'static [ArgSpec]) -> Self {
        self.args = args;
        self
    }

    #[must_use]
    pub const fn flattened(mut self, flattened: &'static [&'static Self]) -> Self {
        self.flattened = flattened;
        self
    }

    /// interleave the direct args and flattened structs in source order, which
    /// decides the order positionals fill in
    #[must_use]
    pub const fn argument_order(mut self, order: &'static [ArgumentOrder]) -> Self {
        self.argument_order = order;
        self
    }

    #[must_use]
    pub const fn groups(mut self, groups: &'static [GroupSpec]) -> Self {
        self.groups = groups;
        self
    }

    #[must_use]
    pub const fn conflicts(mut self, conflicts: &'static [(usize, usize)]) -> Self {
        self.conflicts = conflicts;
        self
    }

    #[must_use]
    pub const fn requires(mut self, requires: &'static [(usize, usize)]) -> Self {
        self.requires = requires;
        self
    }

    #[must_use]
    pub const fn subs(mut self, subs: &'static [SubSpec]) -> Self {
        self.subs = subs;
        self
    }

    /// allow a missing subcommand rather than showing help
    #[must_use]
    pub const fn sub_optional(mut self) -> Self {
        self.sub_optional = true;
        self
    }

    /// whether this command dispatches to subcommands
    #[must_use]
    pub const fn has_subs(&self) -> bool {
        self.selector().is_some()
    }

    /// the spec whose `subs` this command selects from, its own or the one
    /// inside a flattened struct
    pub(crate) const fn selector(&self) -> Option<&Self> {
        if !self.subs.is_empty() {
            return Some(self);
        }
        let mut i = 0;
        while i < self.flattened.len() {
            if let Some(owner) = self.flattened[i].selector() {
                return Some(owner);
            }
            i += 1;
        }
        None
    }

    /// whether a command line may stop before naming a subcommand
    #[must_use]
    pub const fn subcommand_optional(&self) -> bool {
        match self.selector() {
            Some(owner) => owner.sub_optional,
            None => true,
        }
    }

    /// the subcommands this command offers, with flattened enums spliced in
    pub fn subcommands(&self) -> IntoIter<&'static SubSpec> {
        fn expand(subs: &'static [SubSpec], out: &mut Vec<&'static SubSpec>) {
            for sub in subs {
                if sub.flattened {
                    expand(sub.spec.subs, out);
                } else {
                    out.push(sub);
                }
            }
        }

        let mut out = Vec::new();
        if let Some(owner) = self.selector() {
            expand(owner.subs, &mut out);
        }
        out.into_iter()
    }

    /// every arg this command parses, flattened structs included, in
    /// declaration order
    pub fn arguments(&self) -> IntoIter<&'static ArgSpec> {
        let mut out = Vec::new();
        self.visit(&mut Vec::new(), &mut |_, _, arg| out.push(arg));
        out.into_iter()
    }

    pub(crate) const fn order_len(&self) -> usize {
        if self.argument_order.is_empty() {
            self.args.len() + self.flattened.len()
        } else {
            self.argument_order.len()
        }
    }

    /// the field declared at `position`, with direct args first when no
    /// `argument_order` was given
    pub(crate) const fn order_entry(&self, position: usize) -> ArgumentOrder {
        if !self.argument_order.is_empty() {
            self.argument_order[position]
        } else if position < self.args.len() {
            ArgumentOrder::Direct(position)
        } else {
            ArgumentOrder::Flattened(position - self.args.len())
        }
    }

    /// call `f` on each arg in declaration order, with the flattened indices
    /// leading to the spec that owns it and its index there
    pub(crate) fn visit(
        &self,
        path: &mut Vec<usize>,
        f: &mut impl FnMut(&[usize], usize, &'static ArgSpec),
    ) {
        let args: &'static [ArgSpec] = self.args;
        for position in 0..self.order_len() {
            match self.order_entry(position) {
                ArgumentOrder::Direct(i) => f(path, i, &args[i]),
                ArgumentOrder::Flattened(i) => {
                    path.push(i);
                    self.flattened[i].visit(path, f);
                    path.pop();
                },
            }
        }
    }

    /// the subcommand `name` selects, by name or alias
    #[must_use]
    pub fn find_sub(&self, name: &str) -> Option<&'static SubSpec> {
        self.subcommands()
            .find(|s| s.name == name || s.aliases.contains(&name))
    }
}

/// whether `--name` reaches one of a command's own args or a global it inherits
pub(crate) fn claims_long(own: &[&ArgSpec], globals: &[&ArgSpec], name: &str) -> bool {
    own.iter().chain(globals).any(|a| a.answers_long(name))
}

pub(crate) fn claims_short(own: &[&ArgSpec], globals: &[&ArgSpec], short: char) -> bool {
    own.iter().chain(globals).any(|a| a.answers_short(short))
}
