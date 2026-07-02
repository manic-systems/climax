// SPDX-License-Identifier: EUPL-1.2

#![cfg_attr(not(feature = "std"), no_std)]

//! pound: a low footprint, derive-first cli parser.
//!
//!
//! | type        | meaning              |
//! |-------------|----------------------|
//! | `bool`      | flag                 |
//! | `T`         | required positional  |
//! | `Option<T>` | optional positional  |
//! | `Vec<T>`    | variadic/repeatable  |
//!
//! `#[pound(short)]` / `#[pound(long)]` promote these to a named option
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

mod error;
mod help;
mod parse;
pub mod spec;
mod value;

pub use error::Error;
pub use parse::Matches;
#[cfg(feature = "derive")]
pub use pound_derive::{
    Parse,
    ValueEnum,
};
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

/// the trait the derive targets
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
    fn from_matches(spec: &'static CommandSpec, matches: &Matches<'_>) -> Result<Self, Error>;

    /// parse the given args, returning the typed value or an [`Error`].
    ///
    /// args are borrowed (`&str`), so matched values point straight into them;
    /// the iterator's items must outlive the call (which is fine, `Self` owns
    /// any field it keeps). this is the `no_std` entry point.
    fn try_parse_from<'a, I>(args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let matches = parse::parse_spec(Self::SPEC, args)?;
        Self::from_matches(Self::SPEC, &matches)
    }

    /// parse `std::env::args()` minus the program name.
    #[cfg(feature = "std")]
    fn try_parse() -> Result<Self, Error> {
        // env yields owned `String`s; hold them in a buffer the borrowed parse
        // reads from, then return the owned `Self`.
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::try_parse_from(args.iter().map(String::as_str))
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

/// build a borrowed argument iterator from a raw libc `main(argc, argv)`.
///
/// for a `#![no_std]` program that owns its entry point: pairs with
/// [`Parse::try_parse_from`] once you `skip(1)` the program name.
///
/// the yielded `&str`s borrow directly from `argv` — no allocation — and any
/// non-UTF-8 argument is skipped. pound cannot *source* argv portably (that is
/// the OS boundary `std` exists to cross), but it can bridge the pointers you
/// already hold.
///
/// ```no_run
/// use core::ffi::{
///     c_char,
///     c_int,
/// };
///
/// // call this from your `#![no_main]` libc entry point, forwarding its args:
/// fn run(argc: c_int, argv: *const *const c_char) {
///     // SAFETY: argc/argv are the unmodified parameters libc passed `main`.
///     let args = unsafe { pound::args_from_raw(argc, argv) }.skip(1);
///     // let cmd = MyCommand::try_parse_from(args)?;
///     let _ = args.count();
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
) -> impl Iterator<Item = &'a str> {
    let count = usize::try_from(argc).unwrap_or(0); // a negative argc reads as empty
    (0..count).filter_map(move |i| {
        // SAFETY: `i < count <= argc`, so `argv.add(i)` is in bounds and points
        // at a valid C string per the documented contract.
        let ptr = unsafe { *argv.add(i) };
        if ptr.is_null() {
            return None;
        }
        unsafe { core::ffi::CStr::from_ptr(ptr) }.to_str().ok()
    })
}
