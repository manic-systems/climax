// SPDX-License-Identifier: EUPL-1.2

//! subcommand fields, nested subcommands, hidden entries, and value-enum
//! possible-value listing.

#![cfg(feature = "derive")]

use pound::{
    Error,
    Parse,
    ValueEnum,
};

fn argv<'a>(a: &[&'a str]) -> Vec<&'a str> {
    a.to_vec()
}

#[derive(Parse, Debug, PartialEq, Eq)]
enum Action {
    Build {
        #[pound(short, long)]
        release: bool,
    },
    Clean,
}

// a struct with global options that delegates to a subcommand enum
#[derive(Parse, Debug)]
#[pound(name = "tool")]
struct Tool {
    #[pound(short, long)]
    verbose: bool,
    #[pound(long, default = "info")]
    log:     String,
    #[pound(subcommand)]
    action:  Action,
}

#[test]
fn struct_with_subcommand() {
    let t = Tool::parse_from(argv(&["--verbose", "build", "--release"]));
    assert!(t.verbose);
    assert_eq!(t.log, "info");
    assert_eq!(t.action, Action::Build { release: true });

    // globals can also appear with explicit values, before the subcommand
    let t = Tool::parse_from(argv(&["--log", "debug", "clean"]));
    assert!(!t.verbose);
    assert_eq!(t.log, "debug");
    assert_eq!(t.action, Action::Clean);

    // a bare invocation still shows help (subcommand required)
    assert!(matches!(Tool::try_parse_from(argv(&[])), Err(Error::Help(_))));
}

// an optional subcommand: None when absent
#[derive(Parse, Debug)]
#[pound(name = "maybe")]
struct Maybe {
    #[pound(short, long)]
    force:  bool,
    #[pound(subcommand)]
    action: Option<Action>,
}

#[test]
fn optional_subcommand() {
    let m = Maybe::parse_from(argv(&["-f"]));
    assert!(m.force);
    assert!(m.action.is_none());

    let m = Maybe::parse_from(argv(&["clean"]));
    assert_eq!(m.action, Some(Action::Clean));
}

// nested subcommands: an enum variant delegating to another subcommand enum
#[derive(Parse, Debug, PartialEq, Eq)]
enum LeaseAction {
    Open,
    Close,
}

#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "cade")]
enum Cade {
    Lease {
        #[pound(subcommand)]
        action: LeaseAction,
    },
    Status,
}

#[test]
fn nested_subcommands() {
    assert_eq!(
        Cade::parse_from(argv(&["lease", "open"])),
        Cade::Lease { action: LeaseAction::Open }
    );
    assert_eq!(Cade::parse_from(argv(&["status"])), Cade::Status);
}

// hidden subcommand: parses but absent from help
#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "svc")]
enum Svc {
    Run,
    #[pound(hidden)]
    Internal,
}

#[test]
fn hidden_subcommand() {
    assert_eq!(Svc::parse_from(argv(&["internal"])), Svc::Internal);
    #[cfg(feature = "help")]
    {
        let Err(Error::Help(text)) = Svc::try_parse_from(argv(&[])) else {
            panic!("expected help");
        };
        assert!(text.contains("run"));
        assert!(!text.contains("internal"));
    }
}

// hidden arg: parses but absent from help
#[derive(Parse)]
#[pound(name = "hid")]
#[allow(dead_code, reason = "only the parse outcome and help text are asserted")]
struct Hid {
    #[pound(long)]
    normal: bool,
    #[pound(long, hidden)]
    secret: bool,
}

#[test]
fn hidden_arg() {
    assert!(Hid::try_parse_from(argv(&["--secret"])).is_ok());
    #[cfg(feature = "help")]
    {
        let Err(Error::Help(text)) = Hid::try_parse_from(argv(&["--help"])) else {
            panic!("expected help");
        };
        assert!(text.contains("--normal"));
        assert!(!text.contains("--secret"));
    }
}

// value-enum field auto-lists its choices in errors and help
#[derive(ValueEnum, Debug, PartialEq, Eq)]
enum Level {
    Quiet,
    Normal,
    Trace,
}

#[derive(Parse)]
#[pound(name = "vb")]
#[allow(dead_code, reason = "only the parse outcome, error, and help are asserted")]
struct Vb {
    #[pound(long)]
    level: Level,
}

#[test]
fn value_enum_possible_listed() {
    match Vb::try_parse_from(argv(&["--level", "bogus"])) {
        Err(Error::Value { msg, .. }) => {
            assert!(msg.contains("quiet"), "msg was: {msg}");
            assert!(msg.contains("trace"), "msg was: {msg}");
        },
        _ => panic!("expected a value error"),
    }
    #[cfg(feature = "help")]
    {
        let Err(Error::Help(text)) = Vb::try_parse_from(argv(&["--help"])) else {
            panic!("expected help");
        };
        assert!(text.contains("[possible values: quiet, normal, trace]"), "help was:\n{text}");
    }
}
