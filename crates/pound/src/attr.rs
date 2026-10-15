// SPDX-License-Identifier: EUPL-1.2

//! every key `#[pound(...)]` takes
//!
//! the derives refer to each key written, so rust-analyzer completes the keys
//! and shows these docs on hover, and rustc rejects a misspelled one with the
//! closest match. the full rules live on [`Parse`](crate::Parse).

#![expect(
    non_camel_case_types,
    reason = "each struct is spelled as the key it documents"
)]

/// the name a command, subcommand or value is spelled as
pub struct name;

/// the text `--version` prints
pub struct version;

/// the fuller text `--version` prints, while `-V` keeps `version`
pub struct long_version;

/// exactly one member of a group must be given
pub struct required_group;

/// print help to stderr and exit 2 when given no words
pub struct help_if_empty;

/// text the help page opens with, above the description
pub struct before_help;

/// text the help page closes with, such as examples
pub struct after_help;

/// a `--name` flag or option
pub struct long;

/// a `-n` flag or option
pub struct short;

/// a positional argument
pub struct positional;

/// collect the rest of the line, flags too
pub struct trailing;

/// collect everything after `--`, which a bare word never reaches
pub struct last;

/// count repeats of a flag into an integer
pub struct count;

/// the field holding the subcommand tree
pub struct subcommand;

/// embed another `Parse` struct's args at this level
pub struct flatten;

/// take any other subcommand name and what follows
pub struct external;

/// the only arg allowed when given
pub struct exclusive;

/// offer a flag or option to every subcommand below this command too
pub struct global;

/// let values start with `-`
pub struct allow_hyphen_values;

/// extra spellings that also match, never listed in help
pub struct alias;

/// extra short names for a flag or option
pub struct short_alias;

/// the value when the arg is not given
pub struct default;

/// the value of an option written with no value
pub struct default_missing;

/// read this environment variable when the arg is not given
pub struct env;

/// also accept `--no-<long>` to switch a flag off
pub struct negate;

/// read the raw `&str` with this function in place of `FromArg`
pub struct parse;

/// check the parsed value with this function
pub struct validate;

/// the smallest value accepted
pub struct min;

/// the largest value accepted
pub struct max;

/// the most characters the raw text may have
pub struct max_len;

/// the fewest values a `Vec` must receive
pub struct min_values;

/// the most values a `Vec` may receive
pub struct max_values;

/// split each value of a `Vec` at this character
pub struct delimiter;

/// how many words one use of a `Vec` option takes
pub struct num_args;

/// list shell completion candidates with this function
pub struct complete;

/// what the value names, for completion
pub struct hint;

/// at most one member of this group may be given
pub struct group;

/// fields that cannot be given together with this one
pub struct conflicts_with;

/// fields that must be set whenever this one is given
pub struct requires;

/// the short description, in place of the doc comment
pub struct help;

/// what `--help` shows in place of the short description
pub struct long_help;

/// the placeholder for the value in usage and errors
pub struct value_name;

/// list the arg under a help section of this name
pub struct heading;

/// the position this arg or subcommand is listed at in help
pub struct display_order;

/// accepted, but left out of help
pub struct hidden;

/// leave the `[possible values: ...]` tag out of help
pub struct hide_possible;

/// leave the `[default: ...]` tag out of help
pub struct hide_default;

/// leave the `[env: ...]` tag out of help
pub struct hide_env;

/// match a value enum's spellings in any case
pub struct ignore_case;
