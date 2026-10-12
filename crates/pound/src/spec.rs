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
//! - descend from [`Command::root`] through [`Command::subcommands`], which
//!   carries each command's inherited globals down
//! - match [`Kind`] with a `_` arm
//! - construct spec types through their `const fn` builders
//!
//! `-h`/`--help` and `-V`/`--version` are generated unless a local arg or an
//! inherited global claims the spelling, and `-V`/`--version` also need version
//! or hash metadata. [`Command::builtins`] lists whichever survive. a command
//! with subcommands also takes `help <command>...`, see
//! [`Command::help_command`].
//!
//! the spec types are `#[non_exhaustive]` for forward compatibility

use alloc::vec::IntoIter;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::{
    Error,
    complete::Completer,
    help,
    value::{
        Choice,
        const_eq,
    },
};

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
    /// the rest of the line, from `--` or its own first value on
    Trailing,
}

/// what an arg's value names, so a shell completing it knows what to list
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Hint {
    /// nothing a shell can list
    #[default]
    Unknown,
    /// a file system path
    Path,
}

/// how many words one use of an option takes, at least one
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arity {
    min: usize,
    max: Option<usize>,
}

impl Arity {
    /// one word, as most options take
    pub const ONE: Self = Self::exactly(1);

    /// exactly `count` words, as `--move FROM TO` takes two
    ///
    /// # Panics
    ///
    /// when `count` is zero
    #[must_use]
    pub const fn exactly(count: usize) -> Self {
        Self::between(count, count)
    }

    /// `min` words or more, up to the next word that looks like an option
    ///
    /// # Panics
    ///
    /// when `min` is zero
    #[must_use]
    pub const fn at_least(min: usize) -> Self {
        assert!(min > 0, "an option takes at least one word");
        Self { min, max: None }
    }

    /// `min` to `max` words, stopping early at a word that looks like an
    /// option
    ///
    /// # Panics
    ///
    /// when `min` is zero or above `max`
    #[must_use]
    pub const fn between(min: usize, max: usize) -> Self {
        assert!(min > 0 && min <= max, "an option takes 1 to `max` words");
        Self {
            min,
            max: Some(max),
        }
    }

    /// the fewest words a use takes
    #[must_use]
    pub const fn min(self) -> usize {
        self.min
    }

    /// the most words a use takes, `None` for no limit
    #[must_use]
    pub const fn max(self) -> Option<usize> {
        self.max
    }
}

/// one argument's full description
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ArgSpec {
    /// the `--name` spelling, without the dashes
    pub long:            Option<&'static str>,
    /// extra long names that also match this arg, kept out of help
    pub aliases:         &'static [&'static str],
    /// the `-n` spelling
    pub short:           Option<char>,
    /// extra short names that also match this arg, kept out of help
    pub short_aliases:   &'static [char],
    /// the shape of the argument
    pub kind:            Kind,
    /// parsing fails when it is absent and no fallback fills it
    pub required:        bool,
    /// `Vec<T>` field, accept the option/positional more than once
    pub multi:           bool,
    /// fewest values a `multi` arg accepts, waived when a fallback fills it
    pub min_values:      Option<usize>,
    /// most values a `multi` arg accepts
    pub max_values:      Option<usize>,
    /// splits each value of a `multi` arg into several, `,` for `--features
    /// a,b`
    pub delimiter:       Option<char>,
    /// how many words one use of a `multi` option takes
    pub per_use:         Arity,
    /// the mutual-exclusion group this arg belongs to, see [`GroupSpec`]
    pub group:           Option<&'static str>,
    /// the value used when the arg is not given and no env var supplies one
    pub default:         Option<&'static str>,
    /// value a [`Kind::Opt`] takes when given with no `=value`, which also
    /// stops it consuming the following token
    pub default_missing: Option<&'static str>,
    /// a [`Kind::Opt`] written with no `=value` takes no value at all, which
    /// also stops it consuming the following token
    pub optional_value:  bool,
    /// name of an environment variable to fall back to when the arg is not
    /// given on the command line. disabled in nostd.
    pub env:             Option<&'static str>,
    /// long name that switches a [`Kind::Flag`] back off, without the `--`
    pub negate:          Option<&'static str>,
    /// the placeholder shown for the value, see [`Self::metavar`]
    pub value_name:      &'static str,
    /// the short description help shows
    pub help:            &'static str,
    /// fuller help shown by `--help`, `None` when it adds nothing
    pub long_help:       Option<&'static str>,
    /// section this arg is listed under in help, `Options` when unset
    pub heading:         Option<&'static str>,
    /// where help lists this flag or option among the others, in place of
    /// its position in the declaration
    pub display_order:   Option<usize>,
    /// the values a choice type accepts, `None` for free text
    pub possible:        Option<&'static [Choice]>,
    /// kept out of help output, but accepted by parser
    pub hidden:          bool,
    /// help and man pages leave out the possible values
    pub hide_possible:   bool,
    /// help and man pages leave out the default
    pub hide_default:    bool,
    /// help and man pages leave out the env var
    pub hide_env:        bool,
    /// takes a value that starts with a dash, as a positional when no option
    /// answers to the word, or as a word after a multi-word option's first
    pub hyphen_values:   bool,
    /// also accepted by every subcommand below this command
    pub global:          bool,
    /// given, it must be the only arg of its command, and it lifts the
    /// command's requirements
    pub exclusive:       bool,
    /// lists candidate values for shell completion
    pub complete:        Option<Completer>,
    /// what the value names, for a shell with no candidates to offer
    pub hint:            Hint,
}

