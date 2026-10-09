// SPDX-License-Identifier: EUPL-1.2

use climax::pound::{
    ArgSpec,
    CommandSpec,
    Error,
    Kind,
    Matches,
    Parse,
};

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

fn main() -> std::process::ExitCode {
    climax::main(|_context, args: Args| {
        let _ = args.verbose;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_written_parse_impl_parses_through_climax() {
        assert!(Args::try_parse_from(["--verbose"]).unwrap().verbose);
    }
}
