// SPDX-License-Identifier: EUPL-1.2

use pound::{ArgSpec, CommandSpec, Error, Kind, Matches, Parse};

struct Args {
    verbose: bool,
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

fn main() {
    let parsed = Args::try_parse_from(["--verbose"]).expect("fixture arguments parse");
    assert!(parsed.verbose);
}
