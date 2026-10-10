// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]

extern crate alloc;

use pound::Parse;

#[derive(Debug, Parse)]
#[pound(name = "paths")]
struct Args {
    #[pound(long)]
    a: ::core::option::Option<u8>,
    #[pound(long)]
    b: core::option::Option<u8>,
    #[pound(long)]
    c: std::option::Option<u8>,
    #[pound(long)]
    d: ::std::option::Option<u8>,
    #[pound(long)]
    e: std::vec::Vec<u8>,
    #[pound(long)]
    f: ::std::vec::Vec<u8>,
    #[pound(long)]
    g: alloc::vec::Vec<u8>,
    #[pound(long)]
    h: ::alloc::vec::Vec<u8>,
}

#[test]
fn qualified_wrappers_keep_their_cardinality() {
    let none = Args::try_parse_from(<[&str; 0]>::default()).unwrap();
    assert_eq!((none.a, none.b, none.c, none.d), (None, None, None, None));
    assert!(none.e.is_empty() && none.f.is_empty() && none.g.is_empty() && none.h.is_empty());

    let some = Args::try_parse_from(["--a", "1", "--d", "4", "--e", "5", "--e", "6", "--h", "8"])
        .unwrap();
    assert_eq!((some.a, some.b, some.c, some.d), (Some(1), None, None, Some(4)));
    assert_eq!(some.e, [5, 6]);
    assert_eq!(some.h, [8]);
}
