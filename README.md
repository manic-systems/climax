# pound

a low footprint, derive-first cli parser for rust. the derive emits a flat `&'static`
description of your command line and a single non-generic engine interprets it,
so derive ergonomics stay familiar while adding almost nothing to your binary
and dragging in nothing at runtime.

## status

working: tokenizer (long, short, `-abc` bundling, `--opt=val`, `--`),
subcommands (top-level, as a struct field, and nested), mutually-exclusive
groups (optional or required), pairwise conflicts, counts, defaults, hidden
args/commands, gnu-style help, auto help/version, and a `FromArg` trait for
custom value types, all behind the `#[derive(Parse)]` / `#[derive(ValueEnum)]`
macros. you can also hand-build a `CommandSpec` and impl `Parse`, see
`pound/tests/cli.rs`.

## model

field shapes carry meaning, so most fields need no attribute:

| shape       | meaning                |
|-------------|------------------------|
| `bool`      | flag, presence is true |
| `T`         | required positional    |
| `Option<T>` | optional positional    |
| `Vec<T>`    | variadic / repeatable  |

`#[pound(short)]` / `#[pound(long)]` promote any of these to a named option. the
annotated thing is the switch, values stay bare.

field attributes: `short`, `long` (bare or `= "name"` / `= 'c'`), `positional`,
`trailing` (everything after `--`), `count` (`-vvv` into a `uN`), `default =`,
`value_name =`, `help =`, `group = "name"` (mutually exclusive set),
`conflicts_with = "field"`, `hidden` (omit from help), and `subcommand` (the
field's type, itself a `Parse` enum, supplies the commands; `Option<T>` makes the
subcommand optional). item attributes: `name =`, `version =`,
`required_group = "name"` (exactly one of the group must be set), and `hidden` on
an enum variant to hide that command. an enum derives a subcommand tree, one
variant per command. `#[derive(ValueEnum)]` makes a unit enum a `FromArg` choice
type with kebab-cased values, and its choices are listed automatically in help
and in invalid-value errors.

```rust,ignore
use pound::Parse;

#[derive(Parse)]
struct Add {
    name: String,                          // required positional
    url:  String,                          // required positional
    #[pound(long)] unpack:  Option<String>,
    #[pound(long)] follows: Vec<String>,   // repeatable --follows
    #[pound(short, long)] force: bool,      // -f / --force
}

let add = Add::parse(); // exits on -h/--help or a parse error
```

a struct can carry global options and delegate the rest to a subcommand enum:

```rust,ignore
#[derive(Parse)]
enum Action {
    Build { #[pound(short, long)] release: bool },
    Clean,
}

#[derive(Parse)]
struct Cli {
    #[pound(short, long)] verbose: bool, // a global, before the subcommand
    #[pound(subcommand)] action: Action, // tool build --release | tool clean
}
```

custom value types implement `FromArg`:

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

## features

- `help` (default): bake doc-comment help into the binary and enable the
  formatter. build with `default-features = false` for the leanest binary, help
  degrades to a one-line usage string.

## dev

the dev environment is a nix flake. `nix develop` gives the toolchain,
`nix develop .#fmt` formats the tree (nightly rustfmt + taplo) on entry.

## license

EUPL-1.2
