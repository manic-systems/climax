// SPDX-License-Identifier: EUPL-1.2

//! derive macros for pound. `#[derive(Parse)]` turns a struct into a flat
//! command and an enum into a subcommand tree, `#[derive(ValueEnum)]` wires a
//! unit enum up as a `FromArg` choice type. the expansion lives in
//! `pound-derive-impl`, rooted here at `::pound`.
//!
//! the attribute reference is on the macros themselves, see [`Parse`] and
//! [`ValueEnum`].

use pound_derive_impl::Options;
use proc_macro::TokenStream;

const OPTIONS: Options = Options::new("::pound", cfg!(feature = "help"));

/// derives `Parse` for a struct (one command) or an enum (a subcommand tree).
///
/// every example below is a compiled doctest, and they use `try_parse_from` so
/// they can assert on the result. a real program calls `parse()` instead, which
/// reads `std::env::args()` and exits on `-h`, `--version` or a parse error.
///
/// # field types
///
/// a field's type decides what it is, and `#[pound(...)]` refines it.
///
/// | field                                  | meaning                                                  |
/// |----------------------------------------|----------------------------------------------------------|
/// | `bool`                                 | flag, true when present. `--name` is inferred            |
/// | `T`                                    | required positional                                      |
/// | `Option<T>`                            | optional positional                                      |
/// | `Vec<T>`                               | variadic positional                                      |
/// | `T` with `short` or `long`             | option that must be given or have a `default`            |
/// | `Option<T>` with `short` or `long`     | optional option                                          |
/// | `Vec<T>` with `short` or `long`        | option that repeats, `--name a --name b`                 |
/// | integer with `count`                   | `-vvv` or `--verbose --verbose` counted                  |
/// | `Vec<T>` with `trailing`               | everything after `--`                                    |
/// | `T` or `Option<T>` with `subcommand`   | the subcommand tree held by a [`Parse`] enum             |
/// | `T` with `flatten`                     | another [`Parse`] struct's args, embedded at this level  |
///
/// `T` is anything that implements `pound::FromArg`, which covers the
/// primitives, `String`, `PathBuf`, the `NonZero` integers and every
/// `#[derive(ValueEnum)]` type. `parse` swaps in any other conversion.
/// a field with a `default` is never required. a `bool` is a flag whatever
/// attributes it carries, so it needs neither `short` nor `long`.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// struct Add {
///     name: String,                          // required positional
///     url: Option<String>,                   // optional positional
///     #[pound(long)] unpack: Option<String>,
///     #[pound(long)] follows: Vec<String>,
///     #[pound(short, long)] force: bool,
///     #[pound(short, count)] verbose: u8,    // -v, -vv, -vvv
///     #[pound(trailing)] rest: Vec<String>,  // everything after `--`
/// }
///
/// let add = Add::try_parse_from([
///     "-f", "-vv", "tool", "u", "--follows", "a", "--follows", "b", "--", "x", "y",
/// ])
/// .unwrap();
/// assert_eq!(add.name, "tool");
/// assert_eq!(add.url.as_deref(), Some("u"));
/// assert_eq!(add.follows, ["a", "b"]);
/// assert!(add.force);
/// assert_eq!(add.verbose, 2);
/// assert_eq!(add.rest, ["x", "y"]);
/// ```
///
/// # item attributes
///
/// these go on the struct or enum that derives `Parse`.
///
/// | attribute                | meaning                                                                                  |
/// |--------------------------|------------------------------------------------------------------------------------------|
/// | `name = "tool"`          | the command name shown in usage and help. defaults to `CARGO_PKG_NAME`                   |
/// | `version = EXPR`         | the `--version` text. any expression of type `&'static str`, defaults to `CARGO_PKG_VERSION` |
/// | `required_group = "g"`   | exactly one member of group `g` must be given. may repeat, see [groups](#groups)         |
///
/// the doc comment on the item is the command's description. its first
/// paragraph is the summary that `-h` shows, with one trailing period dropped,
/// and the whole comment is what `--help` shows.
///
/// ```
/// use pound::Parse;
///
/// const BUILD: &str = "2.0-rc1";
///
/// /// ships the thing.
/// #[derive(Parse)]
/// #[pound(name = "shipit", version = BUILD, required_group = "target")]
/// struct Ship {
///     #[pound(long, group = "target")] staging: bool,
///     #[pound(long, group = "target")] production: bool,
/// }
///
/// assert!(Ship::try_parse_from(["--staging"]).is_ok());
/// assert!(Ship::try_parse_from([]).is_err());
/// ```
///
/// # variant attributes
///
/// these go on the variants of an enum that derives `Parse`. each variant is a
/// subcommand whose fields take the field attributes below.
///
/// | attribute                | meaning                                                                         |
/// |--------------------------|---------------------------------------------------------------------------------|
/// | `name = "rm"`            | the subcommand name. defaults to the variant name in kebab case                 |
/// | `alias = "remove, del"`  | extra names that select this subcommand, comma separated                        |
/// | `hidden`                 | accepted but left out of help                                                   |
/// | `required_group = "g"`   | exactly one member of group `g` among the variant's fields must be given        |
/// | `flatten`                | on a tuple variant holding one enum, offers that enum's commands as if declared here |
///
/// the doc comment on a variant is the subcommand's description. a variant is
/// a unit variant or has named fields, and tuple fields are only for `flatten`.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// enum Cmd {
///     /// adds a remote.
///     Add { name: String },
///     #[pound(name = "rm", alias = "remove, del")]
///     RemoveRemote { name: String },
///     #[pound(hidden)]
///     Debug,
/// }
///
/// assert!(matches!(Cmd::try_parse_from(["add", "x"]), Ok(Cmd::Add { .. })));
/// assert!(matches!(Cmd::try_parse_from(["del", "x"]), Ok(Cmd::RemoveRemote { .. })));
/// assert!(matches!(Cmd::try_parse_from(["debug"]), Ok(Cmd::Debug)));
/// ```
///
/// # field attributes
///
/// ## naming and shape
///
/// | attribute            | meaning                                                                                        |
/// |----------------------|------------------------------------------------------------------------------------------------|
/// | `long`               | `--name`, taken from the field name with `_` as `-`. `long = "other"` picks the spelling       |
/// | `short`              | `-n`, the field name's first letter. `short = 'x'` picks the letter                            |
/// | `positional`         | a positional, which is already what a field with neither `short` nor `long` is                 |
/// | `trailing`           | collect everything after `--` into a `Vec`. it must be the last positional                     |
/// | `count`              | count repeats of a flag into an integer. it takes no `default` or `env`                        |
/// | `alias = "a, b"`     | extra long names for a flag or option, comma separated                                         |
/// | `subcommand`         | the field holds the command's subcommand tree. use `Option<T>` when one is not required        |
/// | `flatten`            | embed another `Parse` struct's args at this level. it takes no other attribute                 |
///
/// `positional`, `trailing` and `count` exclude each other. a positional takes
/// no names or aliases, `trailing` needs a `Vec`, and `count` needs a scalar.
/// a positional or subcommand may not follow a variadic or trailing positional,
/// and the derive rejects that at compile time.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// struct Common {
///     #[pound(short, long)] verbose: bool,
/// }
///
/// #[derive(Parse)]
/// struct Run {
///     #[pound(flatten)] common: Common,
///     #[pound(long = "jobs", short = 'j', alias = "threads")] workers: Option<u32>,
/// }
///
/// let run = Run::try_parse_from(["-v", "--threads", "4"]).unwrap();
/// assert!(run.common.verbose);
/// assert_eq!(run.workers, Some(4));
/// ```
///
/// ## defaults, env and missing values
///
/// | attribute                | meaning                                                                                  |
/// |--------------------------|------------------------------------------------------------------------------------------|
/// | `default = ...`          | the value used when the arg is not given, see the three forms below                      |
/// | `env = "VAR"`            | read `VAR` when the arg is not given. with the `std` feature only                        |
/// | `default_missing = "v"`  | the value of an option written with no value, so `--color` alone means `--color=v`       |
/// | `negate`                 | on a flag, also accept `--no-<long>` to switch it off. `negate = "off"` or `"--off"` names it |
///
/// a value on the command line beats `env`, which beats `default`. the
/// `default` takes three forms, and the value is read through `FromArg` like a
/// typed one.
///
/// - a string literal, `default = "auto"`
/// - a bare word, `default = auto`
/// - a braced expression of type `&str`, `default = { SYSTEM_PROFILE }`, for a
///   constant that is also used elsewhere
///
/// when the field is a `ValueEnum`, a `default` that is not one of its values
/// fails the build. an `env` or `default` value on a `bool` turns the flag on
/// for `1`, `true` or `yes`. `default_missing` needs an option and `negate`
/// needs a `bool` with a long name, and the negation may not equal the
/// positive spelling or one of its aliases.
///
/// a value that comes from `env` and fails to parse is reported with its
/// source, as in `invalid value '99' for --replicas (from $SHIPIT_REPLICAS)`.
///
/// ```
/// use pound::Parse;
///
/// const SYSTEM_PROFILE: &str = "system";
///
/// #[derive(Parse)]
/// struct Run {
///     #[pound(long, default = "auto")] mode: String,
///     #[pound(long, default = fast)] speed: String,
///     #[pound(long, default = { SYSTEM_PROFILE })] profile: String,
///     #[pound(long, env = "SHIPIT_POUND_DOC_REGION")] region: Option<String>,
///     #[pound(long, default_missing = "always")] color: Option<String>,
///     #[pound(long, negate, default = "true")] cache: bool,
/// }
///
/// let run = Run::try_parse_from(["--color", "--no-cache"]).unwrap();
/// assert_eq!(run.mode, "auto");
/// assert_eq!(run.speed, "fast");
/// assert_eq!(run.profile, "system");
/// assert_eq!(run.region, None);
/// assert_eq!(run.color.as_deref(), Some("always"));
/// assert!(!run.cache);
/// ```
///
/// ## conversion and checks
///
/// | attribute               | meaning                                                                         |
/// |-------------------------|---------------------------------------------------------------------------------|
/// | `parse = F`             | read the raw `&str` with `F` in place of `FromArg`. it returns `Result<T, E>` for any `E: Display` |
/// | `validate = F`          | check the parsed value with `F`. it takes `&T` and returns `Result<(), E>`      |
/// | `min = "n"`             | the parsed value must be at least `n`                                           |
/// | `max = "n"`             | the parsed value must be at most `n`                                            |
/// | `max_len = "n"`         | the raw text may have at most `n` characters                                    |
/// | `min_values = "n"`      | a `Vec` must receive at least `n` values, waived when a `default` or `env` fills it |
/// | `max_values = "n"`      | a `Vec` may receive at most `n` values                                          |
///
/// `parse` and `validate` take a path or any callable expression, such as a
/// closure or `str::parse::<NonZeroUsize>`. a string literal still names a
/// path, so `parse = "my_fn"` works as it always did. `min` and `max` bounds
/// are read through the field's own `FromArg`, so they suit numbers. they
/// cannot be combined with `parse`, where `validate` does the same job. the
/// checks apply to a value from `env` or `default` as well as to one typed on
/// the command line.
///
/// ```
/// use std::num::NonZeroUsize;
/// use pound::Parse;
///
/// fn hex(s: &str) -> Result<u32, std::num::ParseIntError> {
///     u32::from_str_radix(s, 16)
/// }
///
/// #[derive(Parse)]
/// struct Run {
///     #[pound(long, parse = str::parse::<NonZeroUsize>)] jobs: NonZeroUsize,
///     #[pound(long, parse = hex, validate = |n: &u32| if *n > 0 { Ok(()) } else { Err("zero") })]
///     mask: u32,
///     #[pound(long, min = "1", max = "10")] level: u8,
///     #[pound(long, max_len = "8")] tag: Option<String>,
///     #[pound(positional, min_values = "1", max_values = "3")] paths: Vec<String>,
/// }
///
/// let run = Run::try_parse_from(["--jobs", "4", "--mask", "ff", "--level", "3", "a"]).unwrap();
/// assert_eq!(run.mask, 255);
/// assert!(Run::try_parse_from(["--jobs", "0", "--mask", "1", "--level", "3", "a"]).is_err());
/// assert!(Run::try_parse_from(["--jobs", "1", "--mask", "0", "--level", "3", "a"]).is_err());
/// assert!(Run::try_parse_from(["--jobs", "1", "--mask", "1", "--level", "11", "a"]).is_err());
/// assert!(Run::try_parse_from(["--jobs", "1", "--mask", "1", "--level", "3"]).is_err());
/// ```
///
/// ## groups, conflicts and requirements
///
/// | attribute                  | meaning                                                                    |
/// |----------------------------|----------------------------------------------------------------------------|
/// | `group = "g"`              | at most one member of group `g` may be given                               |
/// | `conflicts_with = "a, b"`  | this field cannot be given together with the named fields                  |
/// | `requires = "a, b"`        | giving this field obliges the named fields to be set too                   |
///
/// ### groups
///
/// a group is the set of fields that name it, and any two of them given at once
/// is an error. `required_group` on the item or variant makes exactly one
/// member mandatory, and it fails the build when no field joins the group.
/// `conflicts_with` and `requires` take the Rust field names of the same
/// command, which may be separated by commas. they look at what was given on
/// the command line. `conflicts_with` is symmetric, so naming it on one side is
/// enough. `requires` is one way.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// #[pound(required_group = "mode")]
/// struct Run {
///     #[pound(long, group = "mode")] fetch: bool,
///     #[pound(long, group = "mode")] build: bool,
///     #[pound(long, conflicts_with = "quiet")] verbose: bool,
///     #[pound(long)] quiet: bool,
///     #[pound(long, requires = "user")] password: Option<String>,
///     #[pound(long)] user: Option<String>,
/// }
///
/// assert!(Run::try_parse_from(["--fetch"]).is_ok());
/// assert!(Run::try_parse_from([]).is_err());
/// assert!(Run::try_parse_from(["--fetch", "--build"]).is_err());
/// assert!(Run::try_parse_from(["--fetch", "--verbose", "--quiet"]).is_err());
/// assert!(Run::try_parse_from(["--fetch", "--password", "x"]).is_err());
/// ```
///
/// ## help text
///
/// | attribute            | meaning                                                                          |
/// |----------------------|----------------------------------------------------------------------------------|
/// | `help = "text"`      | the short description, used in place of the doc comment and kept as written      |
/// | `long_help = "text"` | what `--help` shows in place of the short description                            |
/// | `value_name = "N"`   | the placeholder for the value in usage and errors, defaulting to the field name  |
/// | `heading = "Output"` | list the arg under a help section of that name, where `Options` is the default   |
/// | `hidden`             | accept the arg but leave it out of help                                          |
///
/// without `help`, the doc comment on the field is the description. as on an
/// item, its first paragraph is the `-h` summary with one trailing period
/// dropped, and the whole comment is the `--help` text. `heading` takes effect
/// with the `help` feature and only suits a flag or option, since positionals
/// are always listed under `Arguments`.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// struct Run {
///     /// the output format.
///     ///
///     /// one of the formats the exporter knows about.
///     #[pound(long, value_name = "FMT", heading = "Output")]
///     format: Option<String>,
///     #[pound(long, help = "keep the cache", hidden)] keep: bool,
/// }
///
/// assert!(Run::try_parse_from(["--keep"]).is_ok());
/// ```
///
/// ## global arguments
///
/// `global` makes a flag or option that every subcommand below it accepts too,
/// so `tool --verbose sub` and `tool sub --verbose` mean the same. a
/// positional cannot be global. the value is read on the command that declares
/// it.
///
/// ```
/// use pound::Parse;
///
/// #[derive(Parse)]
/// struct Tool {
///     #[pound(long, global)] verbose: bool,
///     #[pound(subcommand)] cmd: Cmd,
/// }
///
/// #[derive(Parse)]
/// enum Cmd {
///     Build,
/// }
///
/// assert!(Tool::try_parse_from(["build", "--verbose"]).unwrap().verbose);
/// ```
///
/// # subcommands
///
/// a struct reaches an enum through a `#[pound(subcommand)]` field, shown
/// above. the field is `Option<T>` when the command may run without one. an
/// enum variant may carry a `subcommand` field of its own, which nests the
/// tree further. a command has at most one subcommand field, counting the ones
/// in its flattened structs.
///
/// # version and build hash
///
/// `--version` prints the crate version, followed by a build hash when the
/// `POUND_GIT_HASH` environment variable is set while the deriving crate
/// compiles. rustc tracks it, so changing it rebuilds the crate. nothing sets
/// it by default, so a build script supplies it.
///
/// ```no_run
/// // build.rs
/// fn main() {
///     let out = std::process::Command::new("git")
///         .args(["rev-parse", "--short=12", "HEAD"])
///         .output();
///     if let Ok(out) = out {
///         let hash = String::from_utf8_lossy(&out.stdout);
///         println!("cargo:rustc-env=POUND_GIT_HASH={}", hash.trim());
///     }
///     println!("cargo:rerun-if-changed=.git/HEAD");
/// }
/// ```
///
/// # compile time errors
///
/// the derive fails the build, naming the field, when an attribute does not
/// apply to where it sits. examples are `count` on a `Vec`, `negate` on a
/// non-`bool`, `default_missing` on a flag, `min_values` on a scalar, a
/// `conflicts_with` naming no field, two args that answer to the same
/// spelling, and an unknown attribute.
#[allow(clippy::needless_doctest_main, reason = "the example is a build script")]
#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

