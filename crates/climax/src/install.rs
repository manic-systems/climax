//! install: shell completion scripts and man pages

use caress::man::pages;
pub use caress::{
    Shell,
    man::Page,
};
use pound::Parse;

/// mdoc pages for `C` and its visible subcommands, stamped with `date`
#[must_use]
pub fn manual<C: Parse>(date: &str) -> Vec<Page> {
    pages(C::SPEC, date)
}
