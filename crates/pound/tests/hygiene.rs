// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]
#![allow(non_upper_case_globals, non_camel_case_types, dead_code)]

use pound::{Parse, ValueEnum};

struct spec;
struct m;
struct __s;
struct __sm;
struct __value;
struct __bound;
struct __msg;
struct other;
struct s;

const ARGS: &str = "from-args";
const CMD: &str = "9.9";
const GROUPS: &str = "from-groups";
const CONFLICTS: &str = "from-conflicts";
const REQUIRES: &str = "from-requires";
const SUBS: &str = "from-subs";
const ROOT: &str = "from-root";
const ARGS0: &str = "from-args0";
const CMD0: &str = "from-cmd0";
const CMD1: &str = "from-cmd1";

fn passthrough(raw: &str) -> Result<String, &'static str> {
    if raw.is_empty() { Err("empty") } else { Ok(format!("{raw}{ARGS}")) }
}

#[derive(Debug, Parse)]
#[pound(name = "probe", version = CMD)]
struct Flat {
    #[pound(long, default = { ARGS })]
    first:  String,
    #[pound(long, default = { GROUPS })]
    second: String,
    #[pound(long, parse = passthrough, validate = |v: &String| if v.is_empty() { Err(CONFLICTS) } else { Ok(()) }, default = { REQUIRES })]
    third:  String,
    #[pound(long, min = "1", max = "9", max_len = "3", default = "2")]
    bounded: u8,
}

#[derive(Debug, Parse)]
#[pound(name = "probe", version = CMD)]
enum Tree {
    Run {
        #[pound(long, default = { ARGS0 })]
        first: String,
        #[pound(long, default = { SUBS })]
        third: String,
    },
    Plain,
    Stop {
        #[pound(long, default = { ROOT })]
        second: String,
    },
}

#[derive(Debug, PartialEq, ValueEnum)]
enum Pick {
    Left,
    Right,
}

#[derive(Debug, Parse)]
#[pound(name = "probe", version = CMD)]
struct Picked {
    #[pound(long, default = left)]
    pick: Pick,
}

#[test]
fn user_items_named_like_generated_locals_do_not_break_the_struct() {
    let args = Flat::try_parse_from(["--third", "x"]).unwrap();
    assert_eq!(args.first, "from-args");
    assert_eq!(args.second, "from-groups");
    assert_eq!(args.third, "xfrom-args");
    assert_eq!(args.bounded, 2);
}

#[test]
fn user_expressions_see_user_items_in_enums() {
    match Tree::try_parse_from(["run"]).unwrap() {
        Tree::Run { first, third } => {
            assert_eq!(first, "from-args0");
            assert_eq!(third, "from-subs");
        },
        unexpected => panic!("{unexpected:?}"),
    }
    match Tree::try_parse_from(["stop"]).unwrap() {
        Tree::Stop { second } => assert_eq!(second, "from-root"),
        unexpected => panic!("{unexpected:?}"),
    }
    assert!(matches!(Tree::try_parse_from(["plain"]), Ok(Tree::Plain)));
}

#[test]
fn version_expressions_see_user_items() {
    let err = Flat::try_parse_from(["--version"]).unwrap_err();
    assert!(err.to_string().contains("9.9"), "{err}");
    let picked = Picked::try_parse_from(["--pick", "right"]).unwrap();
    assert_eq!(picked.pick, Pick::Right);
}