impl ArgSpec {
    /// an arg of `kind` with nothing else set
    #[must_use]
    pub const fn new(kind: Kind) -> Self {
        Self {
            long: None,
            aliases: &[],
            short: None,
            short_aliases: &[],
            kind,
            required: false,
            multi: false,
            min_values: None,
            max_values: None,
            delimiter: None,
            per_use: Arity::ONE,
            group: None,
            default: None,
            default_missing: None,
            optional_value: false,
            env: None,
            negate: None,
            value_name: "",
            help: "",
            long_help: None,
            heading: None,
            display_order: None,
            possible: None,
            hidden: false,
            hide_possible: false,
            hide_default: false,
            hide_env: false,
            hyphen_values: false,
            global: false,
            exclusive: false,
            complete: None,
            hint: Hint::Unknown,
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
    pub const fn short_aliases(mut self, short_aliases: &'static [char]) -> Self {
        self.short_aliases = short_aliases;
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
    pub const fn delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = Some(delimiter);
        self
    }

    #[must_use]
    pub const fn per_use(mut self, per_use: Arity) -> Self {
        self.per_use = per_use;
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
    pub const fn possible(mut self, possible: &'static [Choice]) -> Self {
        self.possible = Some(possible);
        self
    }

    /// set the possible-value list (`None` is freeform)
    #[must_use]
    pub const fn possible_opt(mut self, possible: Option<&'static [Choice]>) -> Self {
        self.possible = possible;
        self
    }

    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    #[must_use]
    pub const fn display_order(mut self, display_order: usize) -> Self {
        self.display_order = Some(display_order);
        self
    }

    #[must_use]
    pub const fn hide_possible(mut self) -> Self {
        self.hide_possible = true;
        self
    }

    #[must_use]
    pub const fn hide_default(mut self) -> Self {
        self.hide_default = true;
        self
    }

    #[must_use]
    pub const fn hide_env(mut self) -> Self {
        self.hide_env = true;
        self
    }

    #[must_use]
    pub const fn optional_value(mut self) -> Self {
        self.optional_value = true;
        self
    }

    #[must_use]
    pub const fn hyphen_values(mut self) -> Self {
        self.hyphen_values = true;
        self
    }

    #[must_use]
    pub const fn global(mut self) -> Self {
        self.global = true;
        self
    }

    #[must_use]
    pub const fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }

    #[must_use]
    pub const fn complete(mut self, complete: Completer) -> Self {
        self.complete = Some(complete);
        self
    }

    #[must_use]
    pub const fn hint(mut self, hint: Hint) -> Self {
        self.hint = hint;
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

    /// whether `-c` reaches this arg, as its short name or an alias
    #[must_use]
    pub const fn answers_short(&self, short: char) -> bool {
        if matches!(self.short, Some(own) if own == short) {
            return true;
        }
        let mut i = 0;
        while i < self.short_aliases.len() {
            if self.short_aliases[i] == short {
                return true;
            }
            i += 1;
        }
        false
    }

    /// whether this option may be written without a value
    #[must_use]
    pub const fn value_may_be_left_out(&self) -> bool {
        self.optional_value || self.default_missing.is_some()
    }

    /// how many words one use of this option takes, `None` when it takes one
    #[must_use]
    pub fn words_per_use(&self) -> Option<Arity> {
        let takes_more = self.kind == Kind::Opt && self.multi && !self.value_may_be_left_out();
        (takes_more && self.per_use != Arity::ONE).then_some(self.per_use)
    }

    pub(crate) fn split<'w>(&self, word: &'w str) -> impl Iterator<Item = &'w str> {
        let delimiter = self.delimiter.filter(|_| self.multi);
        word.split(move |c| Some(c) == delimiter)
    }

    /// the placeholder usage and errors show for this arg's value, the
    /// `value_name` or else the long name, uppercased
    #[must_use]
    pub fn metavar(&self) -> String {
        let name = if self.value_name.is_empty() {
            self.long.unwrap_or("arg")
        } else {
            self.value_name
        };
        name.to_uppercase()
    }

    /// how errors and usage name this arg, its `--long`, else its `-s`, else
    /// its placeholder
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(long) = self.long {
            format!("--{long}")
        } else if let Some(short) = self.short {
            format!("-{short}")
        } else {
            self.metavar()
        }
    }
}

