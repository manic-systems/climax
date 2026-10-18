// SPDX-License-Identifier: EUPL-1.2

//! how `pound` paints help pages and error reports. try `--help`, or a
//! misspelled flag like `--jbs 2` for an error with a `tip:`.

#![expect(
    clippy::print_stdout,
    clippy::use_debug,
    clippy::expect_used,
    reason = "the example prints the painted report"
)]

use pound::{
    Parse,
    Term,
};

/// fetch urls to disk
#[derive(Parse, Debug)]
#[pound(name = "paint", version = "0.1.0")]
struct Paint {
    /// urls to fetch
    url:     Vec<String>,
    /// number of parallel jobs
    #[pound(short, long, default = "4")]
    jobs:    u32,
    /// overwrite existing files
    #[pound(short, long)]
    force:   bool,
    /// write downloads under this directory
    #[pound(short, long)]
    output:  Option<String>,
    /// increase verbosity, repeatable
    #[pound(short, long, count)]
    verbose: u8,
}

fn main() {
    // plain when piped or under `NO_COLOR`, painted under `CLICOLOR_FORCE`
    match Paint::try_parse() {
        Ok(paint) => println!("{paint:?}"),
        Err(err) => println!("{}", err.render(Term::stdout())),
    }

    let help = Paint::try_parse_from(["--help"]).expect_err("`--help` always yields the help page");
    println!("--- plain ---\n{}", help.render(Term::plain()));
    println!("--- painted ---\n{}", help.render(Term::new(None, true)));
}
