flattening reuses arguments and command families without adding names for their
rust wrappers

structs contribute their arguments at the containing command level

flattened enum variants contribute their commands at the containing command
level

```rust
use pound::Parse;

#[derive(Parse)]
struct Shared {
    #[pound(long, global)]
    verbose: bool,
    #[pound(subcommand)]
    command: Commands,
}

#[derive(Parse)]
struct Cli {
    #[pound(flatten)]
    shared: Shared,
}

#[derive(Parse)]
enum Commands {
    #[pound(flatten)]
    Project(ProjectCommands),
    #[pound(flatten)]
    Account(AccountCommands),
}

#[derive(Parse)]
enum ProjectCommands {
    Build,
}

#[derive(Parse)]
enum AccountCommands {
    #[pound(alias = "signin")]
    Login,
}

let cli = Cli::try_parse_from(["signin", "--verbose"]).unwrap();
assert!(cli.shared.verbose);
assert!(matches!(
    cli.shared.command,
    Commands::Account(AccountCommands::Login),
));
assert_eq!(
    Cli::SPEC.subcommands().map(|sub| sub.name).collect::<Vec<_>>(),
    ["build", "login"],
);
```

the user types `signin` rather than `account signin`

flattening does not make arguments global

one subcommand selector is allowed at each command level

`Option<Commands>` makes selection optional

duplicate command names and aliases are rejected before dispatch

`CommandSpec::subcommands()` yields effective commands in declaration order
while raw `subs` retains the rust routing wrappers
