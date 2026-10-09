// SPDX-License-Identifier: EUPL-1.2

#![cfg(all(feature = "derive", feature = "structured"))]

use climax::{
    prelude::*,
    serde::Serialize,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(crate = "climax::serde")]
enum Shell {
    Bash,
    Nushell,
}

/// demo command
#[derive(Parse)]
struct Args {
    /// shell to configure
    #[pound(short, long)]
    shell: Shell,
}

#[test]
fn derives_resolve_through_climax_alone() {
    climax::try_run_from(["--shell", "nushell"], |_context, args: Args| {
        assert_eq!(args.shell, Shell::Nushell);
        Ok(())
    })
    .unwrap();

    let help = Args::try_parse_from(["--help"]).err().unwrap();
    assert!(help.render().contains("shell to configure"));
}
