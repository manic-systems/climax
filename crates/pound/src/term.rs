// SPDX-License-Identifier: EUPL-1.2

//! the terminal a report is laid out for, and how its spans are painted

#[cfg(feature = "std")] use std::{
    env,
    io,
};

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;

/// the width and color a help page or error is laid out for
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Term {
    width: Option<usize>,
    color: bool,
}

impl Term {
    /// no wrapping and no color, for a pipe, a file, or a comparison
    #[must_use]
    pub const fn plain() -> Self {
        Self {
            width: None,
            color: false,
        }
    }

    /// wrap at `width` columns when given, and paint with ANSI escapes when
    /// `color` is set
    #[must_use]
    pub const fn new(width: Option<usize>, color: bool) -> Self {
        Self { width, color }
    }

    /// what standard output is, plain unless it is a terminal or color is
    /// forced
    #[cfg(feature = "std")]
    #[must_use]
    pub fn stdout() -> Self {
        use io::IsTerminal as _;

        Self::detect(io::stdout().is_terminal(), 1)
    }

    /// what standard error is, plain unless it is a terminal or color is
    /// forced
    #[cfg(feature = "std")]
    #[must_use]
    pub fn stderr() -> Self {
        use io::IsTerminal as _;

        Self::detect(io::stderr().is_terminal(), 2)
    }

    #[cfg(feature = "std")]
    fn detect(tty: bool, fd: i32) -> Self {
        let color = Self::color(tty);
        if !tty {
            return Self { width: None, color };
        }

        let width =
            sys::columns(fd).or_else(|| env::var("COLUMNS").ok()?.parse().ok().filter(|&w| w > 0));
        Self { width, color }
    }

    /// `NO_COLOR` outranks `CLICOLOR_FORCE`, which paints a pipe too
    #[cfg(feature = "std")]
    fn color(tty: bool) -> bool {
        let var = |name| env::var_os(name).filter(|value| !value.is_empty());
        if var("NO_COLOR").is_some() {
            return false;
        }
        if var("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
            return true;
        }
        tty && var("TERM").is_none_or(|value| value != "dumb")
            && var("CLICOLOR").is_none_or(|value| value != "0")
    }

    pub(crate) const fn width(self) -> Option<usize> {
        self.width
    }

    pub(crate) fn columns(text: &str) -> usize {
        #[cfg(feature = "unicode")]
        return unicode_width::UnicodeWidthStr::width(text);
        #[cfg(not(feature = "unicode"))]
        return text.chars().count();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paint {
    Plain,
    Heading,
    /// text typed as is, a flag spelling or a command name
    Literal,
    Error,
    Tip,
}

impl Paint {
    const fn escape(self) -> Option<&'static str> {
        match self {
            Self::Plain => None,
            Self::Heading => Some("\x1b[1;4m"),
            Self::Literal => Some("\x1b[1m"),
            Self::Error => Some("\x1b[1;31m"),
            Self::Tip => Some("\x1b[1;32m"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    text:  String,
    paint: Paint,
}

impl Span {
    pub(crate) fn new(text: impl Into<String>, paint: Paint) -> Self {
        Self {
            text: text.into(),
            paint,
        }
    }

    #[cfg_attr(
        not(feature = "help"),
        expect(dead_code, reason = "only help rows have plain spans")
    )]
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Paint::Plain)
    }

    pub(crate) fn literal(text: impl Into<String>) -> Self {
        Self::new(text, Paint::Literal)
    }

    pub(crate) fn width(&self) -> usize {
        Term::columns(&self.text)
    }

    pub(crate) fn write(&self, out: &mut String, term: Term) {
        match self.paint.escape().filter(|_| term.color) {
            Some(escape) => {
                out.push_str(escape);
                out.push_str(&self.text);
                out.push_str("\x1b[0m");
            },
            None => out.push_str(&self.text),
        }
    }
}

#[cfg(all(feature = "std", unix))]
mod sys {
    use core::ffi::{
        c_int,
        c_ulong,
    };

    // bionic, musl and illumos declare the request as `int`, glibc and the BSDs
    // as `unsigned long`
    #[cfg(any(
        target_env = "musl",
        target_env = "ohos",
        target_os = "android",
        target_os = "illumos",
        target_os = "solaris",
    ))]
    type Request = c_int;
    #[cfg(not(any(
        target_env = "musl",
        target_env = "ohos",
        target_os = "android",
        target_os = "illumos",
        target_os = "solaris",
    )))]
    type Request = c_ulong;

    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        not(any(
            target_arch = "mips",
            target_arch = "mips32r6",
            target_arch = "mips64",
            target_arch = "mips64r6",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "sparc",
            target_arch = "sparc64",
        )),
    ))]
    const TIOCGWINSZ: Option<Request> = Some(0x5413);
    #[cfg(any(
        all(
            any(target_os = "linux", target_os = "android"),
            any(
                target_arch = "mips",
                target_arch = "mips32r6",
                target_arch = "mips64",
                target_arch = "mips64r6",
                target_arch = "powerpc",
                target_arch = "powerpc64",
                target_arch = "sparc",
                target_arch = "sparc64",
            ),
        ),
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
    ))]
    const TIOCGWINSZ: Option<Request> = Some(0x4008_7468);
    #[cfg(any(target_os = "illumos", target_os = "solaris"))]
    const TIOCGWINSZ: Option<Request> = Some(0x5468);
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "illumos",
        target_os = "solaris",
    )))]
    const TIOCGWINSZ: Option<Request> = None;

    #[repr(C)]
    struct Winsize {
        rows:    u16,
        columns: u16,
        xpixel:  u16,
        ypixel:  u16,
    }

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: Request, ...) -> c_int;
    }

    pub(super) fn columns(fd: c_int) -> Option<usize> {
        let request = TIOCGWINSZ?;
        let mut size = Winsize {
            rows:    0,
            columns: 0,
            xpixel:  0,
            ypixel:  0,
        };
        // SAFETY: TIOCGWINSZ writes one `winsize` through the pointer, which
        // `Winsize` matches field for field
        let status = unsafe { ioctl(fd, request, &raw mut size) };
        (status == 0 && size.columns > 0).then_some(usize::from(size.columns))
    }
}

#[cfg(all(feature = "std", not(unix)))]
mod sys {
    pub(super) const fn columns(_: i32) -> Option<usize> {
        None
    }
}
