#![cfg(feature = "derive")]

use pound::{
    ErrorKind,
    Parse,
};

#[derive(Debug, Parse, PartialEq, Eq)]
struct Middle {
    second: String,
    #[pound(long, default = "8")]
    limit:  usize,
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
    let parsed = Interleaved::try_parse_from(["one", "two", "--limit", "3", "three", "four"]);
    assert_eq!(parsed.unwrap(), Interleaved {
        first:  "one".to_owned(),
        middle: Middle {
            second: "two".to_owned(),
            limit:  3,
            third:  "three".to_owned(),
        },
        fourth: "four".to_owned(),
    });
}

#[derive(Debug, Parse)]
struct Shared {
    #[pound(long, global, negate, default = "true")]
    cache:   bool,
    #[pound(short = 'v', long, global, count)]
    verbose: u32,
}

#[derive(Debug, Parse)]
struct Root {
    #[pound(flatten)]
    shared:  Shared,
    #[pound(subcommand)]
    command: Outer,
}

#[derive(Debug, Parse)]
enum Outer {
    Outer {
        #[pound(subcommand)]
        command: Inner,
    },
}

#[derive(Debug, Parse)]
enum Inner {
    Inner {
        #[pound(flatten)]
        shared: Shared,
    },
}

#[test]
fn a_reused_flattened_global_belongs_to_the_scope_that_parsed_it() {
    let parsed =
        Root::try_parse_from(["outer", "--no-cache", "-v", "inner", "--cache", "-vv"]).unwrap();
    let Outer::Outer {
        command: Inner::Inner { shared },
    } = parsed.command;
    assert!(!parsed.shared.cache);
    assert_eq!(parsed.shared.verbose, 1);
    assert!(shared.cache);
    assert_eq!(shared.verbose, 2);
}

#[derive(Debug, Parse)]
#[pound(required_group = "mode")]
struct Fast {
    #[pound(long, group = "mode")]
    fast: bool,
}

#[derive(Debug, Parse)]
struct Grouped {
    #[pound(long, group = "mode")]
    safe: bool,
    #[pound(flatten)]
    fast: Fast,
}

#[test]
fn a_group_spans_every_flattened_struct_at_its_level() {
    let parsed = Grouped::try_parse_from(["--safe"]).unwrap();
    assert!(parsed.safe && !parsed.fast.fast);
    assert!(matches!(
        Grouped::try_parse_from([]).unwrap_err().kind,
        ErrorKind::MissingGroup { .. }
    ));
    assert!(matches!(
        Grouped::try_parse_from(["--safe", "--fast"])
            .unwrap_err()
            .kind,
        ErrorKind::Conflict { .. }
    ));
}
