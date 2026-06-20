// SPDX-License-Identifier: EUPL-1.2

//! optional-value options: an `Option<T>` named option carrying
//! `default_missing` may appear bare (use the fallback), with a value (override
//! it), or be absent (`None`).

#![cfg(feature = "derive")]

use pound::{
    Error,
    Parse,
};

fn argv<'a>(a: &[&'a str]) -> Vec<&'a str> {
    a.to_vec()
}

// bare `-o` writes to the current directory, `-o dl` to a named one
#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "grab")]
struct Grab {
    url:    Vec<String>,
    #[pound(short, long, default_missing = ".")]
    output: Option<String>,
}

#[test]
fn three_states() {
    assert_eq!(Grab::parse_from(argv(&[])).output, None);

    assert_eq!(Grab::parse_from(argv(&["-o"])).output.as_deref(), Some("."));
    assert_eq!(Grab::parse_from(argv(&["--output"])).output.as_deref(), Some("."));

    assert_eq!(Grab::parse_from(argv(&["-o", "dl"])).output.as_deref(), Some("dl"));
    assert_eq!(Grab::parse_from(argv(&["--output", "dl"])).output.as_deref(), Some("dl"));
    assert_eq!(Grab::parse_from(argv(&["-odl"])).output.as_deref(), Some("dl"));
    assert_eq!(Grab::parse_from(argv(&["--output=dl"])).output.as_deref(), Some("dl"));
}

#[test]
fn next_token_disambiguation() {
    // `--` ends options, so it never binds; the fallback stands and the url is
    // positional
    let g = Grab::parse_from(argv(&["-o", "--", "x.html"]));
    assert_eq!(g.output.as_deref(), Some("."));
    assert_eq!(g.url, ["x.html"]);

    assert_eq!(Grab::parse_from(argv(&["-o"])).output.as_deref(), Some("."));

    // a bare `-` is a value, not a switch
    assert_eq!(Grab::parse_from(argv(&["-o", "-"])).output.as_deref(), Some("-"));
}

#[test]
fn positional_binds_to_a_bare_flag() {
    // a following positional is value-shaped, so it is taken greedily; route it
    // through `--`, or place the flag last
    let g = Grab::parse_from(argv(&["-o", "x.html"]));
    assert_eq!(g.output.as_deref(), Some("x.html"));
    assert!(g.url.is_empty());

    let g = Grab::parse_from(argv(&["x.html", "-o"]));
    assert_eq!(g.url, ["x.html"]);
    assert_eq!(g.output.as_deref(), Some("."));
}

// the fallback runs through the same parse/validate path as a typed value
#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "grab")]
struct Jobs {
    #[pound(short, long, default_missing = "4", min = "1", max = "64")]
    jobs: Option<u32>,
}

#[test]
fn composes_with_validate() {
    assert_eq!(Jobs::parse_from(argv(&[])).jobs, None);
    assert_eq!(Jobs::parse_from(argv(&["--jobs"])).jobs, Some(4));
    assert_eq!(Jobs::parse_from(argv(&["--jobs", "8"])).jobs, Some(8));

    match Jobs::try_parse_from(argv(&["--jobs", "99"])) {
        Err(Error::Value { value, msg, .. }) => {
            assert_eq!(value, "99");
            assert_eq!(msg, "must be at most 64");
        },
        other => panic!("expected bound error, got {other:?}"),
    }
}

#[derive(Parse, Debug, PartialEq, Eq)]
#[pound(name = "grab")]
struct EnvOut {
    #[pound(short, long, default_missing = ".", env = "GRAB_OUTPUT")]
    output: Option<String>,
}

fn set(k: &str, v: &str) {
    unsafe { std::env::set_var(k, v) };
}
fn clear(k: &str) {
    unsafe { std::env::remove_var(k) };
}

#[test]
fn composes_with_env() {
    clear("GRAB_OUTPUT");
    set("GRAB_OUTPUT", "from-env");

    // absent flag -> env; bare flag -> fallback beats env; explicit value -> wins
    assert_eq!(EnvOut::parse_from(argv(&[])).output.as_deref(), Some("from-env"));
    assert_eq!(EnvOut::parse_from(argv(&["-o"])).output.as_deref(), Some("."));
    assert_eq!(EnvOut::parse_from(argv(&["-o", "dl"])).output.as_deref(), Some("dl"));

    clear("GRAB_OUTPUT");
    assert_eq!(EnvOut::parse_from(argv(&[])).output, None);
}

#[cfg(feature = "help")]
#[test]
fn help_brackets_optional_value() {
    let Err(Error::Help(text)) = Grab::try_parse_from(argv(&["--help"])) else {
        panic!("expected help");
    };
    assert!(text.contains("-o, --output[=OUTPUT]"), "got:\n{text}");
}