/// derives `FromArg` for an enum of unit variants, so the enum can be the type
/// of an option or positional that takes one of a fixed set of words.
///
/// each variant is spelled in kebab case on the command line, so `PlainText` is
/// `plain-text`. `#[pound(name = "yml")]` on a variant picks its spelling.
/// that is the only attribute accepted, and the enum itself takes none.
/// it also implements `pound::ArgValue`, which gives each value's spelling back
/// through `as_str` and lists them all in `ALL`, so `Display` can forward to it.
/// an unrecognized word fails with the full list of possible values, help lists
/// them, and a `default` naming something else fails the build.
///
/// ```
/// use pound::{Parse, ValueEnum};
///
/// #[derive(ValueEnum, Debug, PartialEq)]
/// enum Format {
///     Json,
///     #[pound(name = "yml")]
///     Yaml,
///     PlainText,
/// }
///
/// #[derive(Parse)]
/// struct Export {
///     #[pound(long, default = "json")] format: Format,
/// }
///
/// assert_eq!(Export::try_parse_from([]).unwrap().format, Format::Json);
/// assert_eq!(
///     Export::try_parse_from(["--format", "plain-text"]).unwrap().format,
///     Format::PlainText,
/// );
/// assert_eq!(Export::try_parse_from(["--format=yml"]).unwrap().format, Format::Yaml);
/// assert!(Export::try_parse_from(["--format", "xml"]).is_err());
/// ```
#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}
