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
    let y = Sandbox::parse_from(argv(&[
        "-s", "--env", "A=1", "-e", "B=2", "--", "ls", "-la",
    ]));
    assert!(y.sockets);
    assert_eq!(y.env, ["A=1", "B=2"]);
    assert_eq!(y.exec, ["ls", "-la"]);
}

// count flag and an optional positional
#[derive(Parse)]
#[pound(name = "build")]
struct Build {
    target:  Option<String>,
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
    assert_eq!(Pkg::parse_from(argv(&["init", "--force"])), Pkg::Init {
        force: true,
    });
    assert_eq!(
        Pkg::parse_from(argv(&["add", "serde", "https://x", "-f"])),
        Pkg::Add {
            name:  "serde".into(),
            url:   "https://x".into(),
            force: true,
        }
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
    assert_eq!(
        Run::parse_from(argv(&["--mode", "double-speed"])).mode,
        Mode::DoubleSpeed
    );
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
    assert!(matches!(
        Pick::try_parse_from(argv(&[])),
        Err(Error::MissingGroup { .. })
    ));
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

#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "fallback-limit")]
struct FallbackLimit {
    #[pound(long, default = "8", min = "5", max = "20", validate = "even")]
    count: u64,
    #[pound(long, max_len = "4")]
    label: Option<String>,
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
    let parsed = Limit::parse_from(argv(&["--count", "12", "--name", "short", "--shard", "2"]));
    assert_eq!(parsed.count, 12);
    assert_eq!(parsed.name, "short");
    assert_eq!(parsed.shard, 2);

    match Limit::try_parse_from(argv(&["--count", "4", "--name", "short", "--shard", "2"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "4");
            assert_eq!(msg, "must be at least 5");
        },
        other => panic!("expected lower bound value error, got {other:?}"),
    }

    match Limit::try_parse_from(argv(&["--count", "21", "--name", "short", "--shard", "2"])) {
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

    match Limit::try_parse_from(argv(&["--count", "12", "--name", "short", "--shard", "3"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "3");
            assert_eq!(msg, "must be even");
        },
        other => panic!("expected custom validation error, got {other:?}"),
    }
}

#[test]
fn validation_runs_for_defaults_and_options() {
    let parsed = FallbackLimit::parse_from(argv(&[]));
    assert_eq!(parsed.count, 8);
    assert_eq!(parsed.label, None);

    let parsed = FallbackLimit::parse_from(argv(&["--label", "mini"]));
    assert_eq!(parsed.label.as_deref(), Some("mini"));

    match FallbackLimit::try_parse_from(argv(&["--count", "3"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "3");
            assert_eq!(msg, "must be at least 5");
        },
        other => panic!("expected defaulted field validation error, got {other:?}"),
    }

    match FallbackLimit::try_parse_from(argv(&["--label", "large"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "large");
            assert_eq!(msg, "must be at most 4 chars");
        },
        other => panic!("expected optional field length error, got {other:?}"),
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

#[derive(Debug, PartialEq, Eq)]
struct HexByte(u8);

fn hex_byte(value: &str) -> Result<HexByte, &'static str> {
    let value = value.strip_prefix("0x").ok_or("expected 0xNN")?;
    u8::from_str_radix(value, 16)
        .map(HexByte)
        .map_err(|_| "expected two hex digits")
}

const fn non_zero_hex(value: &HexByte) -> Result<(), &'static str> {
    if value.0 == 0 {
        Err("must not be zero")
    } else {
        Ok(())
    }
}

#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "hex")]
struct Hex {
    #[pound(long, parse = "hex_byte", validate = "non_zero_hex")]
    byte:      HexByte,
    #[pound(long, parse = "hex_byte")]
    many:      Vec<HexByte>,
    #[pound(long, parse = "hex_byte")]
    maybe:     Option<HexByte>,
    #[pound(long, default = "0x10", parse = "hex_byte")]
    defaulted: HexByte,
}

#[test]
fn custom_parsers() {
    let parsed = Hex::parse_from(argv(&[
        "--byte", "0x2a", "--many", "0x01", "--many", "0xff",
    ]));
    assert_eq!(parsed.byte, HexByte(42));
    assert_eq!(parsed.many, [HexByte(1), HexByte(255)]);
    assert_eq!(parsed.maybe, None);
    assert_eq!(parsed.defaulted, HexByte(16));

    let parsed = Hex::parse_from(argv(&[
        "--byte",
        "0x2a",
        "--maybe",
        "0x03",
        "--defaulted",
        "0x04",
    ]));
    assert_eq!(parsed.maybe, Some(HexByte(3)));
    assert_eq!(parsed.defaulted, HexByte(4));

    match Hex::try_parse_from(argv(&["--byte", "2a"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "2a");
            assert_eq!(msg, "expected 0xNN");
        },
        other => panic!("expected custom parser error, got {other:?}"),
    }

    match Hex::try_parse_from(argv(&["--byte", "0x00"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "0x00");
            assert_eq!(msg, "must not be zero");
        },
        other => panic!("expected custom parser validation error, got {other:?}"),
    }
}
