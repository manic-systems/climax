Flattening composes reusable arguments and command families without adding command
names for their Rust wrappers.

Struct flattening is recursive. Each flattened struct contributes its arguments
to the containing command level, in declaration order. Its subcommand selector
also belongs to that level. Explicit subcommands still introduce a new level
and preserve their own switches and nested commands.

```rust
use pound::Parse;

#[derive(Parse)]
struct Logging {
    #[pound(long, global)]
    verbose: bool,
}

#[derive(Parse)]
struct Shared {
    #[pound(flatten)]
    logging: Logging,
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
    Cache {
        #[pound(subcommand)]
        command: CacheCommands,
    },
}

#[derive(Parse)]
enum CacheCommands {
    Clean {
        #[pound(long)]
        all: bool,
    },
}

#[derive(Parse)]
enum AccountCommands {
    #[pound(alias = "signin")]
    Login,
}

let cli = Cli::try_parse_from(["cache", "clean", "--all", "--verbose"]).unwrap();
assert!(cli.shared.logging.verbose);
assert!(matches!(
    cli.shared.command,
    Commands::Project(ProjectCommands::Cache {
        command: CacheCommands::Clean { all: true },
    })
));

let cli = Cli::try_parse_from(["signin"]).unwrap();
assert!(matches!(cli.shared.command, Commands::Account(AccountCommands::Login)));
assert_eq!(
    Cli::SPEC.subcommands().map(|sub| sub.name).collect::<Vec<_>>(),
    ["build", "cache", "login"],
);
assert_eq!(Cli::SPEC.find_sub("signin").unwrap().name, "login");
```

The user types `cache clean`, never `shared project cache clean`. Application
code retains the `Shared` struct and `Commands::Project` wrapper. A flattened
enum variant must contain exactly one command enum as a tuple field. Derived
command enums implement `Subcommands`.

Flattening does not make arguments global. Here `--verbose` works after `clean`
because it explicitly declares `global`. The `--all` switch belongs only to
`clean`. Required arguments contributed by flattened structs apply to every
command choice at their containing level.

A flattened field only accepts `#[pound(flatten)]`. Declare argument
metadata such as `global`, `hidden`, and `conflicts_with` on the fields inside
the contributed struct. Conflicts and requirements refer to fields in that
same struct.

```compile_fail
#[derive(pound::Parse)]
struct Shared {
    #[pound(long)]
    danger: bool,
}

#[derive(pound::Parse)]
struct Cli {
    #[pound(flatten, conflicts_with = "safe")]
    shared: Shared,
    #[pound(long)]
    safe: bool,
}
```

Unknown metadata on a flattened field is also rejected.

```compile_fail
#[derive(pound::Parse)]
struct Shared {
    #[pound(long)]
    verbose: bool,
}

#[derive(pound::Parse)]
struct Cli {
    #[pound(flatten, globall)]
    shared: Shared,
}
```

One selector is allowed at each command level, including flattened structs.
Use flattened enum variants to combine command families. `Option<Commands>`
permits omission, otherwise a missing command returns help.

A named field can also flatten a command enum directly. Its commands belong to
the containing level and selection is required. Use
`#[pound(subcommand)] command: Option<Commands>` when selection should be
optional. Both forms count toward the same one-selector limit.

```rust
use pound::Parse;

#[derive(Debug, PartialEq, Parse)]
enum Commands {
    Run {
        #[pound(long)]
        verbose: bool,
    },
}

#[derive(Debug, Parse)]
struct Cli {
    #[pound(flatten)]
    command: Commands,
}

let cli = Cli::try_parse_from(["run", "--verbose"]).unwrap();
assert_eq!(cli.command, Commands::Run { verbose: true });
assert!(matches!(
    Cli::try_parse_from([]).unwrap_err().kind,
    pound::ErrorKind::Help(_),
));
assert_eq!(Cli::SPEC.find_sub("run").unwrap().name, "run");
```

Duplicate command names and aliases are rejected during parsing, including
collisions involving hidden commands. Flattening merges distinct commands and
does not merge two identically named subtrees.

Competing selectors are rejected before parsing, including manually built
specs.

`CommandSpec::subcommands()` yields effective commands in declaration order.
`find_sub()` resolves names and aliases. Raw `subs` preserves enum routing
wrappers and excludes selectors inherited through structs.

Invalid declarations fail to compile. A flattened variant cannot be a unit
variant or have zero or multiple tuple fields.

```compile_fail
#[derive(pound::Parse)]
enum Commands {
    #[pound(flatten)]
    Group,
}
```

An argument struct cannot supply a flattened enum variant.

```compile_fail
#[derive(pound::Parse)]
struct Arguments {
    #[pound(long)]
    verbose: bool,
}

#[derive(pound::Parse)]
enum Commands {
    #[pound(flatten)]
    Group(Arguments),
}
```

Two direct selectors cannot represent a singular command choice.

```compile_fail
#[derive(pound::Parse)]
enum Commands {
    Run,
}

#[derive(pound::Parse)]
struct Cli {
    #[pound(subcommand)]
    first: Commands,
    #[pound(subcommand)]
    second: Commands,
}
```

A flattened wrapper cannot add its own command name or other Pound metadata.
Put names, aliases, and visibility on the contributed commands instead.

```compile_fail
#[derive(pound::Parse)]
enum Inner {
    Run,
}

#[derive(pound::Parse)]
enum Commands {
    #[pound(flatten, name = "group")]
    Group(Inner),
}
```
