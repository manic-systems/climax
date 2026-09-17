#![cfg(feature = "derive")]

use pound::{
    CommandSpec,
    Error,
    ErrorKind,
    Matches,
    Parse,
    SubSpec,
};

#[derive(Debug, PartialEq, Eq, Parse)]
enum CacheCommands {
    #[pound(alias = "ls")]
    List,
    Clean {
        #[pound(long)]
        all: bool,
    },
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct CacheOptions {
    #[pound(long)]
    directory: Option<String>,
    #[pound(subcommand)]
    command:   CacheCommands,
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum NestedFlatten {
    Cache {
        #[pound(flatten)]
        options: CacheOptions,
    },
}

#[test]
fn a_named_variant_can_flatten_its_own_subcommand_selector() {
    assert_eq!(
        NestedFlatten::try_parse_from(["cache", "--directory", "tmp", "clean", "--all"]).unwrap(),
        NestedFlatten::Cache {
            options: CacheOptions {
                directory: Some("tmp".to_owned()),
                command:   CacheCommands::Clean { all: true },
            },
        }
    );
    assert_eq!(
        NestedFlatten::SPEC
            .find_sub("cache")
            .unwrap()
            .spec
            .subcommands()
            .map(|sub| sub.name)
            .collect::<Vec<_>>(),
        ["list", "clean"]
    );
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum ProjectCommands {
    #[pound(alias = "b")]
    Build {
        #[pound(long)]
        release: bool,
    },
    Cache {
        #[pound(subcommand)]
        command: CacheCommands,
    },
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum AccountCommands {
    Login,
    #[pound(hidden, alias = "secret")]
    Internal,
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum Families {
    #[pound(flatten)]
    Project(ProjectCommands),
    #[pound(flatten)]
    Account(AccountCommands),
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum Commands {
    Status,
    #[pound(flatten)]
    Families(Families),
    Echo {
        text: String,
    },
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct Shared {
    #[pound(long, global, negate)]
    verbose: bool,
    #[pound(subcommand)]
    command: Commands,
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct Middle {
    #[pound(flatten)]
    shared: Shared,
}

#[derive(Debug, PartialEq, Eq, Parse)]
#[pound(name = "flatten-demo")]
struct Cli {
    #[pound(flatten)]
    middle: Middle,
}

#[test]
fn recursive_flatten_routes_each_family_into_its_rust_variant() {
    for (args, command) in [
        (vec!["status"], Commands::Status),
        (
            vec!["b", "--release"],
            Commands::Families(Families::Project(ProjectCommands::Build { release: true })),
        ),
        (
            vec!["cache", "clean", "--all"],
            Commands::Families(Families::Project(ProjectCommands::Cache {
                command: CacheCommands::Clean { all: true },
            })),
        ),
        (
            vec!["secret"],
            Commands::Families(Families::Account(AccountCommands::Internal)),
        ),
        (vec!["echo", "hello"], Commands::Echo {
            text: "hello".to_owned(),
        }),
    ] {
        assert_eq!(Cli::try_parse_from(args).unwrap(), Cli {
            middle: Middle {
                shared: Shared {
                    verbose: false,
                    command
                },
            },
        });
    }
}

#[test]
fn globals_reach_nested_commands_and_child_flags_keep_their_scope() {
    let parsed =
        Cli::try_parse_from(["--verbose", "cache", "clean", "--all", "--no-verbose"]).unwrap();
    assert!(!parsed.middle.shared.verbose);
    assert!(Cli::try_parse_from(["--all", "cache", "clean"]).is_err());
}

#[test]
fn introspection_exposes_effective_order_aliases_and_nested_tree() {
    let spec = Cli::SPEC;
    assert!(!spec.subcommand_optional());
    assert_eq!(
        spec.subcommands().map(|sub| sub.name).collect::<Vec<_>>(),
        ["status", "build", "cache", "login", "internal", "echo"]
    );
    assert_eq!(spec.find_sub("b").unwrap().name, "build");
    assert!(spec.find_sub("secret").unwrap().hidden);
    assert!(spec.find_sub("families").is_none());
    assert!(spec.find_sub("project").is_none());
    assert_eq!(
        spec.find_sub("cache")
            .unwrap()
            .spec
            .find_sub("ls")
            .unwrap()
            .name,
        "list"
    );
}

#[cfg(feature = "help")]
#[test]
fn required_selection_uses_containing_command_help_and_hides_hidden_commands() {
    let ErrorKind::Help(help) = Cli::try_parse_from([]).unwrap_err().kind else {
        panic!("expected containing command help");
    };
    assert!(help.contains("flatten-demo"), "{help}");
    assert!(!help.contains("internal"), "{help}");
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct OptionalShared {
    #[pound(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct OptionalCli {
    #[pound(flatten)]
    shared: OptionalShared,
}

#[test]
fn optional_flattened_selector_can_be_absent_or_selected() {
    assert!(OptionalCli::SPEC.subcommand_optional());
    assert_eq!(
        OptionalCli::try_parse_from([]).unwrap().shared.command,
        None
    );
    assert_eq!(
        OptionalCli::try_parse_from(["status"])
            .unwrap()
            .shared
            .command,
        Some(Commands::Status)
    );
    let help = OptionalCli::try_parse_from(["--help"])
        .unwrap_err()
        .to_string();
    assert!(help.contains("[COMMAND]"), "{help}");
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct WorkspaceCli {
    workspace: String,
    #[pound(flatten)]
    shared:    OptionalShared,
}

#[test]
fn finite_parent_positionals_precede_dispatch_and_delimiter_disables_dispatch() {
    let parsed = WorkspaceCli::try_parse_from(["workspace", "status"]).unwrap();
    assert_eq!(parsed.workspace, "workspace");
    assert_eq!(parsed.shared.command, Some(Commands::Status));
    let parsed = WorkspaceCli::try_parse_from(["--", "status"]).unwrap();
    assert_eq!(parsed.workspace, "status");
    assert_eq!(parsed.shared.command, None);
    assert!(WorkspaceCli::try_parse_from(["workspace", "--", "status"]).is_err());
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct CompetingDirect {
    #[pound(subcommand)]
    command: Commands,
    #[pound(flatten)]
    shared:  OptionalShared,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct CompetingFlattened {
    #[pound(flatten)]
    first:  OptionalShared,
    #[pound(flatten)]
    second: OptionalShared,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
enum DuplicateCommands {
    #[pound(flatten)]
    Family(AccountCommands),
    Login,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
enum DuplicateAlias {
    #[pound(flatten)]
    Family(AccountCommands),
    #[pound(alias = "secret")]
    Other,
}

#[test]
fn competing_selectors_and_effective_name_collisions_are_invalid() {
    for result in [
        CompetingDirect::try_parse_from(["status"]).map(|_| ()),
        CompetingFlattened::try_parse_from(["status"]).map(|_| ()),
        DuplicateCommands::try_parse_from(["login"]).map(|_| ()),
        DuplicateAlias::try_parse_from(["other"]).map(|_| ()),
    ] {
        assert!(
            matches!(&result, Err(error) if matches!(error.kind, ErrorKind::InvalidSpecification(_))),
            "{result:?}"
        );
    }
}

#[derive(Debug)]
struct ManualCollision;

impl Parse for ManualCollision {
    const SPEC: &'static CommandSpec = &CommandSpec::new("manual").subs(&[
        SubSpec::new(
            "",
            &CommandSpec::new("family").subs(&[SubSpec::new("first", &CommandSpec::new("first"))
                .aliases(&["collision"])
                .hidden()]),
        )
        .flattened(),
        SubSpec::new("collision", &CommandSpec::new("collision")),
    ]);

    fn from_matches(_: &'static CommandSpec, _: &Matches<'_>) -> Result<Self, Error> {
        Ok(Self)
    }
}

#[test]
fn manually_built_flattened_alias_collision_is_rejected_before_dispatch() {
    assert!(matches!(
        ManualCollision::try_parse_from(["first"]).unwrap_err().kind,
        ErrorKind::InvalidSpecification(message) if message.contains("collision")
    ));
}

macro_rules! invalid_manual_specs {
    ($($name:ident => $spec:expr),+ $(,)?) => {
        $(
            #[derive(Debug)]
            struct $name;

            impl Parse for $name {
                const SPEC: &'static CommandSpec = &$spec;

                fn from_matches(_: &'static CommandSpec, _: &Matches<'_>) -> Result<Self, Error> {
                    panic!("invalid specifications must fail before constructing a value")
                }
            }
        )+

        #[test]
        fn invalid_flattening_specs_are_rejected_before_parsing() {
            $(
                let error = $name::try_parse_from(["run"]).unwrap_err();
                assert!(matches!(error.kind, ErrorKind::InvalidSpecification(_)),
                    "{} returned {error:?}", stringify!($name));
            )+
        }
    };
}

const FAMILY: CommandSpec =
    CommandSpec::new("family").subs(&[SubSpec::new("run", &CommandSpec::new("run"))]);

static CYCLE: CommandSpec = CommandSpec::new("cycle").flattened(&[&CYCLE]);
static CYCLE_REFS: [&CommandSpec; 1] = [&CYCLE];

invalid_manual_specs! {
    NamedWrapper => CommandSpec::new("root").subs(&[SubSpec::new("named", &FAMILY).flattened()]),
    EmptyWrapper => CommandSpec::new("root").subs(&[SubSpec::new("", &CommandSpec::new("empty")).flattened()]),
    WrapperArguments => CommandSpec::new("root").subs(&[SubSpec::new("", &FAMILY.args(&[
        pound::ArgSpec::new(pound::Kind::Flag).long("flag"),
    ])).flattened()]),
    CyclicFlatten => CommandSpec::new("root").flattened(&CYCLE_REFS),
}

#[test]
fn unknown_flattened_command_suggests_an_alias_and_uses_root_usage() {
    let error = Cli::try_parse_from(["bb"]).unwrap_err();
    assert!(
        matches!(&error.kind,
        ErrorKind::UnknownSubcommand { name, closest: Some(closest) }
        if name == "bb" && closest == "b"),
        "{error:?}"
    );
    assert!(
        error
            .usage
            .as_deref()
            .unwrap()
            .starts_with("Usage: flatten-demo ")
    );
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct RequiredOptions {
    #[pound(long)]
    workspace: String,
    #[pound(subcommand)]
    command:   Commands,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
#[pound(name = "constrained-root")]
struct RequiredRoot {
    #[pound(flatten)]
    options: RequiredOptions,
}

#[test]
fn selected_flattened_command_still_validates_its_parent_options() {
    let error = RequiredRoot::try_parse_from(["status"]).unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::MissingRequired("--workspace".to_owned())
    );
    assert!(
        error
            .usage
            .as_deref()
            .unwrap()
            .starts_with("Usage: constrained-root ")
    );
}
