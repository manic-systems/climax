//! what a package installs beside the binary, its shell completion scripts and
//! man pages

use caress::man::pages;
pub use caress::{
    Shell,
    man::Page,
};
use pound::Parse;

/// an mdoc page for `C` and every visible subcommand below it. `date` goes on
/// each page as written, e.g. `October 11, 2026`.
#[must_use]
pub fn manual<C: Parse>(date: &str) -> Vec<Page> {
    pages(C::SPEC, date)
}