/// mutually-exclusive set of args sharing a `group` name
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct GroupSpec {
    /// the name args use to join the group
    pub name:     &'static str,
    /// exactly one member must be set
    pub required: bool,
}

impl GroupSpec {
    /// an optional group named `name`
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
    /// the index of an arg in `args`
    Direct(usize),
    /// the index of a struct in `flattened`
    Flattened(usize),
}

/// a child command plus the name that selects it
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct SubSpec {
    /// the name that selects the subcommand
    pub name:          &'static str,
    /// extra names that also select this subcommand, kept out of help
    pub aliases:       &'static [&'static str],
    /// the short description subcommand listings show
    pub about:         &'static str,
    /// the spec the subcommand parses against
    pub spec:          &'static CommandSpec,
    /// kept out of help output, still selectable on the command line
    pub hidden:        bool,
    /// offers `spec`'s subcommands at this level in place of a named command
    pub flattened:     bool,
    /// takes any name no other subcommand answers to, with the rest of the
    /// line
    pub external:      bool,
    /// where help lists this command among the others, in place of its
    /// position in the declaration
    pub display_order: Option<usize>,
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
            external: false,
            display_order: None,
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

    /// an entry taking any subcommand name no other entry answers to, the way
    /// `cargo foo` runs `cargo-foo`. it parses the name and every word after
    /// it, flags included, into one trailing list. it is never matched by name
    /// and stays out of help.
    #[must_use]
    pub const fn external() -> Self {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Trailing)
            .value_name("command")
            .required()];
        const SPEC: CommandSpec = CommandSpec::new("").args(ARGS);

        Self {
            hidden: true,
            external: true,
            ..Self::new("", &SPEC)
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
    pub const fn display_order(mut self, display_order: usize) -> Self {
        self.display_order = Some(display_order);
        self
    }
}

const HELP_SPEC: CommandSpec = CommandSpec::new("help");
static HELP_COMMAND: SubSpec = SubSpec::new("help", &HELP_SPEC).about("Print help for a command");

/// a command or subcommand: identity, args, groups, children
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CommandSpec {
    /// the command name usage and help show
    pub name:           &'static str,
    /// when true, parsing the process's argv shows the program's file name
    /// in place of `name`
    pub invoked_name:   bool,
    /// the text `--version` prints, empty when there is none
    pub version:        &'static str,
    /// fuller text `--version` prints in place of `version`, while `-V` keeps
    /// the short line, empty when there is none
    pub long_version:   &'static str,
    /// commit hash for the compiled program's source
    pub hash:           Option<&'static str>,
    /// the short description help shows
    pub about:          &'static str,
    /// fuller description shown by `--help`, empty when it adds nothing
    pub long_about:     &'static str,
    /// text help shows above everything else, empty for none
    pub before_help:    &'static str,
    /// text help shows below everything else, such as examples, empty for none
    pub after_help:     &'static str,
    /// the args declared directly on this command
    pub args:           &'static [ArgSpec],
    /// structs embedded with `#[pound(flatten)]`, whose args parse at this
    /// level
    pub flattened:      &'static [&'static Self],
    /// where each direct arg and flattened struct was declared, empty to put
    /// the direct args first
    pub argument_order: &'static [ArgumentOrder],
    /// the mutual-exclusion groups the args refer to
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
    /// when true, a command given no words at all prints its help instead
    pub help_if_empty:  bool,
}

