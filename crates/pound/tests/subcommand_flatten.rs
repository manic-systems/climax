// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]

use pound::{
    ErrorKind,
    Parse,
};

#[derive(Debug, Parse, PartialEq, Eq)]
enum Deeper {
    Purge,
}

#[derive(Debug, Parse, PartialEq, Eq)]
enum Cache {
    Clean {
        #[pound(long)]
        all: bool,
    },
    #[pound(flatten)]
    Deeper(Deeper),
}

#[derive(Debug, Parse, PartialEq, Eq)]
enum Commands {
    Status,
    #[pound(hidden)]
    Internal,
    #[pound(flatten)]
    Cache(Cache),
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct Cli {
    #[pound(subcommand)]
    command: Commands,
}

#[test]
fn spliced_commands_route_through_every_enum_level() {
    for (args, expected) in [
        (vec!["status"], Commands::Status),
        (
            vec!["clean", "--all"],
            Commands::Cache(Cache::Clean { all: true }),
        ),
        (vec!["purge"], Commands::Cache(Cache::Deeper(Deeper::Purge))),
    ] {
        let parsed = Cli::try_parse_from(args.iter().copied()).unwrap();
        assert_eq!(parsed.command, expected, "{args:?}");
    }
}

#[derive(Debug, Parse)]
struct Shared {
    #[pound(long, global)]
    verbose: bool,
    #[pound(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Parse)]
#[pound(name = "demo")]
struct Root {
    #[pound(flatten)]
    shared: Shared,
}

#[test]
fn a_flattened_struct_can_own_the_optional_selector() {
    let parsed = Root::try_parse_from(["purge", "--verbose"]).unwrap();
    assert!(parsed.shared.verbose);
    assert_eq!(
        parsed.shared.command,
        Some(Commands::Cache(Cache::Deeper(Deeper::Purge)))
    );
    assert_eq!(Root::try_parse_from([]).unwrap().shared.command, None);

    #[cfg(feature = "help")]
    {
        let ErrorKind::Help(text) = Root::try_parse_from(["--help"]).unwrap_err().kind else {
            panic!("expected help");
        };
        assert!(text.contains("Usage: demo [OPTION]... [COMMAND]"));
        assert!(text.contains("clean") && text.contains("purge"));
        assert!(!text.contains("internal"));
    }
}
