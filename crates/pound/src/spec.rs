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
//! - propagate globals downward
//! - match [`Kind`] with a `_` arm
//! - construct spec types through their `const fn` builders
//!
//! `-h`/`--help` are generated unless a local argument or inherited global
//! claims the spelling. `-V`/`--version` also require version or hash metadata
//!
//! the spec types are `#[non_exhaustive]` for forward compatibility

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;

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

/// a child command plus the name that selects it
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct SubSpec {
    pub name: &'static str,
    /// extra names that also select this subcommand, kept out of help
    pub aliases: &'static [&'static str],
    pub about: &'static str,
    pub spec: &'static CommandSpec,
    /// kept out of help output, still selectable on the command line
    pub hidden: bool,
    /// contributes the referenced command choices at this level
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

    #[must_use]
    pub const fn flattened(mut self) -> Self {
        self.flattened = true;
        self
    }
}

/// a command or subcommand: identity, args, groups, children
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CommandSpec {
    pub name: &'static str,
    pub version: &'static str,
    /// commit hash for the compiled program's source
    pub hash: Option<&'static str>,
    pub about: &'static str,
    /// fuller description shown by `--help`, empty when it adds nothing
    pub long_about: &'static str,
    pub args: &'static [ArgSpec],
    /// embedded parsed types
    pub flattened: &'static [&'static Self],
    /// source order for direct and flattened fields
    #[doc(hidden)]
    pub argument_order: &'static [ArgumentOrder],
    pub groups: &'static [GroupSpec],
    /// pairs of arg indices that cannot be set together
    pub conflicts: &'static [(usize, usize)],
    /// pairs where setting the first arg obliges the second
    pub requires: &'static [(usize, usize)],
    pub subs: &'static [SubSpec],
    /// when true, a missing subcommand is allowed rather than showing help
    pub sub_optional: bool,
}

/// direct or flattened field in source order
/// public for derive output
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgumentOrder {
    Direct(usize),
    Flattened(usize),
}

/// arguments in effective declaration order
pub struct Arguments<'a> {
    pending: Vec<ArgumentEntry<'a>>,
}

/// effective subcommands in declaration order
pub struct CommandChildren<'a> {
    pending: Vec<&'a SubSpec>,
}

impl<'a> Iterator for CommandChildren<'a> {
    type Item = &'a SubSpec;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(sub) = self.pending.pop() {
            if sub.flattened {
                self.pending.extend(sub.spec.subs.iter().rev());
            } else {
                return Some(sub);
            }
        }
        None
    }
}

enum ArgumentEntry<'a> {
    Direct(&'a ArgSpec),
    Flattened(&'a CommandSpec),
}

impl<'a> Arguments<'a> {
    fn new(spec: &'a CommandSpec) -> Self {
        let mut pending = Vec::new();
        push_argument_entries(&mut pending, spec);
        Self { pending }
    }
}

impl<'a> Iterator for Arguments<'a> {
    type Item = &'a ArgSpec;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.pending.pop() {
            match entry {
                ArgumentEntry::Direct(argument) => return Some(argument),
                ArgumentEntry::Flattened(spec) => push_argument_entries(&mut self.pending, spec),
            }
        }
        None
    }
}

fn push_argument_entries<'a>(pending: &mut Vec<ArgumentEntry<'a>>, spec: &'a CommandSpec) {
    if spec.argument_order.is_empty() {
        pending.extend(
            spec.flattened
                .iter()
                .rev()
                .copied()
                .map(ArgumentEntry::Flattened),
        );
        pending.extend(spec.args.iter().rev().map(ArgumentEntry::Direct));
        return;
    }

    pending.extend(spec.argument_order.iter().rev().filter_map(|entry| {
        match *entry {
            ArgumentOrder::Direct(index) => spec.args.get(index).map(ArgumentEntry::Direct),
            ArgumentOrder::Flattened(index) => {
                spec.flattened
                    .get(index)
                    .copied()
                    .map(ArgumentEntry::Flattened)
            },
        }
    }));
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

    /// embed another parsed type at this command level
    #[must_use]
    pub const fn flattened(mut self, flattened: &'static [&'static Self]) -> Self {
        self.flattened = flattened;
        self
    }

    /// set source order for direct and flattened fields
    #[doc(hidden)]
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
        self.subcommand_owner().is_some()
    }

    pub(crate) const fn subcommand_owner(&self) -> Option<&Self> {
        if !self.subs.is_empty() {
            return Some(self);
        }
        let mut index = 0;
        while index < self.flattened.len() {
            if let Some(owner) = self.flattened[index].subcommand_owner() {
                return Some(owner);
            }
            index += 1;
        }
        None
    }

    /// whether a missing subcommand is allowed
    #[must_use]
    pub const fn subcommand_optional(&self) -> bool {
        match self.subcommand_owner() {
            Some(owner) => owner.sub_optional,
            None => true,
        }
    }

    /// subcommands at this command level
    pub fn subcommands(&self) -> CommandChildren<'_> {
        CommandChildren {
            pending: self
                .subcommand_owner()
                .map(|owner| owner.subs.iter().rev().collect())
                .unwrap_or_default(),
        }
    }

    /// arguments at this command level
    pub fn arguments(&self) -> Arguments<'_> {
        Arguments::new(self)
    }

    /// find a long argument including flattened fields
    #[must_use]
    #[allow(clippy::manual_contains)]
    pub fn find_long(&self, name: &str) -> Option<&ArgSpec> {
        self.arguments()
            .find(|arg| arg.long == Some(name) || arg.aliases.iter().any(|&alias| alias == name))
    }

    /// find a short argument including flattened fields
    #[must_use]
    pub fn find_short(&self, ch: char) -> Option<&ArgSpec> {
        self.arguments().find(|arg| arg.short == Some(ch))
    }

    /// find a negation name including flattened fields
    #[must_use]
    pub fn find_negate(&self, name: &str) -> Option<&ArgSpec> {
        self.arguments().find(|arg| arg.negate == Some(name))
    }

    #[must_use]
    #[allow(clippy::manual_contains)]
    pub fn find_sub(&self, name: &str) -> Option<&SubSpec> {
        self.subcommands()
            .find(|s| s.name == name || s.aliases.iter().any(|&al| al == name))
    }
}

pub(crate) fn accepts_long<'a>(mut args: impl Iterator<Item = &'a ArgSpec>, name: &str) -> bool {
    args.any(|arg| {
        arg.long == Some(name) || arg.aliases.contains(&name) || arg.negate == Some(name)
    })
}

pub(crate) fn accepts_short<'a>(mut args: impl Iterator<Item = &'a ArgSpec>, short: char) -> bool {
    args.any(|arg| arg.short == Some(short))
}
