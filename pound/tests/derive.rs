// SPDX-License-Identifier: EUPL-1.2

//! the derive producing the same specs as the hand-written impls in cli.rs,
//! across a flat command, a subcommand tree, and a value enum.

#![cfg(feature = "derive")]

use pound::{
    Error,
    Parse,
    ValueEnum,
};

fn argv<'a>(a: &[&'a str]) -> Vec<&'a str> {
    a.to_vec()
}

// sandbox: flat command, flags + repeatable option + trailing exec
#[derive(Parse)]
#[pound(name = "sandbox", version = "0.1.0")]
/// simple sandboxer
struct Sandbox {
    /// allow unix sockets
    #[pound(short, long)]
    sockets: bool,
    /// set env var
    #[pound(short, long)]
    env:     Vec<String>,
    #[pound(trailing)]
    exec:    Vec<String>,
}

#[test]
fn sandbox_flat() {
    let y = Sandbox::parse_from(argv(&["-s", "--env", "A=1", "-e", "B=2", "--", "ls", "-la"]));
    assert!(y.sockets);
    assert_eq!(y.env, ["A=1", "B=2"]);
    assert_eq!(y.exec, ["ls", "-la"]);
}

// count flag and an optional positional
#[derive(Parse)]
#[pound(name = "build")]
struct Build {
    target: Option<String>,
    #[pound(short, long, count)]
    verbose: u8,
}

#[test]
fn counts_and_optionals() {
    let b = Build::parse_from(argv(&["-vvv"]));
    assert_eq!(b.verbose, 3);
    assert_eq!(b.target, None);
    let b = Build::parse_from(argv(&["release", "--verbose"]));
    assert_eq!(b.verbose, 1);
    assert_eq!(b.target.as_deref(), Some("release"));
}

// pkg: subcommand tree
#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "pkg", version = "1.0.0")]
/// a small package manager
enum Pkg {
    /// initialise a project
    Init {
        #[pound(short, long)]
        force: bool,
    },
    /// add a pin
    Add {
        name:  String,
        url:   String,
        #[pound(short, long)]
        force: bool,
    },
}

#[test]
fn pkg_subcommands() {
    assert_eq!(Pkg::parse_from(argv(&["init", "--force"])), Pkg::Init { force: true });
    assert_eq!(
        Pkg::parse_from(argv(&["add", "serde", "https://x", "-f"])),
        Pkg::Add { name: "serde".into(), url: "https://x".into(), force: true }
    );
}

#[test]
fn pkg_help_screen() {
    let Err(Error::Help(text)) = Pkg::try_parse_from(argv(&["--help"])) else {
        panic!("expected help");
    };
    assert!(text.contains("Usage: pkg"));
    #[cfg(feature = "help")]
    {
        assert!(text.contains("Commands:"));
        assert!(text.contains("initialise a project"));
    }
}

// a derived value enum used as a typed option
#[derive(ValueEnum, Debug, PartialEq, Eq)]
enum Mode {
    Fast,
    DoubleSpeed,
}

#[derive(Parse)]
#[pound(name = "run")]
struct Run {
    #[pound(long)]
    mode: Mode,
}

#[test]
fn derived_value_enum() {
    assert_eq!(Run::parse_from(argv(&["--mode", "fast"])).mode, Mode::Fast);
    // camelCase variant becomes kebab-case
    assert_eq!(Run::parse_from(argv(&["--mode", "double-speed"])).mode, Mode::DoubleSpeed);
    match Run::try_parse_from(argv(&["--mode", "warp"])) {
        Err(Error::Value { value, .. }) => assert_eq!(value, "warp"),
        _ => panic!("expected a value error"),
    }
}

// a required, mutually exclusive group: exactly one of --fast/--slow
#[derive(Parse)]
#[pound(name = "pick", required_group = "speed")]
#[allow(dead_code, reason = "only the parse outcome is asserted")]
struct Pick {
    #[pound(long, group = "speed")]
    fast: bool,
    #[pound(long, group = "speed")]
    slow: bool,
}

#[test]
fn required_group() {
    assert!(Pick::try_parse_from(argv(&["--fast"])).is_ok());
    assert!(matches!(Pick::try_parse_from(argv(&[])), Err(Error::MissingGroup { .. })));
    assert!(matches!(
        Pick::try_parse_from(argv(&["--fast", "--slow"])),
        Err(Error::Conflict { .. })
    ));
}

// pairwise conflict without a named group
#[derive(Parse)]
#[pound(name = "log")]
#[allow(dead_code, reason = "only the parse outcome is asserted")]
struct Log {
    #[pound(long)]
    quiet:   bool,
    #[pound(long, conflicts_with = "quiet")]
    verbose: bool,
}

#[test]
fn conflicts_with() {
    assert!(Log::try_parse_from(argv(&["--quiet"])).is_ok());
    assert!(Log::try_parse_from(argv(&["--verbose"])).is_ok());
    assert!(matches!(
        Log::try_parse_from(argv(&["--quiet", "--verbose"])),
        Err(Error::Conflict { .. })
    ));
}

#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "limit")]
struct Limit {
    #[pound(long, min = "5", max = "20")]
    count: u64,
    #[pound(long, max_len = "9")]
    name:  String,
    #[pound(long, validate = "even")]
    shard: u64,
}

#[allow(
    clippy::missing_const_for_fn,
    clippy::trivially_copy_pass_by_ref,
    reason = "validator hooks receive the parsed field by reference"
)]
fn even(value: &u64) -> Result<(), &'static str> {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err("must be even")
    }
}

#[test]
fn validated_values() {
    let parsed = Limit::parse_from(argv(&[
        "--count", "12", "--name", "short", "--shard", "2",
    ]));
    assert_eq!(parsed.count, 12);
    assert_eq!(parsed.name, "short");
    assert_eq!(parsed.shard, 2);

    match Limit::try_parse_from(argv(&[
        "--count", "4", "--name", "short", "--shard", "2",
    ])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "4");
            assert_eq!(msg, "must be at least 5");
        },
        other => panic!("expected lower bound value error, got {other:?}"),
    }

    match Limit::try_parse_from(argv(&[
        "--count", "21", "--name", "short", "--shard", "2",
    ])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "21");
            assert_eq!(msg, "must be at most 20");
        },
        other => panic!("expected upper bound value error, got {other:?}"),
    }

    match Limit::try_parse_from(argv(&[
        "--count",
        "12",
        "--name",
        "very-long-name",
        "--shard",
        "2",
    ])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "very-long-name");
            assert_eq!(msg, "must be at most 9 chars");
        },
        other => panic!("expected length value error, got {other:?}"),
    }

    match Limit::try_parse_from(argv(&[
        "--count", "12", "--name", "short", "--shard", "3",
    ])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "3");
            assert_eq!(msg, "must be even");
        },
        other => panic!("expected custom validation error, got {other:?}"),
    }
}

#[derive(Parse)]
#[pound(name = "batch")]
struct Batch {
    #[pound(long, min = "5", max = "20")]
    counts: Vec<u64>,
}

#[test]
fn validated_repeatable_values() {
    assert_eq!(
        Batch::parse_from(argv(&["--counts", "5", "--counts", "20"])).counts,
        [5, 20]
    );
    assert!(matches!(
        Batch::try_parse_from(argv(&["--counts", "5", "--counts", "40"])),
        Err(Error::Value { value, .. }) if value == "40"
    ));
}
