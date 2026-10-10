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
    Remove {
        #[pound(long, validate = "positive")]
        times: u32,
    },
}

#[allow(clippy::missing_const_for_fn, clippy::trivially_copy_pass_by_ref)]
fn positive(n: &u32) -> Result<(), &'static str> {
    if *n > 0 { Ok(()) } else { Err("zero") }
}

#[test]
fn conversion_errors_carry_the_selected_command_usage() {
    let err = Cli::try_parse_from(["add", "oops"]).unwrap_err();
    let usage = err.usage.as_deref().expect("usage line");
    assert!(usage.starts_with("Usage: prog add"), "{usage}");
    assert_eq!(err.help_flag, Some("--help"));
}

#[test]
fn validation_errors_carry_the_selected_command_usage() {
    let err = Cli::try_parse_from(["remove", "--times", "0"]).unwrap_err();
    let usage = err.usage.as_deref().expect("usage line");
    assert!(usage.starts_with("Usage: prog remove"), "{usage}");
}
