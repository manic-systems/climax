#![cfg(feature = "derive")]

use pound::{CommandSpec, Error, ErrorKind, Matches, Parse};

#[derive(Debug, PartialEq, Eq, Parse)]
enum CacheCommand {
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
    command:   CacheCommand,
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum ProjectCommand {
    Cache {
        #[pound(flatten)]
        options: CacheOptions,
    },
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum AccountCommand {
    Login,
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum Family {
    #[pound(flatten)]
    Project(ProjectCommand),
    #[pound(flatten)]
    Account(AccountCommand),
}

#[derive(Debug, PartialEq, Eq, Parse)]
enum Command {
    #[pound(flatten)]
    Family(Family),
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct Shared {
    #[pound(long, global, negate)]
    verbose: bool,
    #[pound(subcommand)]
    command: Command,
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct Cli {
    #[pound(flatten)]
    shared: Shared,
}

#[test]
fn recursive_flattening_routes_nested_values_and_globals() {
    let parsed = Cli::try_parse_from([
        "--verbose",
        "cache",
        "--directory",
        "tmp",
        "clean",
        "--all",
        "--no-verbose",
    ])
    .unwrap();

    assert_eq!(parsed, Cli {
        shared: Shared {
            verbose: false,
            command: Command::Family(Family::Project(ProjectCommand::Cache {
                options: CacheOptions {
                    directory: Some("tmp".into()),
                    command:   CacheCommand::Clean { all: true },
                },
            })),
        },
    });
    assert!(matches!(
        Cli::try_parse_from(["login"]).unwrap().shared.command,
        Command::Family(Family::Account(AccountCommand::Login))
    ));
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct OptionalShared {
    #[pound(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, PartialEq, Eq, Parse)]
struct WorkspaceCli {
    workspace: String,
    #[pound(flatten)]
    shared:    OptionalShared,
}

#[test]
fn parent_positionals_and_delimiters_control_flattened_dispatch() {
    let parsed = WorkspaceCli::try_parse_from(["workspace", "login"]).unwrap();
    assert_eq!(parsed.workspace, "workspace");
    assert!(matches!(
        parsed.shared.command,
        Some(Command::Family(Family::Account(AccountCommand::Login)))
    ));

    let parsed = WorkspaceCli::try_parse_from(["--", "login"]).unwrap();
    assert_eq!(parsed.workspace, "login");
    assert_eq!(parsed.shared.command, None);
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct CompetingSelectors {
    #[pound(subcommand)]
    command: Command,
    #[pound(flatten)]
    shared:  OptionalShared,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
enum DuplicateCommand {
    #[pound(flatten)]
    Account(AccountCommand),
    Login,
}

#[derive(Debug)]
struct CyclicFlatten;

static CYCLE: CommandSpec = CommandSpec::new("cycle").flattened(&[&CYCLE]);

impl Parse for CyclicFlatten {
    const SPEC: &'static CommandSpec = &CYCLE;

    fn from_matches(_: &'static CommandSpec, _: &Matches<'_>) -> Result<Self, Error> {
        panic!("invalid specifications must fail before constructing a value")
    }
}

#[test]
fn ambiguous_and_cyclic_flattening_are_rejected() {
    for result in [
        CompetingSelectors::try_parse_from(["login"]).map(|_| ()),
        DuplicateCommand::try_parse_from(["login"]).map(|_| ()),
        CyclicFlatten::try_parse_from(["login"]).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(error) if matches!(error.kind, ErrorKind::InvalidSpecification(_))
        ));
    }
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct RequiredOptions {
    #[pound(long)]
    workspace: String,
    #[pound(subcommand)]
    command:   Command,
}

#[derive(Debug, Parse)]
#[allow(dead_code)]
struct RequiredRoot {
    #[pound(flatten)]
    options: RequiredOptions,
}

#[test]
fn selected_flattened_command_still_validates_its_parent() {
    let error = RequiredRoot::try_parse_from(["login"]).unwrap_err();
    assert_eq!(
        error.kind,
        ErrorKind::MissingRequired("--workspace".into())
    );
}
