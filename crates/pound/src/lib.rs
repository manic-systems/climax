// SPDX-License-Identifier: EUPL-1.2

#![cfg_attr(not(feature = "std"), no_std)]

//! pound: a low footprint, derive-first cli parser.
//!
//! a struct deriving [`Parse`] is one command, an enum is a subcommand tree,
//! and a field's type says what it is.
//!
//! | type        | meaning              |
//! |-------------|----------------------|
//! | `bool`      | flag                 |
//! | `T`         | required positional  |
//! | `Option<T>` | optional positional  |
//! | `Vec<T>`    | variadic/repeatable  |
//!
//! `#[pound(short)]` / `#[pound(long)]` promote these to a named option, and
//! `#[pound(flatten)]` embeds another `Parse` struct's args at the same level.
//!
//! ```
//! # #[cfg(feature = "derive")]
//! # {
//! use pound::Parse;
//!
//! #[derive(Parse)]
//! struct Common {
//!     #[pound(long)]
//!     verbose: bool,
//! }
//!
//! #[derive(Parse)]
//! struct Add {
//!     #[pound(flatten)]
//!     common:  Common,
//!     name:    String, // required positional
//!     url:     String, // required positional
//!     #[pound(long)]
//!     unpack:  Option<String>,
//!     #[pound(long)]
//!     follows: Vec<String>, // repeatable --follows
//!     #[pound(short, long)]
//!     force:   bool, // -f / --force
//! }
//!
//! // `Add::parse()` reads argv and exits on -h/--help or a parse error
//! let add = Add::try_parse_from(["--verbose", "-f", "tool", "https://example.org"]).unwrap();
//! assert!(add.common.verbose && add.force);
//! assert_eq!(add.name, "tool");
//! # }
//! ```
//!
//! `parse` and `validate` take a path or any callable expression, and `default`
//! takes a string literal, a bare word such as `auto`, or a braced constant of
//! type `&str`.
//!
//! ```
//! # #[cfg(feature = "derive")]
//! # {
//! use std::num::NonZeroUsize;
//!
//! use pound::Parse;
//!
//! const SYSTEM_PROFILE: &str = "system";
//!
//! fn level(s: &str) -> Result<u8, std::num::ParseIntError> {
//!     s.parse()
//! }
//!
//! #[derive(Parse)]
//! struct Run {
//!     #[pound(long, parse = str::parse::<NonZeroUsize>)]
//!     jobs:    NonZeroUsize,
//!     #[pound(long, parse = level, validate = |n: &u8| if *n > 0 { Ok(()) } else { Err("zero") })]
//!     level:   u8,
//!     #[pound(long, default = { SYSTEM_PROFILE })]
//!     profile: String,
//! }
//!
//! let run = Run::try_parse_from(["--jobs", "4", "--level", "2"]).unwrap();
//! assert_eq!(run.profile, "system");
//! # }
//! ```
//!
//! every attribute, the field type mapping and the `default`, `env`, group and
//! conflict forms are documented on the derive macros, see [`derive@Parse`] and
//! [`derive@ValueEnum`].
//!
//! you may also hand-build a [`spec::CommandSpec`] and impl [`Parse`] yourself.

extern crate alloc;

// alloc machinery for nostd
mod alloc_prelude {
    #[cfg(not(feature = "std"))]
    pub(crate) use alloc::{
        borrow::ToOwned,
        boxed::Box,
        format,
        string::{
            String,
            ToString,
        },
        vec,
        vec::Vec,
    };
}

pub mod attr;
#[doc(hidden)] pub mod checks;
pub mod complete;
mod error;
mod help;
mod parse;
pub mod spec;
mod term;
mod value;

