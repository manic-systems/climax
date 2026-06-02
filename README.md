# pound

a low-footprint, derive-first cli parser for rust.

the derive emits a flat `&'static` description of your command and one non-generic
engine interprets it — so you get familiar derive ergonomics, almost no compile-time
overhead, and nothing pulled in at runtime.

## install

```toml
[dependencies]
pound = "0.1"
```

## the basics

field shape carries meaning, so most fields need no attribute:

| field type  | meaning                   |
|-------------|---------------------------|
| `bool`      | flag — present means true |
| `T`         | required positional       |
| `Option<T>` | optional positional       |
| `Vec<T>`    | repeatable / variadic     |

`#[pound(short)]` and `#[pound(long)]` promote any of these to a named option.

```rust,ignore
use pound::Parse;

/// fetch urls to disk
#[derive(Parse)]
#[pound(name = "grab", version = "0.1.0")]
struct Grab {
    url: Vec<String>,                    // variadic positional

    /// write downloads here
    #[pound(short, long)]
    output: Option<String>,              // -o / --output <path>

    /// overwrite existing files
    #[pound(short, long)]
    force: bool,                         // -f / --force

    /// increase verbosity, pass multiple times
    #[pound(short, long, count)]
    verbose: u8,                         // -v / -vvv / --verbose

    /// parallel jobs
    #[pound(short, long, default = "4")]
    jobs: u32,                           // --jobs <n>  (default: 4)
}

fn main() {
    let grab = Grab::parse(); // exits and prints help on -h/--help or a parse error
    println!("{grab:?}");
}
```

## subcommands

### enum as the top-level command

derive `Parse` on an enum and each variant becomes a subcommand. struct variants
carry the subcommand's own flags and positionals:

```rust,ignore
use pound::Parse;

/// a small package manager
#[derive(Parse)]
#[pound(name = "pkg", version = "1.0.0")]
enum Pkg {
    /// initialise a project
    Init {
        #[pound(short, long)]
        force: bool,
    },
    /// add a dependency
    Add {
        name: String,            // required positional
        url:  String,            // required positional
        #[pound(short, long)]
        force: bool,
    },
}

fn main() {
    match Pkg::parse() {
        Pkg::Init { force }         => { /* ... */ }
        Pkg::Add { name, url, force } => { /* ... */ }
    }
}
```

```
pkg init --force
pkg add serde https://crates.io/crates/serde -f
```

### global options + subcommand field

a struct can carry global flags and delegate the rest of the command line to a
subcommand enum via `#[pound(subcommand)]`:

```rust,ignore
#[derive(Parse)]
enum Action {
    Build { #[pound(short, long)] release: bool },
    Clean,
    Test,
}

#[derive(Parse)]
#[pound(name = "tool", version = "0.1.0")]
struct Cli {
    #[pound(short, long)]
    verbose: bool,

    #[pound(long, default = "info")]
    log: String,

    #[pound(subcommand)]
    action: Action,         // required — shows help if absent
}

fn main() {
    let cli = Cli::parse();
}
```

```
tool --verbose build --release
tool --log debug clean
```

make the subcommand optional with `Option<T>`:

```rust,ignore
#[derive(Parse)]
#[pound(name = "maybe")]
struct Cli {
    #[pound(short, long)]
    force: bool,
    #[pound(subcommand)]
    action: Option<Action>,   // ok to omit entirely
}
```

### nested subcommands

enum variants can themselves carry a `#[pound(subcommand)]` field, nesting as
deep as you need:

```rust,ignore
#[derive(Parse)]
enum LeaseAction { Open, Close }

#[derive(Parse)]
#[pound(name = "cade")]
enum Cade {
    Lease {
        #[pound(subcommand)]
        action: LeaseAction,   // cade lease open | cade lease close
    },
    Status,
}
```

### hidden subcommands

annotate a variant with `#[pound(hidden)]` to accept it without listing it in help:

```rust,ignore
#[derive(Parse)]
#[pound(name = "svc")]
enum Svc {
    Run,
    #[pound(hidden)]
    Internal,   // parses fine, invisible in --help
}
```

## value enums

`#[derive(ValueEnum)]` turns a unit enum into a `FromArg` type. variants are
accepted as kebab-case strings and the valid choices appear automatically in help
text and error messages:

