# Changelog

All notable changes to the pound family (`pound`, `pound-derive` and `pound-derive-impl`) are documented here. The family releases together and the format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - Unreleased

### Added

- `pound-derive-impl`, a plain library crate holding the derive expansion. `derive_parse` and `derive_value_enum` take an `Options` value naming the path to the pound runtime, such as `::pound`, and whether help text is baked in. A crate that re-exports pound can now ship its own derive macros rooted at its own path. `pound-derive` is a thin wrapper over it, so `#[derive(pound::Parse)]` behaves as before.
- `parse` and `validate` accept a path or any callable expression, such as `parse = str::parse::<NonZeroUsize>` or `validate = |n: &u32| if *n < 100 { Ok(()) } else { Err("too big") }`. The string form still names a path.
- `default` accepts a braced constant of type `&str`, such as `default = { SYSTEM_PROFILE }`, as well as a string literal or a bare word like `auto`, which keep their 0.2.0 meaning. A constant that is not one of a `ValueEnum` field's values fails the build, as a literal does.
- `FromArg` for `NonZeroU8` through `NonZeroU128`, `NonZeroUsize` and the signed `NonZero` integers. Zero is rejected as an invalid value.
- `ArgSpec::metavar`, the placeholder usage and errors show for an argument's value. It is the `value_name`, or else the long name, uppercased.
- `CommandSpec::hash_opt`, which sets the hash only when it is `Some` and not empty.
- Help lists `[default: ...]` and, with the `std` feature, `[env: ...]` after the description of any option or positional that takes a value. Flags show neither.
- `ArgValue`, a trait the `ValueEnum` derive implements with `as_str`, the exact word the parser accepts for a value (renames and kebab case included), and `ALL`, every value in declaration order. Forward `Display` or serde to `as_str` and the printed form cannot drift from the parsed one. It is a trait so it cannot collide with a `Display` impl you write.
- A complete attribute reference on the `Parse` and `ValueEnum` derives, covering every item, variant and field attribute, the field type mapping and the `default`, `env`, group and conflict forms. The examples are compiled doctests, and the two headline examples in the `pound` crate docs now compile too.

### Changed

- **Breaking** `--version` no longer runs `git` while the derive expands. The build hash shown after the version now comes from the `POUND_GIT_HASH` environment variable, read with `option_env!` in the deriving crate, so changing it rebuilds that crate. A program that relied on the automatic hash prints only its version until a build script sets the variable.

  ```rust
  // build.rs
  fn main() {
      let out = std::process::Command::new("git").args(["rev-parse", "--short=12", "HEAD"]).output();
      if let Ok(out) = out {
          println!("cargo:rustc-env=POUND_GIT_HASH={}", String::from_utf8_lossy(&out.stdout).trim());
      }
      println!("cargo:rerun-if-changed=.git/HEAD");
  }
  ```

- Usage lines in help and in parse errors for a subcommand carry the full command path, so a failure in `prog remote add` shows `Usage: prog remote add [OPTION]... NAME URL` and not `Usage: add ...`.
- A missing or invalid positional is named by its usage placeholder, `NAME` where it was `<name>`. A positional with no value name was `<value>`. `ArgSpec::display_name` returns the same text.
- The built-in help and version rows read `Print help` and `Print version`, as clap words them.
- Help summaries taken from doc comments drop the final period of the first sentence, for arguments and for subcommand listings. Text given with `help = "..."` is left as written.
- A value that came from an env fallback and fails to parse is reported with its source, as in `invalid value '99' for --replicas (from $SHIPIT_REPLICAS): ...`. The message for a command-line value is unchanged.
- A command that requires a subcommand and got none, such as `prog pkg` or a bare `prog`, now fails with `ErrorKind::MissingSubcommand` and prints its help text to stderr with exit status 2, as clap's `arg_required_else_help` does. It used to return `ErrorKind::Help`, which printed to stdout and exited 0. `Error::usage` holds the whole help text for this error, and `Error::render` shows it after `error: a subcommand is required`. A caller that matched `ErrorKind::Help` to detect the bare invocation should match `MissingSubcommand`.
- The `ValueEnum` derive rejects two variants that end up with the same command-line spelling, whether through `name`, kebab-case conversion or both, with a compile error naming both variants and the spelling. 0.2.0 accepted them, but the later variant could never be parsed.
- A derive without `#[pound(name)]` that expands inside a binary target names the command after that binary, read with `option_env!("CARGO_BIN_NAME")` in your crate. A binary called `tool` in a package called `tool-cli` now shows `tool` in usage and `--version`, where it showed the package name. Libraries, tests and an explicit `name` are unchanged.

### Fixed

