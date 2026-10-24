# Facade

`climax` composes parsing, rendering, typed prompts, output routing and application policy, and
re-exports each component whole, so an application needs no other dependency

## Features

| feature       | provides                                                   | component        |
|---------------|------------------------------------------------------------|------------------|
| `parse`       | `main`, `try_run`, `try_run_from`                          | `climax::pound`  |
| `derive`      | `#[derive(climax::Parse)]`, `#[derive(climax::ValueEnum)]` | `climax::pound`  |
| `render`      | `status`                                                   | `climax::screw`  |
| `interactive` | typed prompts on `Context`, `PromptOutcome`                | `climax::bang`   |
| `structured`  | json results, `#[climax::serde(..)]`                       | `climax::serde`  |
| `complete`    | shell completion in `main`, `climax::install`              | `climax::caress` |
| `anyhow`      | `From<anyhow::Error>`, off by default                      |                  |

`derive` implies `parse`, and every feature but `anyhow` is on by default

## Lifecycle

`main` and `main_with` own parse errors, diagnostics and the exit code. `try_run` and
`try_run_from` report nothing, for embedding and tests, and `run_with` starts from a command you
already built

```rust,no_run
# use climax::prelude::*;
# #[cfg_attr(feature = "derive", derive(Parse))]
# struct Cli {}
# fn run(_: Context, _: Cli) -> climax::Result<()> { Ok(()) }
# #[cfg(not(all(feature = "parse", feature = "derive")))]
# fn main() {}
# #[cfg(all(feature = "parse", feature = "derive"))]
fn main() -> std::process::ExitCode {
    climax::main::<Cli, _>(run)
}
```

### Exit codes

| code             | meaning                                                                          |
|------------------|----------------------------------------------------------------------------------|
| 0                | success, help and version                                                        |
| 1                | application error, printed to stderr as `error: ...`                             |
| 2                | parse failure (`main` only)                                                      |
| 141              | stdout closed (`main`, including help and version), a failed final write exits 1 |
| 130              | cancellation, silent unless related errors are attached                          |
| 128 + n          | prompt interrupted by signal `n`, silent like 130                                |
| `with_exit_code` | the given code for an error of any kind, 0 becomes 1                             |

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

`stream` and `result` cannot be mixed, a second `result` is an error, and json writes a non-finite
float as `null`

```rust,no_run
# use climax::prelude::*;
# fn scan_view(result: &String) -> String { result.clone() }
# #[cfg(feature = "structured")]
# fn run(cx: Context) -> climax::Result<()> {
# let scan_result = String::new();
cx.output()
    .result(&scan_result)
    .text(|result| scan_view(result))
    .emit()?;
# Ok(())
# }
```

## Status

`StatusMode::Auto` animates on a terminal and prints one plain line per finished status
elsewhere, except a failed status without a `failure_message`, which prints nothing. `Silent`
prints nothing and suits a `--quiet` flag. every status shares one renderer, and a prompt
suspends them while it holds the terminal

```rust,no_run
# use climax::prelude::*;
# fn scan() -> climax::Result<u32> { Ok(0) }
# #[cfg(feature = "render")]
# fn run(cx: Context) -> climax::Result<()> {
let value = cx.status("scanning").spinner().during(|| scan())?;
# Ok(())
# }
```

## Errors

`climax::Error` has a stable `ErrorKind` and keeps dependency errors as opaque sources.
`climax::ResultExt` adds `.context(message)` and `.app_err()` to any `Result` of a
`Send + Sync` error, and stays out of the prelude so `anyhow::Context` can sit beside it

## Testing

`climax::testing::run` runs an application in process with the reporting and exit codes of `main`,
and returns an `Outcome` with `exit_code`, `stdout`, `stderr` and `error`. a `Script` answers
prompts in order, panics when input is left over and fails the run with `InputEnded` when it runs out

## Dependencies

every dependency on a climax-family crate is pinned exactly in `[workspace.dependencies]`, and
`crates/climax/tests/workspace_pins.rs` fails on any edge that is not, except the path-only dev
edge from `pound-derive` to `pound`. `pound` and `caress` release
on their own, so `climax` requires them at the range it was tested against, and `climax-derive`
requires `pound-derive-impl` at a caret range. the fixtures under
`tests/compile-fixtures` build `climax` with no default features and with each feature alone