pub use error::{
    Error,
    ErrorKind,
};
pub use help::{
    Help,
    Usage,
};
pub use parse::{
    External,
    Matches,
};
#[cfg(feature = "derive")]
pub use pound_derive::{
    Parse,
    ValueEnum,
};
pub use spec::{
    ArgSpec,
    ArgumentOrder,
    Arity,
    Command,
    CommandSpec,
    GroupSpec,
    Hint,
    Kind,
    SubSpec,
};
pub use term::Term;
pub use value::{
    ArgValue,
    Choice,
    FromArg,
    Source,
    Sourced,
    ValueError,
};

/// the trait the derive targets
///
/// a type carries its static [`CommandSpec`] and reads itself out of
/// [`Matches`]. `parse` is the common "parse argv or exit" path, the
/// `try_*` variants hand back the [`Error`] (including the [`ErrorKind::Help`]
/// / [`ErrorKind::Version`] signals).
pub trait Parse: Sized {
    /// this command's static description.
    const SPEC: &'static CommandSpec;

    /// build `Self` from matches against `spec`. `spec` is passed in (not read
    /// from [`Self::SPEC`]) so the same method works for a subcommand reading
    /// against its own spec.
    fn from_matches(spec: &'static CommandSpec, matches: &Matches<'_>) -> Result<Self, Error>;

    /// overwrite the fields `matches` gave a value on the line, leaving the
    /// rest. a subcommand named again is updated, and a different one
    /// replaces it. setting one member of a group or conflict clears the
    /// others in the same struct. on an error `self` may be partly written.
    fn update_from_matches(
        &mut self,
        spec: &'static CommandSpec,
        matches: &Matches<'_>,
    ) -> Result<(), Error>;

    /// parse the given args as more words for `self`, overwriting only what
    /// they give. nothing they leave out is required, and fallbacks are not
    /// applied again, though a newly chosen subcommand is checked in full.
    /// `self` is left as it was when this fails.
    fn try_update_from<'a, I>(&mut self, args: I) -> Result<(), Error>
    where
        I: IntoIterator<Item = &'a str>,
        Self: Clone,
    {
        let matches = parse::update_spec(Self::SPEC, args)?;
        let mut next = self.clone();
        next.update_from_matches(Self::SPEC, &matches)
            .map_err(|e| matches.locate(Self::SPEC, e))?;
        *self = next;
        Ok(())
    }

    /// [`Parse::try_update_from`], printing help/version or errors and exiting
    #[cfg(feature = "std")]
    fn update_from<'a, I>(&mut self, args: I)
    where
        I: IntoIterator<Item = &'a str>,
        Self: Clone,
    {
        if let Err(err) = self.try_update_from(args) {
            err.exit();
        }
    }

    /// parse the given args, returning the typed value or an [`Error`].
    ///
    /// args are borrowed (`&str`), so matched values point straight into them;
    /// the iterator's items must outlive the call (which is fine, `Self` owns
    /// any field it keeps). this is the `no_std` entry point.
    fn try_parse_from<'a, I>(args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = &'a str>,
    {
        parse_as(Self::SPEC, args)
    }

