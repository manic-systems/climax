use pound::{Error, Parse};

#[derive(Debug, Parse, PartialEq, Eq)]
struct Shared {
    /// minimum appearances
    #[pound(long, default = "8")]
    min_count: usize,
    /// history entries to inspect
    #[pound(long, env = "POUND_TEST_HISTORY_LIMIT", default = "1000")]
    limit: usize,
}

#[derive(Debug, Parse, PartialEq, Eq)]
enum Command {
    Scan {
        #[pound(flatten)]
        shared: Shared,
        #[pound(long, default = "40")]
        top: usize,
    },
}

#[derive(Debug, Parse, PartialEq, Eq)]
#[pound(name = "demo")]
struct Cli {
    #[pound(subcommand)]
    command: Command,
}

#[test]
fn flattened_options_parse_into_their_own_type() {
    let parsed = Cli::try_parse_from(["scan", "--limit", "25", "--top", "3"]).unwrap();
    assert_eq!(
        parsed,
        Cli {
            command: Command::Scan {
                shared: Shared {
                    min_count: 8,
                    limit: 25,
                },
                top: 3,
            },
        }
    );
}

#[test]
fn flattened_options_are_present_in_help() {
    let Error::Help(help) = Cli::try_parse_from(["scan", "--help"]).unwrap_err() else {
        panic!("expected help");
    };
    assert!(help.contains("--min-count"));
    assert!(help.contains("--limit"));
    assert!(help.contains("--top"));
}

#[test]
fn introspection_traverses_flattened_options() {
    let scan = Cli::SPEC
        .subs
        .iter()
        .find(|sub| sub.name == "scan")
        .expect("scan command")
        .spec;
    let longs = scan
        .arguments()
        .filter_map(|argument| argument.long)
        .collect::<Vec<_>>();

    assert_eq!(longs, ["min-count", "limit", "top"]);
    assert_eq!(scan.find_long("limit").unwrap().long, Some("limit"));
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct PositionalMiddle {
    second: String,
    third: String,
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct InterleavedPositionals {
    first: String,
    #[pound(flatten)]
    middle: PositionalMiddle,
    fourth: String,
}

#[test]
fn direct_and_flattened_positionals_follow_declaration_order() {
    let parsed = InterleavedPositionals::try_parse_from(["one", "two", "three", "four"])
        .expect("interleaved positionals should parse");

    assert_eq!(
        parsed,
        InterleavedPositionals {
            first: "one".to_owned(),
            middle: PositionalMiddle {
                second: "two".to_owned(),
                third: "three".to_owned(),
            },
            fourth: "four".to_owned(),
        }
    );

    let names = InterleavedPositionals::SPEC
        .arguments()
        .map(|argument| argument.value_name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["first", "second", "third", "fourth"]);
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct BuiltinAliases {
    #[pound(long, alias = "help")]
    assistance: bool,
    #[pound(flatten)]
    release: ReleaseAlias,
}

#[derive(Debug, Parse, PartialEq, Eq)]
struct ReleaseAlias {
    #[pound(long, alias = "version")]
    release_information: bool,
}

#[test]
fn direct_and_flattened_aliases_override_builtin_long_names() {
    assert_eq!(
        BuiltinAliases::try_parse_from(["--help"]).unwrap(),
        BuiltinAliases {
            assistance: true,
            release: ReleaseAlias {
                release_information: false,
            },
        }
    );
    assert_eq!(
        BuiltinAliases::try_parse_from(["--version"]).unwrap(),
        BuiltinAliases {
            assistance: false,
            release: ReleaseAlias {
                release_information: true,
            },
        }
    );
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
struct DuplicateAlias {
    #[pound(long, alias = "shared")]
    direct: bool,
    #[pound(flatten)]
    nested: DuplicateAliasNested,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct DuplicateAliasNested {
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
fn duplicate_names_across_flattened_groups_are_rejected() {
    for result in [
        DuplicateLong::try_parse_from([]).map(|_| ()),
        DuplicateAlias::try_parse_from([]).map(|_| ()),
        DuplicateShort::try_parse_from([]).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(Error::InvalidSpecification(_))),
            "expected an invalid-specification error, got {result:?}"
        );
    }
}
