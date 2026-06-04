# pound

a low-footprint, derive-first cli parser for rust.

the derive emits a flat `&'static` description of your command and one non-generic
engine interprets it, so you get familiar derive ergonomics, almost no compile-time
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
| `bool`      | flag, present means true  |
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
    #[pound(short, long, default = "4", min = "1", max = "64", validate = "power_of_two")]
    jobs: u32,                           // --jobs <n>  (default: 4, power of two)
}

fn power_of_two(value: &u32) -> Result<(), &'static str> {
    if value.is_power_of_two() {
        Ok(())
    } else {
        Err("must be a power of two")
    }
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

### parent options + subcommand field

a struct can carry its own flags and delegate the rest of the command line to a
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
    #[pound(short, long, global)]
    verbose: bool,

    #[pound(long, default = "info")]
    log: String,

    #[pound(subcommand)]
    action: Action,         // required, shows help if absent
}

fn main() {
    let cli = Cli::parse();
}
```

a plain parent flag must appear *before* the subcommand. mark it
`#[pound(global)]` and it is also accepted *after*, at any subcommand depth,
landing on the parent field either way:

```
tool --verbose build --release     # before the subcommand
tool build --release --verbose     # after it — only because verbose is global
tool --log debug clean             # log is not global, so it must come first
```

`global` only applies to named flags/options (`short`/`long`), never
positionals, and shows up in each subcommand's `--help` under `Global options:`.

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

## custom parsing and validation

add `parse = "path"` to parse a field with your own function. this works for
custom types that do not implement `FromArg`; the parser takes the raw token and
returns the field value:

```rust,ignore
use pound::Parse;

#[derive(Debug)]
struct HexByte(u8);

fn hex_byte(value: &str) -> Result<HexByte, &'static str> {
    let value = value.strip_prefix("0x").ok_or("expected 0xNN")?;
    u8::from_str_radix(value, 16)
        .map(HexByte)
        .map_err(|_| "expected two hex digits")
}

#[derive(Parse)]
#[pound(name = "hex")]
struct Hex {
    #[pound(long, parse = "hex_byte")]
    byte: HexByte,
}
```

for normal `FromArg` value fields, add `min` / `max` to an ordered field or
`max_len` to reject raw values over a character limit. `validate = "path"` calls
your own parsed-value checker and can be combined with either built-in parsing
or `parse = "path"`. the checks compose with required fields, `Option<T>`,
`Vec<T>`, defaults, and environment fallbacks:

```rust,ignore
use pound::Parse;

#[derive(Parse)]
#[pound(name = "limit")]
struct Limit {
    #[pound(long, min = "5", max = "20")]
    count: u64,       // --count must parse as 5..=20

    #[pound(long, max_len = "9")]
    name: String,     // --name must be 9 chars or shorter

    #[pound(long, validate = "even")]
    shard: u64,       // custom parsed-value validation
}

fn even(value: &u64) -> Result<(), &'static str> {
    if value % 2 == 0 {
        Ok(())
    } else {
        Err("must be even")
    }
}
```

`parse = "path"` is for value fields only and cannot be combined with `min`,
`max`, or `max_len`; use a custom `validate = "path"` check for custom parsed
types.

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
| `env = "VAR"`          | fall back to environment variable `VAR` (cli > env > default; `std` only)  |
| `min = "str"`          | reject parsed `FromArg + PartialOrd` values below this bound               |
| `max = "str"`          | reject parsed `FromArg + PartialOrd` values above this bound               |
| `max_len = "n"`        | reject raw value strings longer than `n` characters                        |
| `parse = "path"`       | parse a value field with `fn(&str) -> Result<T, &'static str>`             |
| `validate = "path"`    | validate a parsed value with `fn(&T) -> Result<(), &'static str>`          |
| `value_name = "str"`   | placeholder shown in usage (`<PATH>` instead of `<output>`)               |
| `help = "str"`         | override the doc comment for this field's help line                        |
| `group = "name"`       | add to a named mutually-exclusive group                                    |
| `conflicts_with = "f"` | this flag may not appear alongside field `f`                               |
| `alias = "a,b"`        | extra long names that also match, kept out of help                         |
| `hidden`               | accept the flag/argument but omit it from help                             |
| `global`               | named flag/option also accepted after the subcommand, at any depth         |
| `subcommand`           | delegate remaining args to this field's `Parse` enum                       |

