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
fn bare_root_reports_help_as_an_error() {
    let err = Nested::try_parse_from([] as [&str; 0]).unwrap_err();
    assert_eq!(err.kind, pound::ErrorKind::MissingSubcommand);
    assert!(!err.is_exit());
    let report = err.render();
    assert!(
        report.starts_with("error: a subcommand is required"),
        "{report}"
    );
    assert!(report.contains("Usage: prog"), "{report}");
    #[cfg(feature = "help")]
    assert!(report.contains("pkg"), "{report}");
}

#[test]
fn missing_nested_subcommand_reports_that_commands_help() {
    let err = Nested::try_parse_from(["pkg"]).unwrap_err();
    assert_eq!(err.kind, pound::ErrorKind::MissingSubcommand);
    let report = err.render();
    assert!(report.contains("Usage: prog pkg"), "{report}");
    #[cfg(feature = "help")]
    {
        assert!(report.contains("install"), "{report}");
        assert!(report.contains("remove"), "{report}");
    }
}

#[cfg(feature = "std")]
#[test]
fn missing_subcommand_exits_2_on_stderr() {
    const CHILD: &str = "POUND_TEST_EXIT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        Nested::try_parse_from(["pkg"]).unwrap_err().exit();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "missing_subcommand_exits_2_on_stderr",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Usage:"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: prog pkg"));
}
