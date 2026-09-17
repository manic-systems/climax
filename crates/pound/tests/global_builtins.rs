#![allow(dead_code)]
#![cfg(feature = "derive")]

use pound::Parse;

#[derive(Debug, Parse)]
struct Cli {
    #[pound(flatten)]
    globals: Globals,
    #[pound(subcommand)]
    command: Branch,
}

#[derive(Debug, Parse)]
struct Globals {
    #[pound(short, long, global)]
    help:    bool,
    #[pound(short = 'V', long, global)]
    version: bool,
}

#[derive(Debug, Parse)]
enum Branch {
    Nested {
        #[pound(subcommand)]
        command: Leaf,
    },
}

#[derive(Debug, Parse)]
enum Leaf {
    Run,
}

#[test]
fn flattened_globals_shadow_builtins_at_any_depth() {
    for (flag, expected) in [
        ("--help", [true, false]),
        ("-h", [true, false]),
        ("--version", [false, true]),
        ("-V", [false, true]),
    ] {
        let cli = Cli::try_parse_from(["nested", "run", flag]).unwrap();
        assert_eq!([cli.globals.help, cli.globals.version], expected);
    }
}

#[derive(Debug, Parse)]
struct ScopedGlobals {
    #[pound(long, global, negate, default = "true")]
    cache:   bool,
    #[pound(short = 'v', long, global, count)]
    verbose: u32,
}

#[derive(Debug, Parse)]
struct ScopedRoot {
    #[pound(flatten)]
    globals: ScopedGlobals,
    #[pound(subcommand)]
    command: ScopedOuter,
}

#[derive(Debug, Parse)]
enum ScopedOuter {
    Outer {
        #[pound(subcommand)]
        command: ScopedInner,
    },
}

#[derive(Debug, Parse)]
enum ScopedInner {
    Inner {
        #[pound(flatten)]
        globals: ScopedGlobals,
    },
}

#[test]
fn reused_flattened_globals_keep_their_resolved_scope() {
    let parsed = ScopedRoot::try_parse_from([
        "outer",
        "--no-cache",
        "-v",
        "inner",
        "--cache",
        "-vv",
    ])
    .unwrap();
    let ScopedOuter::Outer {
        command: ScopedInner::Inner { globals },
    } = parsed.command;

    assert!(!parsed.globals.cache);
    assert_eq!(parsed.globals.verbose, 1);
    assert!(globals.cache);
    assert_eq!(globals.verbose, 2);
}
