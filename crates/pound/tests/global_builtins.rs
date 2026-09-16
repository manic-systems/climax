#![allow(dead_code)]
#![cfg(feature = "derive")]

use pound::{
    ArgSpec,
    CommandSpec,
    Error,
    ErrorKind,
    Kind,
    Matches,
    Parse,
    SubSpec,
};

#[derive(Debug, Parse)]
struct Cli {
    #[pound(flatten)]
    shared:  Shared,
    #[pound(subcommand)]
    command: Branch,
}

#[derive(Debug, Parse)]
struct Shared {
    #[pound(flatten)]
    globals: Globals,
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

fn positions(flag: &str) -> [Vec<&str>; 3] {
    [
        vec![flag, "nested", "run"],
        vec!["nested", flag, "run"],
        vec!["nested", "run", flag],
    ]
}

#[test]
fn derived_globals_shadow_builtins_through_nested_flattening() {
    for (flag, expected) in [
        ("--help", [true, false]),
        ("-h", [true, false]),
        ("--version", [false, true]),
        ("-V", [false, true]),
    ] {
        for args in positions(flag) {
            let cli = Cli::try_parse_from(args.iter().copied()).unwrap();
            assert_eq!(
                [cli.shared.globals.help, cli.shared.globals.version],
                expected,
                "{args:?}"
            );
        }
    }
    #[cfg(feature = "help")]
    {
        let text = help(Cli::try_parse_from(["nested"]).unwrap_err());
        assert!(!text.contains("display this help and exit"));
        assert_eq!(text.matches("-h, --help").count(), 1);
        assert_eq!(text.matches("-V, --version").count(), 1);
    }
}

const DIRECT: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag).long("help").short('h').global(),
    ArgSpec::new(Kind::Flag).long("version").short('V').global(),
];
const ALIASES: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag)
        .long("assistance")
        .aliases(&["help"])
        .global(),
    ArgSpec::new(Kind::Flag)
        .long("release")
        .aliases(&["version"])
        .global(),
];
const HIDDEN: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag)
        .long("assistance")
        .aliases(&["help"])
        .short('h')
        .global()
        .hidden(),
    ArgSpec::new(Kind::Flag)
        .long("release")
        .aliases(&["version"])
        .short('V')
        .global()
        .hidden(),
];
const SHORTS: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag).short('h').global(),
    ArgSpec::new(Kind::Flag).short('V').global(),
];
const NEGATIONS: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag)
        .long("keep-help")
        .negate("help")
        .default("true")
        .global(),
    ArgSpec::new(Kind::Flag)
        .long("keep-version")
        .negate("version")
        .default("true")
        .global(),
];
const UNSHADOWED: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag).long("feature").global(),
    ArgSpec::new(Kind::Flag).long("detail").global(),
];
const RUN: CommandSpec = CommandSpec::new("run").hash("abc123");
const NESTED: CommandSpec = CommandSpec::new("nested")
    .version("2.0")
    .subs(&[SubSpec::new("run", &RUN)]);

#[derive(Debug)]
struct Manual<const MODE: usize> {
    flags: [bool; 2],
}

impl<const MODE: usize> Manual<MODE> {
    const GLOBAL_SPEC: CommandSpec = CommandSpec::new("globals").args(match MODE {
        0 => DIRECT,
        1 => ALIASES,
        2 => HIDDEN,
        3 => SHORTS,
        4 => NEGATIONS,
        _ => UNSHADOWED,
    });
    const GLOBALS: &'static [&'static CommandSpec] = &[&Self::GLOBAL_SPEC];
    const SHARED_SPEC: CommandSpec = CommandSpec::new("shared").flattened(Self::GLOBALS);
    const SHARED: &'static [&'static CommandSpec] = &[&Self::SHARED_SPEC];
    const SUBS: &'static [SubSpec] = &[SubSpec::new("nested", &NESTED)];
    const ROOT: CommandSpec = CommandSpec::new("manual")
        .version("1.0")
        .flattened(Self::SHARED)
        .subs(Self::SUBS);
}

impl<const MODE: usize> Parse for Manual<MODE> {
    const SPEC: &'static CommandSpec = &Self::ROOT;

    fn from_matches(spec: &'static CommandSpec, matches: &Matches<'_>) -> Result<Self, Error> {
        let spec = spec.flattened[0].flattened[0];
        let matches = matches.flattened(0).flattened(0);
        Ok(Self {
            flags: [matches.switch(spec, 0), matches.switch(spec, 1)],
        })
    }
}

fn help(error: Error) -> String {
    let ErrorKind::Help(text) = error.kind else {
        panic!("expected help, got {error:?}");
    };
    text
}

