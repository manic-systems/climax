# Facade

`climax` composes parsing, rendering, typed prompts, output routing and application policy, and
re-exports each component whole, so an application needs no other dependency

## Features

| feature       | provides                                                   | component       |
|---------------|------------------------------------------------------------|-----------------|
| `parse`       | `main`, `try_run`, `try_run_from`                          | `climax::pound` |
| `derive`      | `#[derive(climax::Parse)]`, `#[derive(climax::ValueEnum)]` | `climax::pound` |
| `render`      | `status`                                                   | `climax::screw` |
| `interactive` | typed prompts on `Context`, `PromptOutcome`                | `climax::bang`  |
| `structured`  | json results, `#[climax::serde(..)]`                       | `climax::serde` |
| `complete`    | shell completion in `main`, `climax::install`              | `climax::caress` |
| `anyhow`      | `From<anyhow::Error>`, off by default                      |                 |

`derive` implies `parse`, and every feature but `anyhow` is on by default

## Lifecycle

`main` and `main_with` own parse errors, diagnostics and the exit code. `try_run` and
`try_run_from` report nothing, for embedding and tests, and `run_with` starts from a command you
already built

```rust,ignore
fn main() -> std::process::ExitCode {
    climax::main::<Cli, _>(run)
}
```

### Exit codes

`main` and `main_with` map outcomes to exit codes in one place.

- 0 for success, and for help and version output on stdout.
- 1 for an application error, printed to stderr as `error: ...`.
- 2 for a parse failure printed to stderr (`main` only).
- 130 for a cancellation, including `Error::cancelled()`, or 128 plus the signal
  number when a signal interrupted a live prompt. Either exit is silent unless
  related errors are attached, in which case those print instead.
- The code given to `Error::with_exit_code`, for an error of any kind. A
  cancellation with an explicit code stays silent and any other error is still
  printed.

`Error::cancelled()` is a cancellation of your own, and an explicit code keeps it silent

`ResultExt`, in the prelude, adds `.context(message)` and `.app_err()` to any
`Result` whose error is a `Send + Sync` `std::error::Error`, so
`text.parse::<u32>().context("reading the count")?` becomes an application
error that reads `reading the count: invalid digit found in string` and keeps
the original as its source. A `Box<dyn std::error::Error + Send + Sync>`
converts into `Error` with `?`.

a signal during a prompt restores the terminal and surfaces as `Cancelled` with
`Error::signal()` set, `main` exits 128 plus the number, and the host is never killed by it

## Output

durable results go to stdout, and notices, status and prompts go to stderr

- `result` registers one finite result, held until the handler succeeds, then written as text or
  json
- `stream` writes values as they arrive, as json lines in json mode
- `notice` writes human-only context and is suppressed in json mode
- `Context::diagnostic` is a second handle that defaults to stderr

```rust,ignore
cx.output()
    .result(&scan_result)
    .text(|result| scan_view(result))
    .emit()?;
```

## Status

`StatusMode::Auto` animates on a terminal and prints one plain line per finished status
elsewhere, `Silent` prints nothing and suits a `--quiet` flag. every status shares one renderer,
and a prompt suspends them while it holds the terminal

```text
climax::pound   (parse)        climax::bang   (interactive)
climax::screw   (render)       climax::serde  (structured, with serde's derive feature)
```

Every dependency between workspace crates is pinned exactly (`=x.y.z`) in
`[workspace.dependencies]`, including dev-dependencies, and a test in
`crates/climax/tests/workspace_pins.rs` fails on any edge that is not. A
`climax` release therefore names one tested set of `pound`, `bang`, and `screw`
versions, and `bang` cannot resolve against a `screw` it was not built with. An
application that also depends on a component directly stays on the same
version. `pound`, `bang`, and `screw` remain fully usable without `climax`.

## Parse derive detail