### enum variant attributes

| attribute       | meaning                                  |
|-----------------|------------------------------------------|
| `name = "str"`  | override the subcommand name             |
| `alias = "a,b"` | extra names that also select the command, kept out of help |
| `hidden`        | accept the command but hide it from help |

## going without the derive

you can hand-build a `CommandSpec` and impl `Parse` yourself, which is handy for
dynamic or programmatic command trees. see `pound/tests/cli.rs` for a full
worked example.

## features

| feature  | default | description                                                                      |
|----------|---------|----------------------------------------------------------------------------------|
| `std`    | yes     | the std-only conveniences: `parse()` / `try_parse()` (read argv), `Error::exit()`, and the `PathBuf` value impl. turn it off for `#![no_std]` against `alloc` (see below) |
| `derive` | yes     | enables `#[derive(Parse)]` and `#[derive(ValueEnum)]`                            |
| `help`   | yes     | bakes doc-comment help text in and enables the formatter; without it, `-h` shows a bare usage line |

disable all three with `default-features = false` for the leanest possible binary.

## no_std

pound is `#![no_std]` when you turn the `std` feature off. it still needs an
allocator (matched values, the help formatter, and error messages use `alloc`),
but nothing from `std`:

```toml
[dependencies]
pound = { version = "0.1", default-features = false, features = ["derive", "help"] }
```

what you give up without `std`, and what to use instead:

| std convenience            | no_std replacement                                              |
|----------------------------|----------------------------------------------------------------|
| `Parse::parse()` (reads argv) | feed args to `Parse::try_parse_from(args)` yourself          |
| `Error::exit()`            | match on the returned `Error` and decide how to bail            |
| `FromArg for PathBuf`      | impl `FromArg` for your own path type                          |

### borrowed arguments

`try_parse_from` takes an `IntoIterator<Item = &str>`, and the parser never
copies an argument into an owned `String`. matched values borrow straight from
the input; only the fields you declare as `String` allocate, when they're read
out. the input just has to outlive the call, which always holds, since `Self`
owns whatever it keeps.

### sourcing argv on no_std

pound can't portably *fetch* argv; that's an OS detail `std` normally handles.
but if your program owns its libc entry point, it can hand pound the
`(argc, argv)` it already holds via `args_from_raw`:

```rust,ignore
use core::ffi::{c_char, c_int};

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: c_int, argv: *const *const c_char) -> c_int {
    // SAFETY: argc/argv are the unmodified parameters libc passed `main`.
    let args = unsafe { pound::args_from_raw(argc, argv) }.skip(1);
    match MyCommand::try_parse_from(args) {
        Ok(cmd) => { /* ... */ 0 }
        Err(e)  => { /* render e */ 2 }
    }
}
```

the yielded `&str`s borrow straight from `argv`, so parsing stays zero-copy.

## pound vs clap

the same CLI built both ways (root flags, three subcommands, a value enum, a
repeatable option, defaults), release profile `opt-level = "s"`, `lto = "fat"`,
stripped, on one machine.

### size

| parser    | stripped binary | over a no-parser baseline |
|-----------|-----------------|---------------------------|
| **pound** | 345 KiB         | +60 KiB                   |
| **clap**  | 517 KiB         | +232 KiB                  |

### build time

| parser    | cold debug | cold release | incremental |
|-----------|------------|--------------|-------------|
| **pound** | 2.1 s      | 4.9 s        | 0.13 s      |
| **clap**  | 4.1 s      | 9.3 s        | 0.22 s      |

### parse speed

| parser    | per parse (`try_parse_from`) |
|-----------|------------------------------|
| **pound** | ~52 ns                       |
| **clap**  | ~8.4 µs                      |

## dev

the dev environment is a nix flake. `nix develop` gives the toolchain,
`nix develop .#fmt` formats the tree (nightly rustfmt + taplo) on entry.

## license

EUPL-1.2