impl CommandSpec {
    /// a command named `name`, with no args, groups, conflicts, or subs yet
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            invoked_name: false,
            version: "",
            long_version: "",
            hash: None,
            about: "",
            long_about: "",
            before_help: "",
            after_help: "",
            args: &[],
            flattened: &[],
            argument_order: &[],
            groups: &[],
            conflicts: &[],
            requires: &[],
            subs: &[],
            sub_optional: false,
            help_if_empty: false,
        }
    }

    #[must_use]
    pub const fn invoked_name(mut self) -> Self {
        self.invoked_name = true;
        self
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

    /// like [`Self::hash`], but a `None` or empty value leaves the hash unset
    #[must_use]
    pub const fn hash_opt(self, hash: Option<&'static str>) -> Self {
        match hash {
            Some(hash) if !hash.is_empty() => self.hash(hash),
            _ => self,
        }
    }

    /// whether `--version` has anything to print
    #[must_use]
    pub const fn has_version_info(&self) -> bool {
        !self.version.is_empty() || !self.long_version.is_empty() || self.hash.is_some()
    }

    #[must_use]
    pub const fn long_version(mut self, long_version: &'static str) -> Self {
        self.long_version = long_version;
        self
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
    pub const fn before_help(mut self, before_help: &'static str) -> Self {
        self.before_help = before_help;
        self
    }

    #[must_use]
    pub const fn after_help(mut self, after_help: &'static str) -> Self {
        self.after_help = after_help;
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

    /// print help rather than parse when the command is given no words
    #[must_use]
    pub const fn help_if_empty(mut self) -> Self {
        self.help_if_empty = true;
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
        in_display_order(out, |sub| sub.display_order).into_iter()
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
            .find(|s| !s.external && (s.name == name || s.aliases.contains(&name)))
    }
}

/// a command as the parser reaches it, its spec plus what the commands above
/// it hand down
///
/// start at [`Command::root`] and descend with [`Command::subcommands`], which
/// works out each child's inherited globals.
#[derive(Clone, Debug)]
pub struct Command<'s> {
    spec:    &'s CommandSpec,
    hidden:  bool,
    path:    Vec<&'static str>,
    globals: Vec<&'static ArgSpec>,
}

impl<'s> Command<'s> {
    /// the top of a command tree
    #[must_use]
    pub const fn root(spec: &'s CommandSpec) -> Self {
        Self {
            spec,
            hidden: false,
            path: Vec::new(),
            globals: Vec::new(),
        }
    }

    #[must_use]
    pub const fn spec(&self) -> &'s CommandSpec {
        self.spec
    }

    /// the names of the commands above this one, root first
    #[must_use]
    pub fn path(&self) -> &[&'static str] {
        &self.path
    }

    /// the globals this command inherits from the commands above it, nearest
    /// last
    #[must_use]
    pub fn globals(&self) -> &[&'static ArgSpec] {
        &self.globals
    }

    /// whether the subcommand that reaches this command is hidden
    #[must_use]
    pub const fn hidden(&self) -> bool {
        self.hidden
    }

    /// every subcommand, hidden ones included, with flattened enums spliced in
    pub fn subcommands(&self) -> impl Iterator<Item = Self> + '_ {
        self.spec
            .subcommands()
            .map(|sub| -> Self { self.child(sub) })
    }

    pub(crate) fn child(&self, sub: &'static SubSpec) -> Command<'static> {
        let mut globals = self.globals.clone();
        globals.extend(self.spec.arguments().filter(|a| a.global));
        let mut path = self.path.clone();
        path.push(self.spec.name);

        Command {
            spec: sub.spec,
            hidden: sub.hidden,
            path,
            globals,
        }
    }

    /// every name `-` or `--` reaches here and help lists, in help's order.
    /// the command's own visible flags and options, then
    /// [`Self::inherited`], then [`Self::builtins`].
    #[must_use]
    pub fn options(&self) -> Vec<ArgSpec> {
        self.own_options()
            .into_iter()
            .copied()
            .chain(self.inherited())
            .chain(self.builtins())
            .collect()
    }

    /// the arg `--name` reaches here, one of the command's own before an
    /// inherited global, and a nearer global before a farther one
    #[must_use]
    pub fn long(&self, name: &str) -> Option<&'static ArgSpec> {
        let globals = self.globals.iter().rev().copied();
        self.spec
            .arguments()
            .chain(globals)
            .find(|a| a.answers_long(name))
    }

    /// the arg `-c` reaches here, by the same precedence as [`Self::long`]
    #[must_use]
    pub fn short(&self, c: char) -> Option<&'static ArgSpec> {
        let globals = self.globals.iter().rev().copied();
        self.spec
            .arguments()
            .chain(globals)
            .find(|a| a.answers_short(c))
    }

    /// the visible flags and options declared on this command, in the order
    /// help lists them
    #[must_use]
    pub fn own_options(&self) -> Vec<&'static ArgSpec> {
        let options = self
            .spec
            .arguments()
            .filter(|a| !a.hidden && !a.is_positional());
        in_display_order(options, |a| a.display_order)
    }

    /// the visible positionals, in the order they fill
    pub fn positionals(&self) -> impl Iterator<Item = &'static ArgSpec> {
        self.spec
            .arguments()
            .filter(|a| !a.hidden && a.is_positional())
    }

    /// the inherited globals still reachable here, hidden ones left out and
    /// each stripped of the spellings that a nearer arg answers to instead
    #[must_use]
    pub fn inherited(&self) -> Vec<ArgSpec> {
        let own = self.spec.arguments().collect::<Vec<_>>();
        let globals = &self.globals;
        globals
            .iter()
            .enumerate()
            .filter(|(_, a)| !a.hidden)
            .filter_map(|(i, &&global)| {
                let nearer = &globals[i + 1..];
                let mut shown = global;
                shown.short = shown.short.filter(|&c| !claims_short(&own, nearer, c));
                shown.long = shown.long.filter(|l| !claims_long(&own, nearer, l));
                shown.negate = shown.negate.filter(|n| !claims_long(&own, nearer, n));
                if shown.long.is_none() {
                    shown.long = shown.negate.take();
                }
                (shown.short.is_some() || shown.long.is_some()).then_some(shown)
            })
            .collect()
    }

    /// the generated `-h`/`--help` and `-V`/`--version` flags, each with only
    /// the spellings that no arg of this command or inherited global claims.
    /// the parser answers each spelling on its own, so a command that takes
    /// `-h` for itself still gets `--help`.
    #[must_use]
    pub fn builtins(&self) -> Vec<ArgSpec> {
        let own = self.spec.arguments().collect::<Vec<_>>();
        let builtin = |short, long, help| {
            let free_short = !claims_short(&own, &self.globals, short);
            let free_long = !claims_long(&own, &self.globals, long);
            let mut a = ArgSpec::new(Kind::Flag).help(help);
            if free_short {
                a = a.short(short);
            }
            if free_long {
                a = a.long(long);
            }
            (free_short || free_long).then_some(a)
        };

        let version = self
            .spec
            .has_version_info()
            .then(|| builtin('V', "version", "Print version"));
        builtin('h', "help", "Print help")
            .into_iter()
            .chain(version.flatten())
            .collect()
    }

    /// the generated `help` command, which takes the names of the command to
    /// show help for. a command with subcommands has it unless one of them
    /// answers to `help`, or an external one takes the names nothing else
    /// does.
    #[must_use]
    pub fn help_command(&self) -> Option<&'static SubSpec> {
        let taken = self
            .spec
            .subcommands()
            .any(|s| s.external || s.name == "help" || s.aliases.contains(&"help"));
        (self.spec.has_subs() && !taken).then_some(&HELP_COMMAND)
    }

    /// the spelling that still reaches the generated help, if any does
    pub(crate) fn help_flag(&self) -> Option<&'static str> {
        let own = self.spec.arguments().collect::<Vec<_>>();
        if !claims_long(&own, &self.globals, "help") {
            Some("--help")
        } else if !claims_short(&own, &self.globals, 'h') {
            Some("-h")
        } else {
            None
        }
    }

    /// tag `err` with this command's usage line, unless a nested command
    /// already claimed it
    pub(crate) fn locate(&self, err: Error) -> Error {
        err.or_usage(|| (help::usage_line(self), self.help_flag()))
    }
}

