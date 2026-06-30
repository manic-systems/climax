// SPDX-License-Identifier: EUPL-1.2

//! aliases: extra long names for an option, and extra names for a subcommand.
//! aliases match on the command line but stay out of help.

use pound::Parse;

#[derive(Parse)]
enum Cmd {
    #[pound(alias = "rm,del")]
    Remove {
        name: String,
    },
    Status,
}

#[derive(Parse)]
#[pound(name = "app")]
struct Cli {
    #[pound(long, alias = "colour")]
    color: Option<String>,
    #[pound(subcommand)]
    cmd:   Cmd,
}

#[test]
fn arg_alias_matches_like_the_primary() {
    let a = Cli::try_parse_from(["--color", "red", "status"]).unwrap();
    assert_eq!(a.color.as_deref(), Some("red"));
    let a = Cli::try_parse_from(["--colour", "blue", "status"]).unwrap();
    assert_eq!(a.color.as_deref(), Some("blue"));
}

#[test]
fn command_aliases_select_the_variant() {
    // the primary name plus both aliases all reach Remove and bind its field
    for name in ["remove", "rm", "del"] {
        let Cmd::Remove { name: got } = Cli::try_parse_from([name, "x"]).unwrap().cmd else {
            panic!("`{name}` did not select Remove");
        };
        assert_eq!(got, "x");
    }
}
