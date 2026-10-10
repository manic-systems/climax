// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]
#![allow(dead_code)]

use pound::Parse;

#[derive(Debug, Parse)]
#[pound(name = "prog")]
struct Cli {
    #[pound(subcommand)]
    command: Commands,
}

#[derive(Debug, Parse)]
enum Commands {
    Add {
        #[pound(positional)]
        count: u32,
    },
}

#[test]
fn conversion_errors_carry_the_selected_command_usage() {
    let err = Cli::try_parse_from(["add", "oops"]).unwrap_err();
    let usage = err.usage.as_deref().expect("usage line");
    assert!(usage.starts_with("Usage: prog add"), "{usage}");
    assert_eq!(err.help_flag, Some("--help"));
}

#[derive(Debug, Parse)]
#[pound(name = "prog")]
struct Nested {
    #[pound(subcommand)]
    command: Group,
}

#[derive(Debug, Parse)]
enum Group {
    Pkg {
        #[pound(subcommand)]
        action: PkgAction,
    },
}

#[derive(Debug, Parse)]
enum PkgAction {
    Install,
    Remove,
}

#[test]
fn missing_nested_subcommand_reports_that_commands_help() {
    let err = Nested::try_parse_from(["pkg"]).unwrap_err();
    assert!(matches!(err.kind, pound::ErrorKind::MissingSubcommand {
        help: Some(_),
    }));
    let report = err.render();
    assert!(report.contains("Usage: prog pkg"), "{report}");
    #[cfg(feature = "help")]
    {
        assert!(report.contains("install"), "{report}");
        assert!(report.contains("remove"), "{report}");
    }
}
