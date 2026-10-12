# Changelog

`pound`, `pound-derive` and `pound-derive-impl` release together under one version.

## 0.4.0

### Breaking

- `Parse` has a required `update_from_matches` method, which the derive writes. A hand-written `Parse` impl has to add it.
- `ErrorKind` has new `Exclusive`, `NoArguments` and `InvalidUtf8` variants, so an exhaustive match on it has to handle them.
- Help and usage are typed. `ErrorKind::Help` holds a `Help`, `ErrorKind::MissingSubcommand` an `Option<Help>`, and `Error::usage` an `Option<Usage>`, in place of `String`s. `Error::render` takes the `Term` to render for, such as `Term::stdout()` or `Term::plain()`.
- `FromArg::POSSIBLE`, `FromArg::possible_values`, `ArgSpec::possible` and `ArgSpec::possible_opt` hold `Choice`s in place of `&str`s.
- `args_from_raw` returns a `Result`, failing with `ErrorKind::InvalidUtf8` on a non-UTF-8 argument it used to drop.
- An attribute string reads every Rust escape, so `\n` or `\t` in a `help` is the character it names, and a raw string is taken as written.
- A trailing arg takes every word after its first value, flags included, so `--force ls -la --force` hands the second `--force` to the trailing arg.
- A command with subcommands answers `help <command>...` with that command's help. Where a subcommand name can go, the word `help` no longer fills an optional positional or fails as an unknown subcommand. A subcommand named or aliased `help`, or an `external` variant, keeps the word.

### Added

- `external` on a `Vec<T>` or `pound::External` variant takes any subcommand name no other variant answers to, with the rest of the line. `External` keeps the name apart and can raise the error the name would have.
- `exclusive` makes an arg the only one allowed when given, and lifts the command's requirements.
- `help_if_empty` prints the help and exits 2 when a command is given no words.
- `Option<Vec<T>>` fields read `None` when never given.
- `delimiter` splits each value of a `Vec` arg, its `env` and `default` included, so `--features a,b` gives two.
- A `ValueEnum` variant takes `alias` and `hidden`, and its doc comment describes it in long help. `ignore_case` on the enum matches its values in any case.
- `pound::complete` parses a half-typed command line for shell completion, and `complete` and `hint` name the candidates for an arg's value.
- `Command` views a command with its path and the globals it inherits.
- `Parse::try_update_from` and `Parse::update_from` parse more words into a value already parsed, overwriting only what they give. They need `Self: Clone`, so a failed update leaves the value as it was.
- `Sourced<T>`, `Option<Sourced<T>>` and `Sourced<bool>` fields carry a `Source` saying whether the value was typed, read from `env`, or defaulted.
- `Option<Option<T>>` options read `Some(None)` when given with no value.
- `num_args` lets one use of a `Vec` option take several words, as `2`, `1..` or `1..=3`.
- `allow_hyphen_values` lets a positional or a `num_args` option take words that start with a dash.
- `short_alias` gives a flag or option more short names.
- `display_order` orders flags, options and subcommands in help and man pages.
- `hide_possible`, `hide_default` and `hide_env` keep those tags out of help and man pages.
- `before_help` and `after_help` set text around the help page and man page, keeping indented lines as written.
- `long_version` sets fuller text for `--version`, while `-V` keeps `version`.
- The `unicode` feature measures wide characters when wrapping help.

### Changed

- Help and errors wrap to the terminal width, read from the terminal or `COLUMNS`, and are colored on a terminal. `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE` and `TERM=dumb` are honoured.

### Fixed

- `Parse::try_parse` fails with `ErrorKind::InvalidUtf8` on a non-UTF-8 argument where it used to panic.
