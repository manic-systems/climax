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

| code        | meaning                                                                    |
|-------------|----------------------------------------------------------------------------|
| 0           | success, help and version                                                  |
| 1           | application error, printed to stderr as `error: ...`                       |
| 2           | parse failure (`main` only)                                                |
| 141         | help or version hit a closed stdout pipe (`main` only), other write failures exit 1 |
| 130         | cancellation, silent unless related errors are attached                    |
| 128 + n     | prompt interrupted by signal `n`, silent like 130                          |
| `with_exit_code` | the given code for an error of any kind, 0 becomes 1                  |

`Error::cancelled()` is a cancellation of your own, and an explicit code keeps it silent

## Prompts

prompts return `PromptOutcome`, either `Submit` with the user's type or `Leave` when cancelled,
and `or_cancel` turns leaving into a `Cancelled` error that exits 130. without a terminal on
stdin a prompt fails with `InteractionUnavailable`, it never invents a value

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

```rust,ignore
let value = cx.status("scanning").spinner().during(|| scan())?;
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
`#[climax::serde(..)]` does not exist and fails with `expected attribute, found
module`, so serde's own derives need `#[serde(crate = "climax::serde")]` when serde is
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

## Testing

`climax::testing::run` runs an application in process with the reporting and exit codes of `main`,
and returns an `Outcome` with `exit_code`, `stdout`, `stderr` and `error`. a `Script` answers
prompts in order and fails the run when input is left over

## Dependencies

every dependency on a climax-family crate is pinned exactly in `[workspace.dependencies]`, and
`crates/climax/tests/workspace_pins.rs` fails on any edge that is not. `pound` and `caress` release
on their own, so `climax` requires them at the range it was tested against. the fixtures under
`tests/compile-fixtures` build `climax` with no default features and with each feature alone
