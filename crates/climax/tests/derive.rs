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

#[derive(Debug, Eq, PartialEq)]
struct Port(u16);

impl FromArg for Port {
    fn from_arg(value: &str) -> core::result::Result<Self, ValueError> {
        value
            .parse()
            .map(Self)
            .map_err(|_| ValueError::new(value, "expected a port number"))
    }
}

#[derive(Parse)]
struct Listen {
    #[pound(long)]
    port: Port,
}

#[test]
fn the_prelude_carries_what_a_manual_from_arg_impl_needs() {
    let listen = Listen::try_parse_from(["--port", "8080"]).unwrap();
    assert_eq!(listen.port, Port(8080));
}
