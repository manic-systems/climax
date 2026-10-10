# Facade

`climax` composes parsing, rendering, typed prompts, output routing and application policy, and
re-exports each component whole, so an application needs no other dependency

## Promise

`climax` is the application product. It composes parsing, rendering, typed
interaction, output routing, and application policy without making users learn
the component crates behind them. An application needs `climax` as its only
dependency, since each component is re-exported whole behind the feature that
enables it.

The root surface is small.

```text
climax::{main, try_run, try_run_from, run_with, Context, Error, ErrorKind, Result,
    PromptOutcome, Configurable, Date, the config type and the prompt type of
    each prompt, ReviewConfig, ReviewExit, ReviewOutcome, ReviewState, Reviewed}
```

`main`, `try_run` and `try_run_from` need the `parse` feature. `PromptOutcome`,
`Configurable`, the prompt config types, and the review types need
`interactive`, and so do the prompt builders on `Context` such as `select` and
`confirm`. `run_with`, `Context`, `Error`, `ErrorKind`, and `Result` are always present.

`climax::prelude` re-exports those same lifecycle functions plus `Context`,
`Error`, `ErrorKind`, `Result`, and `output::Format` always, `FromArg` and `ValueError` under `parse`, and four terminal policy types from the
`terminal` module, `InteractionMode`, `StatusMode`, `TerminalCapabilities`, and
`TerminalPolicy`. Under `interactive` it adds `PromptOutcome`, `Configurable`,
the prompt config types, and the review types. The `output` module is always
present and is named explicitly instead of being folded into the prelude. It is
reached only through `Context::output` and `Context::diagnostic`, since
`Output::new` is crate-private. `status` is likewise named explicitly, and it
exists only when the `render` feature is on. Component internals do not belong
in the prelude.

## Typed prompts

`Context` delegates prompt construction to the public `bang` product while
keeping the ordinary outcome in the facade.

```rust,ignore
let outcome = cx
    .select("Shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let climax::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
```

An ordinary application therefore needs only `climax`. It re-exports
`PromptOutcome`, `Configurable`, the config type of every prompt, and the
review types that typed prompts return, consume, and configure.

The submitted value is the user's `Shell`, not `bang_core::Value`, and leaving
is a typed `PromptOutcome::Leave`. Select, multi-select, search, text,
password, confirm, date, and number prompts follow the same path. `Context`
hands out `password`, `confirm`, `date`, and `number::<T>` alongside the list
prompts. Every builder implements `bang::Configurable` with an associated
prompt-specific config, so list presentation cannot be applied to a text
prompt, while choices and their typed values stay on the builder. Presentation
is set through `with_config` and the config type, or through the same settings
as direct builder methods. `select`, `multi_select`, `search`, and `review`
take the visible header text and `.id(...)` is optional. Every list prompt
rejects zero choices, and page keys clamp at the ends of a list while single
steps still wrap.

An action-free review follows the ordinary shape and returns
`PromptOutcome<Vec<bang::Reviewed<T>>>`. Calling its first intrinsic `.action`
transitions to `bang::ReviewPromptWithActions<T, A>`. Further actions use the
same `A`, and interaction returns `bang::ReviewOutcome<T, A>`. Arbitrary action
layers remain an advanced widget facility. Prompt implementation and
live-session orchestration belong to `bang`, which draws its widgets through
`screw` and re-exports it as `bang::screw`. `climax` supplies application
policy and maps errors at the facade boundary.

Bang prompt builders carry an opaque, cloneable `bang::Interaction`. The
default driver uses stdin and stderr, and deterministic and custom drivers are
available through `bang::advanced`. Climax injects its configured driver into
every prompt created by `Context`.

### Signals

A signal that arrives during a live prompt makes bang restore the terminal and
return `ErrorKind::Interrupted`, with `bang::Error::signal()` naming the
signal. Bang never re-raises it, so the host process keeps running. A signal
the host set to ignored before the session stays ignored and does not interrupt
the prompt, and any other previous handler is restored on exit rather than
invoked. Applications that want the Unix convention of dying by the signal can
re-raise it themselves after the terminal is restored. The signal is a
`bang::terminal::Signal` (`INT`, `TERM`, `HUP` and `QUIT`, with `as_raw` and
`name`), which bang owns so that no dependency's signal type is public.

Climax maps the error to `ErrorKind::Cancelled` and keeps the signal number,
which `climax::Error::signal()` returns. `climax::main` exits with 128 plus
that number for such an error, and with 130 for any other cancellation. A
signal that lands while the session is already tearing down is still reported,
even if the prompt had submitted.

## Terminal and stream policy

`Context` snapshots process terminal capabilities and owns three logical output
channels.

- Durable application output goes to stdout by default.
- Diagnostics go to stderr by default.
- Transient prompts, status presentation, and notices go to stderr by default.

Automatic prompt interaction requires terminal-capable stdin, a suitable
transient terminal, and ANSI presentation support. `InteractionMode` can force
or disable interaction. Unavailable prompts return an error, and Climax never
invents a typed fallback value.

`StatusMode::Auto` animates only on a suitable transient terminal and is silent
otherwise. Plain, live, and silent modes override it, and silent mode never
emits status or final-message text. A status started with `during` prints
`final_message` only when the operation succeeds, `failure_message` when it
errors, unwinds, or the handle is dropped before finishing, and nothing when
`failure_message` is unset. Status handles register widgets with one
coordinator and one renderer, allowing multiple animations to coexist without
competing cursor writes. Human-only notices share that transient coordinator,
so a notice never interleaves with a status final line. Diagnostics on the
default stderr go through it too, and when stdout is also the terminal, `stream`
output clears the live region before it is written. An interactive prompt or
custom terminal application temporarily suspends status presentation and defers
finishing status lines until its exclusive lease releases. A second prompt on
the default driver waits on the stdin lock and runs after the first. Bang's
`InteractionBusy` appears only when the prompt uses a different terminal handle
or an outside `SignalGuard` already holds the signal handlers. Climax's own
status lease is separate, and a prompt or terminal application that finds it
held gets the same `InteractionBusy` category.

Streamed output respects that lease. A stream write from another thread while
a prompt or terminal application holds the lease is deferred and written when
the lease releases, and the call returns after the write. A stream write from
the thread that holds the lease fails with an error, because it would wait on
itself. The wait is bounded by the same ten second timeout as every coordinator
request, so a lease holder that joins a worker which is streaming sees the
worker's write fail and the join complete, and the write is dropped. A
`diagnostic().stream` write behaves like a notice. It blocks until written, or
is queued while a lease is held.

Scoped work uses a status.

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

Help and version go to stdout with status 0, parse failures go to stderr with
status 2, and application failures go to stderr with status 1. A cancellation
that reaches `main` exits with status 130, or with 128 plus the signal number
when a signal interrupted a live prompt. Either exit is silent unless related
errors are attached, in which case those print instead. `try_run` and
`try_run_from` are the non-reporting paths for embedding and tests. `run_with`
starts from an already constructed command.

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

Serde's derives need `#[serde(crate = "climax::serde")]` when serde is reached
through `climax`.

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
