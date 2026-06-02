// SPDX-License-Identifier: EUPL-1.2

//! pound: a low footprint, derive-first cli parser.
//!
//! the derive emits a flat `&'static` [`spec::CommandSpec`] and one non-generic
//! engine interprets it, so derives stay ergonomic while adding almost nothing
//! to the binary and nothing at runtime.
//!
//! field shapes carry meaning, so most fields need no attribute:
//!
//! | shape       | meaning              |
//! |-------------|----------------------|
//! | `bool`      | flag, presence is true |
//! | `T`         | required positional  |
//! | `Option<T>` | optional positional  |
//! | `Vec<T>`    | variadic/repeatable  |
//!
//! `#[pound(short)]` / `#[pound(long)]` promote any of these to a named option.
//! the annotated thing is the switch, values stay bare.
//!
//! ```ignore
//! use pound::Parse;
//!
//! #[derive(Parse)]
//! struct Add {
//!     name: String,                          // required positional
//!     url:  String,                          // required positional
//!     #[pound(long)] unpack:  Option<String>,
//!     #[pound(long)] follows: Vec<String>,   // repeatable --follows
//!     #[pound(short, long)] force: bool,     // -f / --force
//! }
//!
//! let add = Add::parse(); // exits on -h/--help or a parse error
//! ```
//!
//! you can also hand-build a [`spec::CommandSpec`] and impl [`Parse`] yourself,
//! as the test suite does.

mod error;
mod help;
mod parse;
pub mod spec;
mod value;

pub use error::Error;
pub use parse::Matches;
pub use spec::{
    ArgSpec,
    CommandSpec,
    GroupSpec,
    Kind,
    SubSpec,
};
pub use value::{
    FromArg,
    ValueError,
};

// the derive macros share names with the `Parse` trait and `FromArg`, which is
// fine: macros and types live in separate namespaces (same trick serde uses).
#[cfg(feature = "derive")]
pub use pound_derive::{
    Parse,
    ValueEnum,
};

/// the trait the derive targets, also implementable by hand.
///
/// a type carries its static [`CommandSpec`] and reads itself out of
/// [`Matches`]. [`Self::parse`] is the common "parse argv or exit" path, the
/// `try_*` variants hand back the [`Error`] (including the [`Error::Help`] /
/// [`Error::Version`] signals).
pub trait Parse: Sized {
    /// this command's static description.
    const SPEC: &'static CommandSpec;

    /// build `Self` from matches against `spec`. `spec` is passed in (not read
    /// from [`Self::SPEC`]) so the same method works for a subcommand reading
    /// against its own spec.
    fn from_matches(spec: &'static CommandSpec, matches: &Matches) -> Result<Self, Error>;

    /// parse the given args, returning the typed value or an [`Error`].
    fn try_parse_from<I>(args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = String>,
    {
        let mut matches = parse::parse_spec(Self::SPEC, args)?;
        parse::apply_defaults(Self::SPEC, &mut matches);
        Self::from_matches(Self::SPEC, &matches)
    }

    /// parse `std::env::args()` minus the program name.
    fn try_parse() -> Result<Self, Error> {
        Self::try_parse_from(std::env::args().skip(1))
    }

    /// parse argv, printing help/version or errors and exiting.
    #[must_use]
    fn parse() -> Self {
        match Self::try_parse() {
            Ok(value) => value,
            Err(err) => err.exit(),
        }
    }

    /// parse the given args, printing help/version or errors and exiting.
    #[must_use]
    fn parse_from<I>(args: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        match Self::try_parse_from(args) {
            Ok(value) => value,
            Err(err) => err.exit(),
        }
    }
}
