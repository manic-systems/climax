// SPDX-License-Identifier: EUPL-1.2

use climax::{
    caress::Shell,
    install::manual,
    pound::{
        ArgSpec,
        CommandSpec,
        Error,
        Kind,
        Matches,
        Parse,
    },
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
            verbose: matches.switch(spec, 0)?,
        })
    }

    fn update_from_matches(
        &mut self,
        spec: &'static CommandSpec,
        matches: &Matches<'_>,
    ) -> Result<(), Error> {
        self.verbose = matches.switch(spec, 0)?;
        Ok(())
    }
}

fn main() -> std::process::ExitCode {
    let _: Option<Shell> = None;
    assert!(!manual::<Args>("October 11, 2026").is_empty());
    climax::main(|_context, args: Args| {
        let _ = args.verbose;
        Ok(())
    })
}