    /// parse [`std::env::args_os`], naming the program after its file when the
    /// spec asks for [`CommandSpec::invoked_name`].
    #[cfg(feature = "std")]
    fn try_parse() -> Result<Self, Error> {
        static INVOKED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

        let mut argv = std::env::args_os();
        let program = argv.next();
        let invoked = INVOKED.get_or_init(|| {
            let path = std::path::Path::new(program.as_deref()?);
            Some(path.file_name()?.to_str()?.to_owned())
        });
        let spec = match invoked {
            Some(name) if Self::SPEC.invoked_name => {
                CommandSpec {
                    name,
                    ..*Self::SPEC
                }
            },
            _ => *Self::SPEC,
        };

        let args = argv
            .map(|arg| {
                arg.into_string().map_err(|bad| {
                    let kind = ErrorKind::InvalidUtf8(bad.to_string_lossy().into_owned());
                    Command::root(&spec).locate(Error::from(kind))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        parse_as(&spec, args.iter().map(String::as_str))
    }

    /// parse argv, printing help/version or errors and exiting.
    #[cfg(feature = "std")]
    #[must_use]
    fn parse() -> Self {
        match Self::try_parse() {
            Ok(value) => value,
            Err(err) => err.exit(),
        }
    }

    /// parse the given args, printing help/version or errors and exiting.
    #[cfg(feature = "std")]
    #[must_use]
    fn parse_from<'a, I>(args: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        match Self::try_parse_from(args) {
            Ok(value) => value,
            Err(err) => err.exit(),
        }
    }
}

/// a [`Parse`] enum whose variants are commands, which is what a
/// `#[pound(subcommand)]` field or a `#[pound(flatten)]` variant must hold.
/// the derive implements it for every enum.
pub trait Subcommands: Parse {}

fn parse_as<'a, T: Parse>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<T, Error> {
    let matches = parse::parse_spec(spec, args)?;
    T::from_matches(T::SPEC, &matches).map_err(|e| matches.locate(spec, e))
}

/// build a borrowed argument iterator from a raw libc `main(argc, argv)`.
///
/// for a `#![no_std]` program that owns its entry point: pairs with
/// [`Parse::try_parse_from`] once you `skip(1)` the program name.
///
/// the yielded `&str`s borrow directly from `argv` — no allocation. a
/// non-UTF-8 argument is an [`ErrorKind::InvalidUtf8`] rather than a gap that
/// would shift every positional after it. pound cannot *source* argv portably
/// (that is the OS boundary `std` exists to cross), but it can bridge the
/// pointers you already hold.
///
/// ```no_run
/// use core::ffi::{
///     c_char,
///     c_int,
/// };
///
/// // call this from your `#![no_main]` libc entry point, forwarding its args:
/// fn run(argc: c_int, argv: *const *const c_char) -> Result<(), pound::Error> {
///     // SAFETY: argc/argv are the unmodified parameters libc passed `main`.
///     let args = unsafe { pound::args_from_raw(argc, argv) }?.skip(1);
///     // let cmd = MyCommand::try_parse_from(args)?;
///     let _ = args.count();
///     Ok(())
/// }
/// ```
///
/// # Safety
///
/// `argv` must point to `argc` consecutive, valid, NUL-terminated C strings
/// that stay alive and immutable for `'a` — exactly the contract a libc runtime
/// upholds for `main`'s parameters. A negative `argc` is treated as zero.
// argc/argv are the libc contract names; keeping them reads clearer than any
// rename clippy's `similar_names` would prefer.
#[allow(clippy::similar_names)]
pub unsafe fn args_from_raw<'a>(
    argc: core::ffi::c_int,
    argv: *const *const core::ffi::c_char,
) -> Result<impl Iterator<Item = &'a str>, Error> {
    let count = usize::try_from(argc).unwrap_or(0); // a negative argc reads as empty
    let args = (0..count).filter_map(move |i| {
        // SAFETY: `i < count <= argc`, so `argv.add(i)` is in bounds and points
        // at a valid C string per the documented contract.
        let ptr = unsafe { *argv.add(i) };
        (!ptr.is_null()).then(|| unsafe { core::ffi::CStr::from_ptr::<'a>(ptr) })
    });

    if let Some(bad) = args.clone().find(|arg| arg.to_str().is_err()) {
        return Err(ErrorKind::InvalidUtf8(bad.to_string_lossy().into_owned()).into());
    }
    // SAFETY: every argument was checked above, and the contract keeps them
    // unchanged for `'a`.
    Ok(args.map(|arg| unsafe { core::str::from_utf8_unchecked(arg.to_bytes()) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_args_reject_non_utf8() {
        let raw = [c"prog".as_ptr(), c"\xff".as_ptr(), c"next".as_ptr()];
        // SAFETY: three live, NUL-terminated C strings
        let parsed = unsafe { args_from_raw(3, raw.as_ptr()) };
        let Err(err) = parsed.map(Iterator::count) else {
            panic!("a non-UTF-8 argument was accepted");
        };
        assert!(matches!(err.kind, ErrorKind::InvalidUtf8(_)));
    }
}
