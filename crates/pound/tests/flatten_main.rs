#![cfg(feature = "derive")]
#![allow(dead_code)]

use pound::{
    Error,
    ErrorKind,
    Parse,
};

#[derive(Debug, Parse)]
#[pound(name = "flatten-main")]
struct GlobalRoot {
    #[pound(flatten)]
    settings: Settings,
    #[pound(subcommand)]
    command:  Command,
}

#[derive(Debug, Parse)]
struct Settings {
    #[pound(flatten)]
    storage: Storage,
    #[pound(
        short = 'C',
        long,
        global,
        default = "auto",
        default_missing = "always"
    )]
    color:   String,
}

#[derive(Debug, Parse)]
struct Storage {
    #[pound(
        short = 'c',
        long,
        global,
        negate,
        default = "true",
        heading = "Storage",
        help = "Use cached data",
        long_help = "Keep cached data across runs"
    )]
    cache: bool,
}

#[derive(Debug, Parse)]
enum Command {
    Run,
    Cache {
        #[pound(subcommand)]
        command: Leaf,
    },
}

#[derive(Debug, Parse)]
enum Leaf {
    Run,
}

#[test]
fn nested_flattened_global_negation_uses_last_spelling() {
    for (args, expected) in [
        (vec!["cache", "run"], true),
        (vec!["--no-cache", "cache", "run"], false),
        (vec!["--cache", "cache", "run", "--no-cache"], false),
        (vec!["--no-cache", "cache", "run", "-c"], true),
    ] {
        let cli = GlobalRoot::try_parse_from(args.iter().copied()).unwrap();
        assert_eq!(cli.settings.storage.cache, expected, "{args:?}");
    }
}

#[test]
fn flattened_default_missing_preserves_subcommand_tokens() {
    for (args, expected) in [
        (vec!["cache", "run"], "auto"),
        (vec!["--color", "cache", "run"], "always"),
        (vec!["cache", "--color", "run"], "always"),
        (vec!["cache", "run", "--color=never"], "never"),
    ] {
        let cli = GlobalRoot::try_parse_from(args.iter().copied()).unwrap();
        assert_eq!(cli.settings.color, expected, "{args:?}");
        assert!(matches!(cli.command, Command::Cache { command: Leaf::Run }));
    }
}

#[derive(Debug, Parse)]
struct Constraints {
    #[pound(long, requires = "token")]
    publish: bool,
    #[pound(long)]
    token:   Option<String>,
    #[pound(long, min_values = 1, max_values = 2)]
    tag:     Vec<String>,
}

#[derive(Debug, Parse)]
#[pound(name = "constraints-root")]
struct RootConstraints {
    #[pound(flatten)]
    constraints: Constraints,
}

#[derive(Debug, Parse)]
enum ChildConstraints {
    Validate {
        #[pound(flatten)]
        constraints: Constraints,
    },
}

#[test]
fn flattened_constraints_keep_the_owning_command_usage() {
    for (args, expected) in [
        (vec!["--tag", "one", "--publish"], ErrorKind::Requires {
            arg:   "--publish".into(),
            needs: "--token".into(),
        }),
        (vec![], ErrorKind::TooFewValues {
            arg: "--tag".into(),
            min: 1,
            got: 0,
        }),
        (
            vec!["--tag", "one", "--tag", "two", "--tag", "three"],
            ErrorKind::TooManyValues {
                arg: "--tag".into(),
                max: 2,
                got: 3,
            },
        ),
    ] {
        let error = RootConstraints::try_parse_from(args.iter().copied()).unwrap_err();
        assert_eq!(error.kind, expected, "{args:?}");
        assert_eq!(
            error.usage.as_deref().unwrap().split_whitespace().nth(1),
            Some("constraints-root")
        );

        let error = ChildConstraints::try_parse_from(
            std::iter::once("validate").chain(args.iter().copied()),
        )
        .unwrap_err();
        assert_eq!(error.kind, expected, "{args:?}");
        assert_eq!(
            error.usage.as_deref().unwrap().split_whitespace().nth(1),
            Some("validate")
        );
    }
    let args = [
        "--tag",
        "one",
        "--tag",
        "two",
        "--publish",
        "--token",
        "secret",
    ];
    assert!(RootConstraints::try_parse_from(args).is_ok());
    assert!(ChildConstraints::try_parse_from(std::iter::once("validate").chain(args)).is_ok());
}

#[derive(Debug, Parse)]
struct RequiredWorkspace {
    workspace: String,
}

#[derive(Debug, Parse)]
struct RequiredParent {
    #[pound(flatten)]
    location: RequiredWorkspace,
    #[pound(subcommand)]
    command:  Leaf,
}

#[derive(Debug, Parse)]
struct OptionalWorkspace {
    workspace: Option<String>,
}

#[derive(Debug, Parse)]
struct OptionalParent {
    #[pound(flatten)]
    location: OptionalWorkspace,
    #[pound(subcommand)]
    command:  Leaf,
}

#[derive(Debug, Parse)]
struct Workspaces {
    workspace: Vec<String>,
}

#[derive(Debug, Parse)]
struct VariadicParent {
    #[pound(flatten)]
    location: Workspaces,
    #[pound(subcommand)]
    command:  Option<Leaf>,
}

