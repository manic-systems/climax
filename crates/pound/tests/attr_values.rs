// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]

use core::num::NonZeroUsize;

use pound::Parse;

const SYSTEM_PROFILE: &str = "system";

fn even(raw: &str) -> Result<u32, &'static str> {
    let n: u32 = raw.parse().map_err(|_| "not a number")?;
    if n.is_multiple_of(2) {
        Ok(n)
    } else {
        Err("odd")
    }
}

#[derive(Debug, Parse)]
#[pound(name = "attrs")]
struct Args {
    #[pound(long, parse = str::parse::<NonZeroUsize>)]
    jobs:    NonZeroUsize,
    #[pound(long, parse = even, validate = |n: &u32| if *n < 100 { Ok(()) } else { Err("too big") })]
    pairs:   u32,
    #[pound(long, default = { SYSTEM_PROFILE })]
    profile: String,
    #[pound(long, default = auto)]
    bare:    String,
    #[pound(long, default = "literal")]
    mode:    String,
    #[pound(long, parse = "even", default = "4")]
    quoted:  u32,
}

fn parse(args: &[&str]) -> Result<Args, pound::ErrorKind> {
    Args::try_parse_from(args.iter().copied()).map_err(|e| e.kind)
}

#[test]
fn parse_and_validate_take_paths_and_expressions() {
    let args = parse(&["--jobs", "3", "--pairs", "8"]).unwrap();
    assert_eq!(args.jobs.get(), 3);
    assert_eq!(args.pairs, 8);

    assert!(parse(&["--jobs", "0", "--pairs", "8"]).is_err());
    assert!(parse(&["--jobs", "1", "--pairs", "7"]).is_err());
    assert!(parse(&["--jobs", "1", "--pairs", "200"]).is_err());
}

#[test]
fn default_takes_a_constant_or_a_literal() {
    let args = parse(&["--jobs", "1", "--pairs", "2"]).unwrap();
    assert_eq!(args.profile, SYSTEM_PROFILE);
    assert_eq!(args.mode, "literal");
    assert_eq!(args.bare, "auto");
    assert_eq!(args.quoted, 4);
    assert_eq!(
        parse(&["--jobs", "1", "--pairs", "2", "--profile", "user"])
            .unwrap()
            .profile,
        "user"
    );
}

macro_rules! forwarded {
    ($parser:expr, $validator:expr) => {
        #[derive(Debug, Parse)]
        struct Forwarded {
            #[pound(long, parse = $parser, validate = $validator)]
            pairs: u32,
        }
    };
}

#[allow(clippy::missing_const_for_fn, clippy::trivially_copy_pass_by_ref)]
fn small(n: &u32) -> Result<(), &'static str> {
    if *n < 100 { Ok(()) } else { Err("too big") }
}

forwarded!("even", "small");

#[test]
fn forwarded_quoted_callables_still_name_paths() {
    let parse = |args: &[&str]| Forwarded::try_parse_from(args.iter().copied());
    assert_eq!(parse(&["--pairs", "8"]).unwrap().pairs, 8);
    assert!(parse(&["--pairs", "7"]).is_err());
    assert!(parse(&["--pairs", "200"]).is_err());
}

struct Wrap<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> Wrap<A, B> {
    fn raw(raw: &str) -> Result<u32, &'static str> {
        even(raw)
    }
}

#[derive(Debug, Parse)]
struct Generic {
    #[pound(long, parse = |s: &str| -> Result<u32, &'static str> { even(s) }, validate = |n: &u32| -> Result<(), &'static str> { small(n) })]
    closure:   u32,
    #[pound(long, parse = <Wrap<u8, u16>>::raw, default = "2")]
    qualified: u32,
    #[pound(long, parse = <u32 as core::str::FromStr>::from_str)]
    plain:     Option<u32>,
}

#[test]
fn generic_arguments_do_not_split_attribute_values() {
    let parse = |args: &[&str]| Generic::try_parse_from(args.iter().copied());
    let args = parse(&["--closure", "4", "--qualified", "6", "--plain", "9"]).unwrap();
    assert_eq!((args.closure, args.qualified, args.plain), (4, 6, Some(9)));
    assert_eq!(parse(&["--closure", "4"]).unwrap().qualified, 2);
    assert!(parse(&["--closure", "5"]).is_err());
    assert!(parse(&["--closure", "4", "--qualified", "7"]).is_err());
}

macro_rules! forwarded_defaults {
    ($bare:expr, $constant:expr) => {
        #[derive(Debug, Parse)]
        struct Defaults {
            #[pound(long, default = $bare)]
            bare:     String,
            #[pound(long, default = $constant)]
            constant: String,
        }
    };
}

forwarded_defaults!(auto, { SYSTEM_PROFILE });

#[test]
fn forwarded_defaults_keep_their_meaning() {
    let args = Defaults::try_parse_from([]).unwrap();
    assert_eq!(args.bare, "auto");
    assert_eq!(args.constant, SYSTEM_PROFILE);
}
