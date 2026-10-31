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
| `structured`  | json or text results, `#[climax::serde(..)]` with `derive` | `climax::serde`  |
| `complete`    | shell completion in `main`, `climax::install`              | `climax::caress` |
| `anyhow`      | `From<anyhow::Error>`, off by default                      |                  |

`derive` implies `parse`, and every feature but `anyhow` is on by default

derive with `climax::Parse` and `climax::ValueEnum`. the derives under `climax::pound` expand to
`::pound` paths and need a direct `pound` dependency

## Lifecycle

`main` and `main_with` own parse errors, diagnostics and the exit code. `try_run` and
`try_run_from` report nothing, for embedding and tests, though with `complete` `try_run` still
answers a completion request and exits the process. `run_with` starts from a command you already
built

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
| 141              | stdout closed, or a `BrokenPipe` i/o error the handler returns (`main`)          |
| 130              | cancellation, silent unless related errors are attached                          |
| 128 + n          | prompt interrupted by signal `n`, silent like 130                                |
| `with_exit_code` | the given code for an error of any kind, 0 becomes 1                             |

`Error::cancelled()` is a cancellation of your own, and an explicit code keeps it silent. an
explicit code also wins over 141. a final report that cannot be written keeps its intended code,
except that a closed stdout still gives 141

## Prompts

prompts return `PromptOutcome`, either `Submit` with the user's type or `Leave` when cancelled,
and `or_cancel` turns leaving into a `Cancelled` error that exits 130. a prompt needs stdin and
stderr on a terminal and `TERM` other than `dumb`, otherwise it fails with
`InteractionUnavailable` and never invents a value

a signal during a prompt restores the terminal and surfaces as `Cancelled` with
`Error::signal()` set, `main` exits 128 plus the number, and the host is never killed by it

## Output

durable results go to stdout, and notices, status and prompts go to stderr

- `result` registers one finite result, held until the handler succeeds, then written as text or
  json
- `stream` writes values as they arrive, as json lines in json mode
- `notice` writes human-only context and is suppressed in json mode
- `Context::diagnostic` is a second handle that defaults to stderr

output is text until `Context::set_output_format(Format::Json)` or `with_output_format` switches
it, usually from a `--json` flag. `stream` and `result` cannot be mixed, a second `result` is an
error, and json writes a non-finite float as the string `"NaN"`, `"Infinity"` or `"-Infinity"`

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
`Send + Sync` error, and stays out of the prelude so `anyhow::Context` can sit beside it. a
`climax::Error` or `bang::Error` they wrap, directly or inside an `anyhow` chain, keeps its kind,
signal and exit code, so a cancelled prompt stays a silent 130

## Testing

`climax::testing::run` runs an application in process with the reporting and exit codes of `main`,
and returns an `Outcome` with `exit_code`, `stdout`, `stderr` and `error`. a `Script` answers
prompts in order, panics when input is left over and fails the run with `InputEnded` when it runs out

`testing` needs no feature for `Capture`, `Outcome` and `run_with`, `run` needs `parse`, and a
`Script` answers prompts only with `interactive`