#[test]
fn globals_shadow_versioned_and_hash_only_descendants() {
    for (flag, expected) in [
        ("--help", [true, false]),
        ("-h", [true, false]),
        ("--version", [false, true]),
        ("-V", [false, true]),
    ] {
        let cli = Manual::<0>::try_parse_from(["nested", "run", flag]).unwrap();
        assert_eq!(cli.flags, expected, "{flag}");
    }
    #[cfg(feature = "help")]
    {
        let text = help(Manual::<0>::try_parse_from(["nested"]).unwrap_err());
        assert!(!text.contains("display this help and exit"));
        assert!(!text.contains("output version information and exit"));
        assert_eq!(text.matches("-h, --help").count(), 1);
        assert_eq!(text.matches("-V, --version").count(), 1);
    }
}

#[test]
fn global_long_aliases_shadow_builtins_independently() {
    for (flag, expected) in [("--help", [true, false]), ("--version", [false, true])] {
        assert_eq!(
            Manual::<1>::try_parse_from(["nested", "run", flag])
                .unwrap()
                .flags,
            expected,
            "{flag}"
        );
    }
    #[cfg(feature = "help")]
    {
        let text = help(Manual::<1>::try_parse_from(["nested", "run", "-h"]).unwrap_err());
        assert!(!text.contains("--help"));
        assert!(!text.contains("--version"));
        assert!(text.contains("--assistance"));
        assert!(text.contains("--release"));
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with("-h "))
        );
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with("-V "))
        );
    }
    assert_eq!(
        Manual::<1>::try_parse_from(["nested", "run", "-V"])
            .unwrap_err()
            .kind,
        ErrorKind::Version("run (abc123)".into())
    );
}

#[test]
fn hidden_globals_still_shadow_every_builtin_spelling() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let cli = Manual::<2>::try_parse_from(["nested", "run", flag]).unwrap();
        assert_eq!(cli.flags, [
            matches!(flag, "--help" | "-h"),
            matches!(flag, "--version" | "-V")
        ]);
    }
    #[cfg(feature = "help")]
    {
        let text = help(Manual::<2>::try_parse_from(["nested"]).unwrap_err());
        for hidden in [
            "--help",
            "--version",
            "--assistance",
            "--release",
            "display this help and exit",
            "output version information and exit",
        ] {
            assert!(!text.contains(hidden), "{hidden}");
        }
        assert!(!text.lines().any(|line| {
            line.trim_start().starts_with("-h ") || line.trim_start().starts_with("-V ")
        }));
    }
}

#[test]
fn global_short_flags_leave_long_builtins_visible_without_duplicate_rows() {
    for flag in ["-h", "-V"] {
        let cli = Manual::<3>::try_parse_from(["nested", "run", flag]).unwrap();
        assert_eq!(cli.flags, [flag == "-h", flag == "-V"]);
    }
    #[cfg(feature = "help")]
    {
        let text = help(Manual::<3>::try_parse_from(["nested", "run", "--help"]).unwrap_err());
        assert!(!text.contains("-h, --help"));
        assert!(!text.contains("-V, --version"));
        for flag in ["-h", "-V", "--help", "--version"] {
            assert_eq!(
                text.lines()
                    .filter(|line| line.split_whitespace().next() == Some(flag))
                    .count(),
                1,
                "{flag}"
            );
        }
    }
    assert_eq!(
        Manual::<3>::try_parse_from(["nested", "run", "--version"])
            .unwrap_err()
            .kind,
        ErrorKind::Version("run (abc123)".into())
    );
}

#[test]
fn global_negation_names_shadow_long_builtins() {
    for (flag, expected) in [("--help", [false, true]), ("--version", [true, false])] {
        assert_eq!(
            Manual::<4>::try_parse_from(["nested", "run", flag])
                .unwrap()
                .flags,
            expected,
            "{flag}"
        );
    }
    #[cfg(feature = "help")]
    {
        let text = help(Manual::<4>::try_parse_from(["nested", "run", "-h"]).unwrap_err());
        assert!(!text.contains("-h, --help"));
        assert!(!text.contains("-V, --version"));
        assert_eq!(text.matches("--help").count(), 1);
        assert_eq!(text.matches("--version").count(), 1);
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with("-h "))
        );
    }
    assert_eq!(
        Manual::<4>::try_parse_from(["nested", "run", "-V"])
            .unwrap_err()
            .kind,
        ErrorKind::Version("run (abc123)".into())
    );
}

#[test]
fn unshadowed_builtins_signal_at_every_command_depth() {
    for (path, version) in [
        (vec![], "manual 1.0"),
        (vec!["nested"], "nested 2.0"),
        (vec!["nested", "run"], "run (abc123)"),
    ] {
        for flag in ["-h", "--help"] {
            let text =
                help(Manual::<5>::try_parse_from(path.iter().copied().chain([flag])).unwrap_err());
            #[cfg(feature = "help")]
            {
                assert_eq!(text.matches("-h, --help").count(), 1);
                assert_eq!(text.matches("-V, --version").count(), 1);
            }
            #[cfg(not(feature = "help"))]
            drop(text);
        }
        for flag in ["-V", "--version"] {
            let error =
                Manual::<5>::try_parse_from(path.iter().copied().chain([flag])).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Version(version.into()));
        }
    }
}