`climax::main` and `try_run` accept a `pound::Parse` command when the `parse`
feature is enabled. The `derive` feature, on by default and implying `parse`,
provides `#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` through
the `climax-derive` proc-macro crate. Its expansion is rooted at
`::climax::pound`, so no direct `pound` dependency is needed. The prelude brings
in both derives and the `pound::Parse` trait anonymously (`as _`), which makes
`Args::try_parse_from(...)` available.

`climax-derive` and `pound-derive` are thin wrappers around the plain library
crate `pound-derive-impl`, which takes the root path and a help switch.
`#[derive(pound::Parse)]` with a direct `pound` dependency behaves as before.
The root path of `climax-derive` is fixed at `::climax::pound`, so a crate that
renames its `climax` dependency cannot use the derive.

With the `derive` and `structured` features, `#[climax::serde(Serialize,
Deserialize)]` derives serde's traits rooted at `::climax::serde` and adds
`#[serde(crate = "::climax::serde")]`, keeping the item and its other
attributes. It takes plain derive names, rejects an empty list, and must come
before other derives that share serde's helper attributes. Without `derive`,
serde's own derives need `#[serde(crate = "climax::serde")]` when serde is
reached through `climax`.

## Ownership

- `pound` owns parsing, derives, specifications, and raw matches.
- `screw` owns surfaces, rendering, styles, widgets, terminal size, and renderer
  runtimes.
- `bang` owns typed prompt builders, typed results, interaction sessions, and
  the public advanced escape hatch.
- `climax` owns application lifecycle, policy, output routing, status access,
  and high-level error mapping.
- `bang-core` and `bang-terminal` are bang's implementation crates and not
  ordinary user APIs. Terminal input, guards and signals are reached as
  `bang::terminal`.

`climax::Error` exposes facade-level categories and retains dependency errors as
opaque sources. General I/O, output failures, unavailable or busy interaction,
and application-owned source errors remain distinguishable. `Error::application`
and `Error::application_context` retain source chains without making dependency
error enums part of the facade contract. Uncaught cancellation exits with
status 130, or 128 plus the signal number for an interrupted prompt, silently
unless related errors are attached, in which case those print instead.
`Error::with_exit_code` replaces the code for any kind.

`climax::Error` has a stable `ErrorKind` and keeps dependency errors as opaque sources.
`climax::ResultExt` adds `.context(message)` and `.app_err()` to any `Result` of a
`Send + Sync` error, and stays out of the prelude so `anyhow::Context` can sit beside it

A cancellation whose terminal restore then fails is not classified as
cancelled at all. `bang` reports it as a terminal error instead, so it reaches
an application through the ordinary error path and exits 1 rather than 130.

Pound preserves source declaration order across direct and flattened fields
for positional parsing, help, and introspection, and rejects ambiguous long
names, aliases, or short names within a flattened command level. Screw runtime
root and configured-final widgets have independent types, and only widgets sent
to an already running renderer thread are type-erased.

## Features

- `derive` provides `#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]`
  and implies `parse`.
- `parse` provides the lifecycle entry points and Pound integration.
- `render` provides status rendering and Screw integration.
- `interactive` provides typed Bang prompts.
- `structured` provides Serde-backed application results and JSON and JSON
  Lines output.

All five are on by default. The dependency fixtures under
`tests/compile-fixtures` build and test `climax` with no default features and
with each of `derive`, `interactive`, `parse`, `render` and `structured` alone.
Another fixture checks that a direct dependency on a component unifies with the
one `climax` re-exports.

The README code blocks are compiled as `climax` doctests only when `derive`,
`interactive`, `render` and `structured` are all on.

`climax::testing::run` runs an application in process with the reporting and exit codes of `main`,
and returns an `Outcome` with `exit_code`, `stdout`, `stderr` and `error`. a `Script` answers
prompts in order and fails the run when input is left over

## Dependencies

every dependency on a climax-family crate is pinned exactly in `[workspace.dependencies]`, and
`crates/climax/tests/workspace_pins.rs` fails on any edge that is not. `pound` and `caress` release
on their own, so `climax` requires them at the range it was tested against. the fixtures under
`tests/compile-fixtures` build `climax` with no default features and with each feature alone