/// `items` with each one that asks for a display order placed at that
/// position, and the rest filling the gaps in their own order
fn in_display_order<T: Copy>(
    items: impl IntoIterator<Item = T>,
    order: impl Fn(T) -> Option<usize>,
) -> Vec<T> {
    let (mut asked, free) = items
        .into_iter()
        .partition::<Vec<_>, _>(|&item| order(item).is_some());
    asked.sort_by_key(|&item| order(item));

    let mut placed = asked.into_iter().peekable();
    let mut rest = free.into_iter().peekable();
    let mut out = Vec::new();
    loop {
        let due = placed
            .peek()
            .is_some_and(|&item| order(item) <= Some(out.len()));
        let next = if due || rest.peek().is_none() {
            placed.next()
        } else {
            rest.next()
        };
        let Some(item) = next else {
            return out;
        };
        out.push(item);
    }
}

/// whether `--name` reaches one of a command's own args or a global it inherits
pub(crate) fn claims_long(own: &[&ArgSpec], globals: &[&ArgSpec], name: &str) -> bool {
    own.iter().chain(globals).any(|a| a.answers_long(name))
}

pub(crate) fn claims_short(own: &[&ArgSpec], globals: &[&ArgSpec], short: char) -> bool {
    own.iter().chain(globals).any(|a| a.answers_short(short))
}
