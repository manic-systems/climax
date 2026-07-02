// SPDX-License-Identifier: EUPL-1.2

//! turning a raw `&str` into a typed value
//!
//! no blanket impl over `FromStr`: that would block a bespoke [`FromArg`] for
//! any type that already has `FromStr` (uuids, ip addrs). instead std scalars
//! are wired up here, [`from_str!`] opts a `FromStr` type in with one line, and
//! you hand-write [`FromArg`] for anything exotic (hex colours, durations)
//! without a coherence fight.

use core::fmt;

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;

/// a value that would not parse, plus context for the message. the parser wraps
/// it into [`crate::Error::Value`] once it knows which arg it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueError {
    pub value: String,
    pub msg: String,
}

impl ValueError {
    pub fn new(value: &str, msg: impl fmt::Display) -> Self {
        Self {
            value: value.to_owned(),
            msg: msg.to_string(),
        }
    }
}

/// parse a single token into `Self`. impl it for your own field types:
///
/// ```
/// use pound::{
///     FromArg,
///     ValueError,
/// };
///
/// struct Rgb(u8, u8, u8);
///
/// impl FromArg for Rgb {
///     fn from_arg(s: &str) -> Result<Self, ValueError> {
///         let s = s.strip_prefix('#').unwrap_or(s);
///         if s.len() != 6 {
///             return Err(ValueError::new(s, "expected a 6-digit hex colour"));
///         }
///         let byte =
///             |i: usize| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| ValueError::new(s, e));
///         Ok(Rgb(byte(0)?, byte(2)?, byte(4)?))
///     }
/// }
/// ```
pub trait FromArg: Sized {
    /// the closed set of accepted values, if any. powers choice listings in
    /// help and value errors. set by the `ValueEnum` derive, `None` otherwise.
    /// being a const lets the `Parse` derive wire it into a spec at compile
    /// time.
    const POSSIBLE: Option<&'static [&'static str]> = None;

    /// attempt the conversion.
    fn from_arg(s: &str) -> Result<Self, ValueError>;

    /// runtime view of [`Self::POSSIBLE`].
    #[must_use]
    fn possible_values() -> Option<&'static [&'static str]> {
        Self::POSSIBLE
    }
}

/// impl [`FromArg`] for one or more types via their [`FromStr`].
///
/// ```
/// # struct Uuid;
/// # impl std::str::FromStr for Uuid {
/// #     type Err = std::convert::Infallible;
/// #     fn from_str(_: &str) -> Result<Self, Self::Err> { Ok(Uuid) }
/// # }
/// pound::from_str!(Uuid);
/// ```
///
/// [`FromStr`]: std::str::FromStr
#[macro_export]
macro_rules! from_str {
    ($($t:ty),+ $(,)?) => {$(
        impl $crate::FromArg for $t {
            fn from_arg(s: &str) -> ::core::result::Result<Self, $crate::ValueError> {
                <$t as ::core::str::FromStr>::from_str(s)
                    .map_err(|e| $crate::ValueError::new(s, e))
            }
        }
    )+};
}

from_str! {
    String,
    char,
    bool,
    i8, i16, i32, i64, i128, isize,
    u8, u16, u32, u64, u128, usize,
    f32, f64,
    core::net::IpAddr,
    core::net::Ipv4Addr,
    core::net::Ipv6Addr,
    core::net::SocketAddr,
}

// `PathBuf` lives in `std` (it wraps `OsString`), so its value impl is the one
// scalar that cannot ride along in a `no_std` build.
#[cfg(feature = "std")]
from_str! {
    std::path::PathBuf,
}
