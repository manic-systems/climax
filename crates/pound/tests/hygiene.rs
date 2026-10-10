// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]
#![allow(non_upper_case_globals, non_camel_case_types, dead_code)]

use pound::Parse;

struct spec;
struct m;
struct s;
struct value;

const ARGS: &str = "from-args";
const CMD: &str = "9.9";
const ARGS0: &str = "from-args0";

#[derive(Debug, Parse)]
#[pound(name = "probe", version = CMD)]
struct Flat {
    #[pound(long, default = { ARGS })]
    first:   String,
    #[pound(long, min = "1", max = "9", validate = |n: &u8| if *n > 0 { Ok(()) } else { Err("zero") }, default = "2")]
    bounded: u8,
}

#[derive(Debug, Parse)]
enum Tree {
    Run {
        #[pound(long, default = { ARGS0 })]
        first: String,
    },
}

#[test]
fn user_items_named_like_generated_ones_stay_the_users() {
    let flat = Flat::try_parse_from([]).unwrap();
    assert_eq!((flat.first.as_str(), flat.bounded), ("from-args", 2));
    let version = Flat::try_parse_from(["--version"]).unwrap_err();
    assert!(version.to_string().contains("9.9"), "{version}");
    let Tree::Run { first } = Tree::try_parse_from(["run"]).unwrap();
    assert_eq!(first, "from-args0");
}
