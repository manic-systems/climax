// SPDX-License-Identifier: EUPL-1.2

//! turning a raw `&str` into a typed value
//!
//! no blanket impl over `FromStr`: that would block a bespoke [`FromArg`] for
//! any type that already has `FromStr` (uuids, ip addrs). instead std scalars
//! are wired up here, [`from_str!`] opts a `FromStr` type in with one line, and
//! you hand-write [`FromArg`] for anything exotic (hex colours, durations)
//! without a coherence fight.

use core::fmt;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;

/// a value that would not parse, plus context for the message. the parser wraps
/// it into [`crate::ErrorKind::Value`] once it knows which arg it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueError {
    /// the text that failed to parse
    pub value: String,
    /// why it failed
    pub msg:   String,
}

impl ValueError {
    /// a failure for `value`, with `msg` saying why
    pub fn new(value: &str, msg: impl fmt::Display) -> Self {
        Self {
            value: value.to_owned(),
            msg:   msg.to_string(),
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

/// the spellings of a choice type, for printing a value back out
///
/// `#[derive(ValueEnum)]` implements it with exactly the words [`FromArg`]
/// accepts, so renames and kebab case carry over and `from_arg(v.as_str())`
/// always gives `v` back. it is a trait and not inherent methods or a
/// generated `Display`, so the derive cannot collide with an impl you write
/// yourself. `ALL` lists the variants in declaration order.
///
/// forward `Display`, or a serde `Serialize`, to it so the printed form can
/// never drift from the parsed one.
///
/// ```
/// # #[cfg(feature = "derive")]
/// # {
/// use std::fmt;
///
/// use pound::{
///     ArgValue,
///     FromArg,
///     ValueEnum,
/// };
///
/// #[derive(ValueEnum, Debug, PartialEq)]
/// enum Format {
///     Json,
///     #[pound(name = "yml")]
///     Yaml,
///     PlainText,
/// }
///
/// impl fmt::Display for Format {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         f.write_str(self.as_str())
///     }
/// }
///
/// // with serde, `serializer.serialize_str(self.as_str())` in `Serialize`
/// // and `Format::from_arg` in `Deserialize` do the same job
///
/// assert_eq!(Format::PlainText.to_string(), "plain-text");
/// assert_eq!(Format::Yaml.as_str(), "yml");
/// let all: Vec<_> = Format::ALL.iter().map(Format::as_str).collect();
/// assert_eq!(all, ["json", "yml", "plain-text"]);
/// assert_eq!(Format::from_arg("yml").unwrap(), Format::Yaml);
/// # }
/// ```
pub trait ArgValue: FromArg + 'static {
    /// every value, in declaration order
    const ALL: &'static [Self];

    /// the word [`FromArg::from_arg`] accepts for this value
    fn as_str(&self) -> &'static str;
}

// `str` has no const `PartialEq`, so compare the bytes by hand.
pub(crate) const fn const_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
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
/// [`FromStr`]: core::str::FromStr
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
    core::num::NonZeroI8, core::num::NonZeroI16, core::num::NonZeroI32,
    core::num::NonZeroI64, core::num::NonZeroI128, core::num::NonZeroIsize,
    core::num::NonZeroU8, core::num::NonZeroU16, core::num::NonZeroU32,
    core::num::NonZeroU64, core::num::NonZeroU128, core::num::NonZeroUsize,
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

#[cfg(test)]
mod tests {
    use core::num::{
        NonZeroI8,
        NonZeroU16,
        NonZeroUsize,
    };

    use super::*;

    #[test]
    fn nonzero_integers_reject_zero() {
        assert_eq!(NonZeroUsize::from_arg("4").unwrap().get(), 4);
        assert_eq!(NonZeroI8::from_arg("-3").unwrap().get(), -3);
        assert!(NonZeroU16::from_arg("0").is_err());
        assert!(NonZeroU16::from_arg("x").is_err());
    }
}