- A comma inside a turbofish such as `parse = pair::<u8, u16>` no longer splits the attribute.
- `#[pound(heading)]` is left out of the expansion when the `help` feature is off, where it used to be compiled into the spec and never shown.
- Generated bindings and items carry a `__pound_` or `__POUND_` prefix and a mixed-site span, so a unit struct named `spec` or a const named `ARGS` no longer breaks the expansion, and `default = { ARGS }` or `version = CMD` resolves to your item and never to a generated one.
- `Option<T>` and `Vec<T>` fields written as `::core::option::Option<T>`, `std::option::Option<T>`, `alloc::vec::Vec<T>`, `std::vec::Vec<T>` and the matching forms are recognised. They were treated as plain required values.

## [0.2.0] - 2026-10-06

This release changes the public spec API, tightens what the derive accepts and corrects several parsing behaviours. Read the Breaking items before upgrading from 0.1.x.

### Added

- `#[pound(flatten)]` on a struct field embeds another `Parse` struct's arguments at the same level. Fields keep their declaration order across direct and flattened arguments for positionals, help and introspection. Required groups and conflicts are checked once across every flattened struct at a level.
- `#[pound(flatten)]` on an enum variant holding another command enum offers that enum's commands as if they were declared on the outer enum.
- `Subcommands`, a marker trait the derive implements for every command enum. A `#[pound(subcommand)]` field or flattened variant must hold such a type, and a struct there now fails to compile.
- Spec API for hand-built and introspecting code, namely `CommandSpec::arguments` and `CommandSpec::subcommands` (both follow flattening), `CommandSpec::flattened`, `CommandSpec::argument_order` with `ArgumentOrder`, `SubSpec::flatten`, the `SubSpec::flattened` field, `ArgSpec::answers_long` and `ArgSpec::answers_short`, `CommandSpec::subcommand_optional` and `Error::help_flag`.
- The `version` attribute accepts any expression, such as `version = env!("CARGO_PKG_VERSION")`.
- Optional subcommands show as `[COMMAND]` in usage where required ones show `COMMAND`.
- A global declared as `--help`, `-h`, `--version` or `-V` shadows the generated built-in in every descendant command. When several ancestors declare the same spelling, the nearest one wins.
- The `--help` and `--version` rows in help appear only for spellings that no local argument or inherited global claims.

### Changed

- **Breaking** `CommandSpec::find_long`, `CommandSpec::find_short` and `CommandSpec::find_negate` are removed. Use `ArgSpec::answers_long` and `ArgSpec::answers_short` over `CommandSpec::arguments()`, which also covers flattened structs.
- **Breaking** `CommandSpec::find_sub` returns `Option<&'static SubSpec>` and not an index, and it also finds commands spliced in through a flattened enum.
- **Breaking** `CommandSpec::args` and `CommandSpec::subs` no longer list everything a command parses, since flattened structs and enums live elsewhere. Code that walks a spec should read `arguments()` and `subcommands()`. `SubSpec` entries with `flattened` set have no name of their own.
- **Breaking** `pound::default_allowed` is removed from the crate root. The derive reaches it through the hidden `pound::checks` module, which is not public API.
- **Breaking** `Error` has a new public field, `help_flag`, so code that builds one with a struct literal must set it. The trailing `For more information, try '--help'.` line now names whichever of `--help` or `-h` still reaches the generated help and is left out when neither does.
- **Breaking** an unknown short flag such as `-q` is an `Unknown` error. It used to be taken as a positional value. A token that starts with a digit or `.` after the dash, such as `-5`, is still a value.
- **Breaking** an environment fallback counts toward a required argument or group only when the variable is set. Declaring `env` alone used to satisfy `required`, so a command whose variable is unset now reports `MissingRequired`.
- **Breaking** the derive rejects attributes it used to ignore. An unknown key, a key that is not valid on that item, field or variant, a bare key given a value (`count = "x"`), a value-taking key with no value, a malformed meta and `short` given more than one character are compile errors. `short = "ab"` used to use `a`. Fix these by removing the attribute or placing it where it applies.
- **Breaking** the derive checks more at compile time. It rejects two arguments answering to the same spelling (counting flattened structs), two commands sharing a name or alias, a command enum with no variants, more than one subcommand field across a command and its flattened structs, and a positional or subcommand that no command line can reach because a variadic or trailing positional precedes it. A struct with a `Vec` positional and a subcommand field must move the positional into a subcommand variant.
- `Error::exit` exits 141 when stdout is a closed pipe and 1 on any other failure to write help or version. Parse errors still exit 2.
- The examples need the `std` feature as well as `derive`.

### Fixed

- Printing help or version into a closed pipe no longer panics.

## [0.1.8] and earlier

Releases 0.1.0 through 0.1.8 are not itemised here. See the git history, where the tags `v0.1.0` to `v0.1.6` mark the early releases.
