// SPDX-License-Identifier: EUPL-1.2

//! every key `#[pound(...)]` takes, one unit struct each
//!
//! the derives refer to each key written, so rust-analyzer completes the keys
//! and shows these docs on hover, and rustc rejects a misspelled one with the
//! closest match. the full rules live on [`Parse`](crate::Parse).

#![expect(
    non_camel_case_types,
    reason = "each struct is spelled as the key it documents"
)]

/// item: the command name in usage and help. subcommand: its name, kebab case
/// by default. value: the spelling in place of the kebab case one
pub struct name;

/// item: the `--version` text, any `&'static str` expression. defaults to
/// `CARGO_PKG_VERSION`
pub struct version;

/// item: the fuller text `--version` prints, while `-V` keeps `version`
pub struct long_version;

/// item or subcommand: exactly one member of the named group must be given.
/// may repeat
pub struct required_group;

/// item or subcommand: given no words at all, print help to stderr and exit 2
pub struct help_if_empty;

/// item or subcommand: text the help page opens with, above the description
pub struct before_help;

/// item or subcommand: text the help page closes with, such as examples
pub struct after_help;

/// field: `--name`, from the field name with `_` as `-`. `long = "other"`
/// picks the spelling
pub struct long;

/// field: `-n`, the field name's first letter. `short = 'x'` picks the letter
pub struct short;

/// field: a positional, which a field with neither `short` nor `long` is
/// already
pub struct positional;

/// field: collect the rest of the line, flags too, from `--` or its first
/// value. must be the last positional
pub struct trailing;

/// field: count repeats of a flag into an integer
pub struct count;

/// field: holds the subcommand tree. use `Option<T>` when one is not required
pub struct subcommand;

/// field: embed another `Parse` struct's args at this level. subcommand: offer
/// a held enum's commands as if declared here
pub struct flatten;

/// subcommand: on a `Vec<T>` or `pound::External` tuple variant, take any
/// other subcommand name and what follows
pub struct external;

/// field: the only arg allowed when given, lifting requirements. every other
/// field must be optional
pub struct exclusive;

/// field: offer a flag or option to every subcommand below this command too
pub struct global;

/// field: a positional takes `-x` when no option answers to it, and a
/// `num_args` option reads past words like `-x`
pub struct allow_hyphen_values;

/// field: extra long names. subcommand: extra names. value: extra spellings,
/// never listed. one as `"a"`, several as `["a", "b"]`
pub struct alias;

/// field: extra short names for a flag or option, one as `'x'`, several as
/// `['x', 'y']`
pub struct short_alias;

/// field: the value when the arg is not given. `"auto"`, a bare `auto`, a
/// value enum variant like `Format::Json`, or a braced `&str` constant
pub struct default;

/// field: the value of an option written with no value, so `--color` alone
/// means `--color=v`
pub struct default_missing;

/// field: read this environment variable when the arg is not given
pub struct env;

/// field: on a flag, also accept `--no-<long>` to switch it off. `negate =
/// "off"` names it
pub struct negate;

/// field: read the raw `&str` with this function in place of `FromArg`
pub struct parse;

/// field: check the parsed value with this function, taking `&T` and
/// returning `Result<(), E>`
pub struct validate;

/// field: the parsed value must be at least this, as `min = -1`
pub struct min;

/// field: the parsed value must be at most this
pub struct max;

/// field: the raw text may have at most this many characters
pub struct max_len;

/// field: a `Vec` must receive at least this many values
pub struct min_values;

/// field: a `Vec` may receive at most this many values
pub struct max_values;

/// field: a `Vec` splits each value at this character, `env` and `default`
/// too
pub struct delimiter;

/// field: how many words one use of a `Vec` option takes, as `2`, `1..` or
/// `1..=3`
pub struct num_args;

/// field: list shell completion candidates with this
/// `pound::complete::Completer`
pub struct complete;

/// field: what the value names for completion, a [`Hint`](crate::Hint)
/// variant such as `Path`, when its type does not say
pub struct hint;

/// field: at most one member of the named group may be given
pub struct group;

/// field: cannot be given together with these fields, as `a` or `[a, b]`
pub struct conflicts_with;

/// field: giving this obliges these fields to be set too, as `a` or `[a, b]`
pub struct requires;

/// field: the short description, in place of the doc comment
pub struct help;

/// field: what `--help` shows in place of the short description
pub struct long_help;

/// field: the placeholder for the value in usage and errors
pub struct value_name;

/// field: list the arg under a help section of this name
pub struct heading;

/// field or subcommand: list it at this position in help
pub struct display_order;

/// field, subcommand or value: accepted but left out of help
pub struct hidden;

/// field: leave the `[possible values: ...]` tag out of help
pub struct hide_possible;

/// field: leave the `[default: ...]` tag out of help
pub struct hide_default;

/// field: leave the `[env: ...]` tag out of help
pub struct hide_env;

/// value enum: match spellings regardless of case
pub struct ignore_case;
