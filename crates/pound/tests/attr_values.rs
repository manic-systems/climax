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

#[allow(clippy::missing_const_for_fn, clippy::trivially_copy_pass_by_ref)]
fn small(n: &u32) -> Result<(), &'static str> {
    if *n < 100 { Ok(()) } else { Err("too big") }
}

struct Wrap<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> Wrap<A, B> {
    fn raw(raw: &str) -> Result<u32, &'static str> {
        even(raw)
    }
}

#[derive(Debug, Parse)]
struct Args {
    #[pound(long, parse = str::parse::<NonZeroUsize>)]
    jobs:      NonZeroUsize,
    #[pound(long, parse = even, validate = |n: &u32| if *n < 100 { Ok(()) } else { Err("too big") })]
    pairs:     u32,
    #[pound(long, parse = <Wrap<u8, u16>>::raw, default = "2")]
    qualified: u32,
    #[pound(long, parse = |raw: &str| if raw < "5" { even(raw) } else { Err("late") }, validate = |n: &u32| if *n > 0 { Ok(()) } else { Err("zero") })]
    compared:  Option<u32>,
    #[pound(long, default = { SYSTEM_PROFILE })]
    profile:   String,
    #[pound(long, default = auto)]
    bare:      String,
    #[pound(long, parse = "even", default = "4")]
    quoted:    u32,
}

fn parse(args: &[&str]) -> Result<Args, pound::ErrorKind> {
    Args::try_parse_from(args.iter().copied()).map_err(|error| error.kind)
}

#[test]
fn parse_validate_and_default_take_expressions() {
    let args = parse(&["--jobs", "3", "--pairs", "8", "--compared", "2"]).unwrap();
    assert_eq!(
        (args.jobs.get(), args.pairs, args.compared),
        (3, 8, Some(2))
    );
    assert_eq!((args.qualified, args.quoted), (2, 4));
    assert_eq!(
        (args.profile.as_str(), args.bare.as_str()),
        (SYSTEM_PROFILE, "auto")
    );

    assert!(parse(&["--jobs", "0", "--pairs", "8"]).is_err());
    assert!(parse(&["--jobs", "1", "--pairs", "7"]).is_err());
    assert!(parse(&["--jobs", "1", "--pairs", "200"]).is_err());
    assert!(parse(&["--jobs", "1", "--pairs", "2", "--compared", "0"]).is_err());
}

macro_rules! forwarded {
    ($parser:expr, $validator:expr, $bare:expr, $constant:expr) => {
        #[derive(Debug, Parse)]
        struct Forwarded {
            #[pound(long, parse = $parser, validate = $validator)]
            pairs:    u32,
            #[pound(long, default = $bare)]
            bare:     String,
            #[pound(long, default = $constant)]
            constant: String,
        }
    };
}

forwarded!("even", "small", auto, { SYSTEM_PROFILE });

#[test]
fn forwarded_values_keep_their_meaning() {
    let parse = |args: &[&str]| Forwarded::try_parse_from(args.iter().copied());
    let args = parse(&["--pairs", "8"]).unwrap();
    assert_eq!(args.pairs, 8);
    assert_eq!(
        (args.bare.as_str(), args.constant.as_str()),
        ("auto", SYSTEM_PROFILE)
    );
    assert!(parse(&["--pairs", "7"]).is_err());
    assert!(parse(&["--pairs", "200"]).is_err());
}