#[derive(Debug, Parse)]
struct ReusedGlobals {
    #[pound(long, global, negate, default = "true")]
    cache:   bool,
    #[pound(short = 'v', long, global, count)]
    verbose: u32,
    #[pound(short = 'l', long, global)]
    label:   Option<String>,
}

#[derive(Debug, Parse)]
struct ReusedRoot {
    #[pound(flatten)]
    globals: ReusedGlobals,
    #[pound(subcommand)]
    command: ReusedOuter,
}

#[derive(Debug, Parse)]
enum ReusedOuter {
    Outer {
        #[pound(subcommand)]
        command: ReusedInner,
    },
}

#[derive(Debug, Parse)]
enum ReusedInner {
    Inner {
        #[pound(flatten)]
        globals: ReusedGlobals,
    },
}

#[test]
fn reused_flattened_globals_keep_the_scope_where_they_were_resolved() {
    for args in [
        vec![
            "outer",
            "--no-cache",
            "--verbose",
            "--label=root",
            "inner",
            "--cache",
            "--verbose",
            "--verbose",
            "--label=child",
        ],
        vec![
            "outer",
            "--no-cache",
            "-v",
            "-lroot",
            "inner",
            "--cache",
            "-vv",
            "-lchild",
        ],
    ] {
        let parsed = ReusedRoot::try_parse_from(args.iter().copied()).unwrap();
        let ReusedOuter::Outer {
            command: ReusedInner::Inner { globals },
        } = parsed.command;
        assert!(!parsed.globals.cache, "{args:?}");
        assert_eq!(parsed.globals.verbose, 1, "{args:?}");
        assert_eq!(parsed.globals.label.as_deref(), Some("root"), "{args:?}");
        assert!(globals.cache, "{args:?}");
        assert_eq!(globals.verbose, 2, "{args:?}");
        assert_eq!(globals.label.as_deref(), Some("child"), "{args:?}");
    }
}

#[derive(Debug, Parse)]
struct ShadowedRoot {
    #[pound(long, short = 'm', alias = "selection", global)]
    mode:    Option<String>,
    #[pound(long, global)]
    disable: bool,
    #[pound(long, global, negate = "off", default = "true")]
    active:  bool,
    #[pound(subcommand)]
    command: ShadowedMiddle,
}

#[derive(Debug, Parse)]
enum ShadowedMiddle {
    Middle {
        #[pound(long, short = 'm', alias = "selection", global)]
        mode:    Option<String>,
        #[pound(subcommand)]
        command: ShadowedInner,
    },
}

#[derive(Debug, Parse)]
enum ShadowedInner {
    Inner {
        #[pound(long, short = 'm', alias = "selection", global)]
        mode:    Option<String>,
        #[pound(long, global, negate = "disable", default = "true")]
        enabled: bool,
        #[pound(long, global)]
        off:     bool,
        #[pound(subcommand)]
        command: Leaf,
    },
}

#[test]
fn nearest_ancestor_owns_shadowed_global_longs_aliases_and_shorts() {
    for flag in ["--mode=inner", "--selection=inner", "-minner"] {
        for tail in [vec![flag, "run"], vec!["run", flag]] {
            let args = ["--mode=root", "middle", "--mode=middle", "inner"]
                .into_iter()
                .chain(tail.iter().copied());
            let parsed = ShadowedRoot::try_parse_from(args).unwrap();
            let ShadowedMiddle::Middle {
                mode: middle,
                command: ShadowedInner::Inner { mode: inner, .. },
            } = parsed.command;
            assert_eq!(parsed.mode.as_deref(), Some("root"), "{tail:?}");
            assert_eq!(middle.as_deref(), Some("middle"), "{tail:?}");
            assert_eq!(inner.as_deref(), Some("inner"), "{tail:?}");
        }
    }
}

#[test]
fn nearest_ancestor_wins_even_when_a_spelling_changes_its_negation_role() {
    for tail in [vec!["--disable", "--off", "run"], vec![
        "run",
        "--disable",
        "--off",
    ]] {
        let parsed = ShadowedRoot::try_parse_from(
            ["middle", "inner"].into_iter().chain(tail.iter().copied()),
        )
        .unwrap();
        let ShadowedMiddle::Middle {
            command: ShadowedInner::Inner { enabled, off, .. },
            ..
        } = parsed.command;
        assert!(!parsed.disable, "{tail:?}");
        assert!(parsed.active, "{tail:?}");
        assert!(!enabled, "{tail:?}");
        assert!(off, "{tail:?}");
    }
    let error =
        ShadowedRoot::try_parse_from(["middle", "inner", "run", "--disable=true"]).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnexpectedValue("--disable".into()));
}
