#![cfg(feature = "derive")]

use pound::{ErrorKind, Parse};

#[derive(Debug, Parse, PartialEq, Eq)]
struct Middle {
    second: String,
    third:  String,
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct Interleaved {
    first:  String,
    #[pound(flatten)]
    middle: Middle,
    fourth: String,
}

#[test]
fn flattened_positionals_keep_declaration_order() {
    let parsed = Interleaved::try_parse_from(["one", "two", "three", "four"]).unwrap();

    assert_eq!(parsed, Interleaved {
        first:  "one".into(),
        middle: Middle {
            second: "two".into(),
            third:  "three".into(),
        },
        fourth: "four".into(),
    });
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct DuplicateLong {
    #[pound(long = "shared")]
    direct: bool,
    #[pound(flatten)]
    nested: DuplicateLongNested,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct DuplicateLongNested {
    #[pound(long = "shared")]
    nested: bool,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct DuplicateShort {
    #[pound(short = 's')]
    direct: bool,
    #[pound(flatten)]
    nested: DuplicateShortNested,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct DuplicateShortNested {
    #[pound(short = 's')]
    nested: bool,
}

#[test]
fn flattened_groups_reject_ambiguous_switches() {
    for result in [
        DuplicateLong::try_parse_from([]).map(|_| ()),
        DuplicateShort::try_parse_from([]).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(error) if matches!(error.kind, ErrorKind::InvalidSpecification(_))
        ));
    }
}