#[test]
fn flattened_parent_positionals_preserve_subcommand_dispatch() {
    let required = RequiredParent::try_parse_from(["workspace", "run"]).unwrap();
    assert_eq!(required.location.workspace, "workspace");
    assert!(matches!(required.command, Leaf::Run));

    let skipped = OptionalParent::try_parse_from(["run"]).unwrap();
    assert_eq!(skipped.location.workspace, None);
    assert!(matches!(skipped.command, Leaf::Run));

    let filled = OptionalParent::try_parse_from(["workspace", "run"]).unwrap();
    assert_eq!(filled.location.workspace.as_deref(), Some("workspace"));
    assert!(matches!(filled.command, Leaf::Run));

    let variadic = VariadicParent::try_parse_from(["workspace", "run"]).unwrap();
    assert_eq!(variadic.location.workspace, ["workspace", "run"]);
    assert!(variadic.command.is_none());
}

fn help(error: Error) -> String {
    let ErrorKind::Help(text) = error.kind else {
        panic!("expected help");
    };
    text
}

#[cfg(feature = "help")]
#[test]
fn flattened_help_exposes_new_metadata() {
    let short = help(GlobalRoot::try_parse_from(["-h"]).unwrap_err());
    let long = help(GlobalRoot::try_parse_from(["--help"]).unwrap_err());
    assert!(short.contains("Storage:"));
    assert!(long.contains("Storage:"));
    assert!(short.contains("Use cached data"));
    assert!(!short.contains("Keep cached data across runs"));
    assert!(long.contains("Keep cached data across runs"));
    assert!(!long.contains("Use cached data"));
}

#[test]
fn flattened_introspection_exposes_new_metadata() {
    let spec = GlobalRoot::SPEC;
    let cache = spec.find_negate("no-cache").unwrap();
    assert_eq!(cache.long, Some("cache"));
    assert_eq!(cache.negate, Some("no-cache"));
    assert_eq!(cache.heading, Some("Storage"));
    assert_eq!(
        cache.long_help,
        Some(if cfg!(feature = "help") {
            "Keep cached data across runs"
        } else {
            ""
        })
    );
    assert_eq!(cache.default, Some("true"));
    assert!(cache.global);
    let color = spec.find_long("color").unwrap();
    assert_eq!(color.default_missing, Some("always"));
    assert_eq!(
        spec.arguments()
            .filter_map(|arg| arg.long)
            .collect::<Vec<_>>(),
        ["cache", "color"]
    );

    let tag = RootConstraints::SPEC.find_long("tag").unwrap();
    assert_eq!(tag.min_values, Some(1));
    assert_eq!(tag.max_values, Some(2));
}

#[derive(Debug, Parse)]
struct DuplicateNegate {
    #[pound(long = "no-cache")]
    direct:  bool,
    #[pound(flatten)]
    storage: Storage,
}

#[test]
fn flattened_negation_cannot_duplicate_a_long_name() {
    let error = DuplicateNegate::try_parse_from([]).unwrap_err();
    assert!(
        matches!(error.kind, ErrorKind::InvalidSpecification(message) if message.contains("no-cache"))
    );
}

#[derive(Debug, Parse)]
struct ShortOverrides {
    #[pound(short = 'h')]
    heading: bool,
    #[pound(short = 'V')]
    verbose: bool,
}

#[derive(Debug, Parse)]
struct ShortOverrideRoot {
    #[pound(flatten)]
    options: ShortOverrides,
}

#[derive(Debug, Parse)]
struct LongOverrides {
    #[pound(long)]
    help:    bool,
    #[pound(long)]
    version: bool,
}

#[derive(Debug, Parse)]
struct LongOverrideRoot {
    #[pound(flatten)]
    options: LongOverrides,
}

#[test]
fn flattened_builtin_overrides_leave_the_other_spelling_visible() {
    let long = help(ShortOverrideRoot::try_parse_from(["--help"]).unwrap_err());
    if cfg!(feature = "help") {
        assert!(long.contains("--help"));
        assert!(long.contains("--version"));
        assert!(!long.contains("-h, --help"));
        assert!(!long.contains("-V, --version"));
    }
    assert!(ShortOverrideRoot::try_parse_from(["-h", "-V"]).is_ok());
    assert!(matches!(
        ShortOverrideRoot::try_parse_from(["--version"])
            .unwrap_err()
            .kind,
        ErrorKind::Version(_)
    ));

    let short = help(LongOverrideRoot::try_parse_from(["-h"]).unwrap_err());
    if cfg!(feature = "help") {
        assert!(
            short
                .lines()
                .any(|line| line.trim_start().starts_with("-h "))
        );
        assert!(
            short
                .lines()
                .any(|line| line.trim_start().starts_with("-V "))
        );
        assert!(!short.contains("-h, --help"));
        assert!(!short.contains("-V, --version"));
    }
    assert!(LongOverrideRoot::try_parse_from(["--help", "--version"]).is_ok());
    assert!(matches!(
        LongOverrideRoot::try_parse_from(["-V"]).unwrap_err().kind,
        ErrorKind::Version(_)
    ));
}
