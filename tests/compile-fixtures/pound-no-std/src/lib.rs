// SPDX-License-Identifier: EUPL-1.2

#![cfg_attr(not(test), no_std)]

use pound::{ArgSpec, CommandSpec, Error, Kind, Matches, Parse};

pub struct Args {
    pub verbose: bool,
}

const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Flag).long("verbose")];
const SPEC: CommandSpec = CommandSpec::new("fixture").args(ARGS);

impl Parse for Args {
    const SPEC: &'static CommandSpec = &SPEC;

    fn from_matches(spec: &'static CommandSpec, matches: &Matches<'_>) -> Result<Self, Error> {
        Ok(Self {
            verbose: matches.switch(spec, 0),
        })
    }
}

/// Parses borrowed arguments through the entry point pound keeps without `std`.
pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<Args, Error> {
    Args::try_parse_from(args)
}

#[cfg(test)]
mod tests {
    #[test]
    fn borrowed_arguments_parse_without_std() {
        assert!(super::parse(["--verbose"]).expect("fixture arguments parse").verbose);
        assert!(!super::parse([]).expect("no arguments parse").verbose);
    }
}