```rust,ignore
use pound::{Parse, ValueEnum};

#[derive(ValueEnum)]
enum Level { Quiet, Normal, Trace }

#[derive(Parse)]
#[pound(name = "run")]
struct Run {
    #[pound(long)]
    level: Level,   // --level quiet|normal|trace
}
```

```
$ run --level bogus
error: invalid value 'bogus' for --level [possible values: quiet, normal, trace]
```

## mutually exclusive options

`group = "name"` puts flags into a named set. by default the group is optional
(at most one). add `required_group = "name"` at the item level to require exactly one:

```rust,ignore
#[derive(Parse)]
#[pound(name = "pick", required_group = "speed")]
struct Pick {
    #[pound(long, group = "speed")] fast: bool,
    #[pound(long, group = "speed")] slow: bool,
}
```

```
pick --fast          ✓
pick                 ✗  error: one of --fast / --slow is required
pick --fast --slow   ✗  error: --fast conflicts with --slow
```

## pairwise conflicts

for a one-off conflict without a named group, use `conflicts_with = "field"`:

```rust,ignore
#[derive(Parse)]
#[pound(name = "log")]
struct Log {
    #[pound(long)]
    quiet: bool,
    #[pound(long, conflicts_with = "quiet")]
    verbose: bool,
}
```

## trailing arguments

`#[pound(trailing)]` collects everything after `--` into a `Vec<String>`:

```rust,ignore
#[derive(Parse)]
#[pound(name = "sandbox")]
struct Sandbox {
    #[pound(short, long)]
    sockets: bool,
    #[pound(trailing)]
    exec: Vec<String>,   // sandbox -- ls -la
}
```

## custom value types

implement `FromArg` for any type you want to parse directly from the command line:

```rust,ignore
use pound::{FromArg, ValueError};

struct Rgb(u8, u8, u8);

impl FromArg for Rgb {
    fn from_arg(s: &str) -> Result<Self, ValueError> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 {
            return Err(ValueError::new(s, "expected a 6-digit hex colour"));
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|e| ValueError::new(s, e));
        Ok(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }
}
```

## attribute reference

### item attributes (struct or enum)

| attribute              | meaning                                                                    |
|------------------------|----------------------------------------------------------------------------|
| `name = "str"`         | command name in help/usage (defaults to the type name, lowercased)         |
| `version = "str"`      | version shown by `-V` / `--version`                                        |
| `required_group = "g"` | exactly one flag in group `g` must be provided                             |

### field attributes

| attribute              | meaning                                                                    |
|------------------------|----------------------------------------------------------------------------|
| `short`                | short flag (`-f` from field name, or `= 'x'` to override)                 |
| `long`                 | long flag (`--field-name`, or `= "name"` to override)                     |
| `positional`           | force positional parsing (usually inferred from the type)                  |
| `trailing`             | collect everything after `--` into a `Vec<String>`                         |
| `count`                | count repeated flags into a `uN` (`-vvv` → `3`)                           |
| `default = "str"`      | default value, parsed the same way as a user-supplied string               |
| `value_name = "str"`   | placeholder shown in usage (`<PATH>` instead of `<output>`)               |
| `help = "str"`         | override the doc comment for this field's help line                        |
| `group = "name"`       | add to a named mutually-exclusive group                                    |
| `conflicts_with = "f"` | this flag may not appear alongside field `f`                               |
| `hidden`               | accept the flag/argument but omit it from help                             |
| `subcommand`           | delegate remaining args to this field's `Parse` enum                       |

### enum variant attributes

| attribute      | meaning                                  |
|----------------|------------------------------------------|
| `name = "str"` | override the subcommand name             |
| `hidden`       | accept the command but hide it from help |

## going without the derive

you can hand-build a `CommandSpec` and impl `Parse` yourself — useful for
dynamic or programmatic command trees. see `pound/tests/cli.rs` for a full
worked example.

## features

| feature  | default | description                                                                      |
|----------|---------|----------------------------------------------------------------------------------|
| `derive` | yes     | enables `#[derive(Parse)]` and `#[derive(ValueEnum)]`                            |
| `help`   | yes     | bakes doc-comment help text in and enables the formatter; without it, `-h` shows a bare usage line |

disable both with `default-features = false` for the leanest possible binary.

## dev

the dev environment is a nix flake. `nix develop` gives the toolchain,
`nix develop .#fmt` formats the tree (nightly rustfmt + taplo) on entry.

## license

EUPL-1.2
