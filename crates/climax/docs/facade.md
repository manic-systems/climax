# Climax facade

This is the current facade policy. The API remains pre-1.0 and may change.

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
let shell = cx
    .select("Shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?
    .or_cancel()?;
```

An ordinary application therefore needs only `climax`. It re-exports
`PromptOutcome`, `Configurable`, the config type of every prompt, and the
review types that typed prompts return, consume, and configure.

The submitted value is the user's `Shell`, not `bang_core::Value`, and leaving
is a typed `PromptOutcome::Leave`. `or_cancel` turns it into a `Cancelled` error
for a handler that cannot go on without an answer, `into_option` and `unwrap_or`
give the fallbacks, and matching on the outcome handles leaving explicitly.
Select, multi-select, search, text,
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

`StatusMode::Auto` animates only on a suitable transient terminal and resolves
to plain lines otherwise, so a piped run still reports its final message on
success and its failure message on failure, one line each. Plain, live, and
silent modes override it, and silent mode never emits status or final-message
text. Set `StatusMode::Silent` for a `--quiet` flag. A status started with `during` prints
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

```rust,ignore
let value = cx
    .status("scanning history")
    .spinner()
    .during(|| scan())?;
```

`Status::widget` takes a `screw::WidgetRef` and draws it in the live region,
after the spinner when `.spinner()` is set. The message remains what plain
status mode prints. The widget is drawn on the coordinator's thread, so any
state it reads must be shared, for example an `Arc<AtomicUsize>`, and
`StatusRuntime::mark_dirty` asks for a redraw after that state changes. A
widget that blocks or panics affects the coordinator. A panic removes the
widget instead of killing the thread, and only that status's `finish` or the
closing flush of the application lifecycle reports it. Every coordinator request
is bounded, and one that gets no answer within ten seconds produces an error
naming a blocking widget. This replaces a
separate screw runtime, which would compete with the coordinator for the
terminal.

Status cleanup occurs on success, error, cancellation, and unwinding. The
operation error takes precedence over a cleanup error, including cancellation,
so a cancelled operation still reports its status teardown as a related error.
Cleanup continues after an earlier failure and retains subsequent failures as
related errors. `Context` has a single owner. Policy setters take `&mut self`
and there is no `Clone`, so no second handle can prompt with a superseded
driver. A prompt builder already made from a `Context` keeps the driver it
captured, so `set_interaction_mode` or a terminal change afterwards only affects
builders made later. `Context` is also neither `Send` nor `Sync` in any feature configuration, so
it stays on the thread that built it. `Output` handles stay cloneable and share
their lifecycle state.

A notice blocks until it has been written and returns the write result, so it
stays ordered against the caller's own writes to the same stream. While a
prompt or terminal application holds the lease, the notice is queued and the
call returns at once. A transient line whose write fails stays queued and
resumes after the bytes the writer already accepted. At most 1024 lines wait,
and the oldest are dropped beyond that. A broken pipe or EIO ends retries for
the rest of the run. The next status finish or the lifecycle commit reports
the failure together with the number of dropped lines. Context builders which
can trigger that cleanup return `Result<Context>`.

```rust,ignore
let cx = Context::new()
    .with_terminal_capabilities(capabilities)?
    .with_status_mode(climax::terminal::StatusMode::Silent)?;
```

## Application lifecycle

The normal executable entry point lets Climax own parse signals, diagnostics,
and process status.

```rust,ignore
fn main() -> std::process::ExitCode {
    climax::main::<Cli, _>(run)
}
```

`main_with` is the same entry point for an application that takes no arguments.

```rust,ignore
fn main() -> std::process::ExitCode {
    climax::main_with(|cx: climax::Context| -> climax::Result<()> { run(&cx) })
}
```

### Exit codes

`main` and `main_with` map outcomes to exit codes in one place.

- 0 for success, and for help and version output on stdout.
- 1 for an application error, printed to stderr as `error: ...`.
- 2 for a parse failure printed to stderr (`main` only).
- 141 when writing help or version output fails because stdout is a closed pipe (`main` only), and 1 for any other write failure.
- 130 for a cancellation, including `Error::cancelled()`, or 128 plus the signal
  number when a signal interrupted a live prompt. Either exit is silent unless
  related errors are attached, in which case those print instead.
- The code given to `Error::with_exit_code` (0 becomes 1), for an error of any kind. A
  cancellation with an explicit code stays silent and any other error is still
  printed.

`try_run` and `try_run_from` are the non-reporting paths for embedding and
tests. `run_with` starts from an already constructed command.

`climax::ResultExt`, imported by name and absent from the prelude so that
`anyhow::Context` can sit beside `climax::prelude::*`, adds `.context(message)` and `.app_err()` to any
`Result` whose error is a `Send + Sync` `std::error::Error`, so
`text.parse::<u32>().context("reading the count")?` becomes an application
error that reads `reading the count: invalid digit found in string` and keeps
the original as its source. A `Box<dyn std::error::Error + Send + Sync>`
converts into `Error` with `?`.

With the optional `anyhow` feature, off by default, `anyhow::Error` converts
into `Error` with `?` as well. The anyhow error is kept whole as the source, so
its context chain prints under `Caused by:`.

## Testing

`climax::testing` runs an application in-process, with the reporting and exit
codes of `main` and `main_with`. `testing::run(args, script, handler)` parses
`args` like `main`, and `testing::run_with(script, handler)` is the form for an
application without arguments. Both return an `Outcome` with `exit_code`,
`stdout`, `stderr` and the `error`, and nothing touches the process.

A `Script` lists the answer to each prompt in order, with `select_nth`,
`multi_select_nth`, `text`, `text_attempts`, `confirm`, `enter`, `esc` and the
`keys` escape hatch. Each method answers exactly one prompt. A script with
input the application never read fails the run with a panic that counts the
unused scripts and events, and a prompt after the script ends is an
`ErrorKind::InputEnded` error. `Capture` is the cloneable `Write + Send`
buffer behind the output streams, for tests that build their own `Context`
with `with_output_writer`.

`Capture` is always available. `Script` and `run_with` need `interactive`, and
`run` needs `interactive` and `parse`.

## Results and sideband output

Durable stdout contains application results. Human context, diagnostics, and
transient status are separate concerns.

- `result` registers the invocation's one canonical finite result.
- `notice` writes human-only context through the transient channel (stderr
  unless `with_transient_writer` moves it, or `with_diagnostic_writer` when
  the `render` feature is off) in text mode and is suppressed in JSON mode.
- Application errors remain diagnostics on stderr.
- Statuses and prompts use the transient terminal, which is process stdin and
  stderr unless `with_terminal` moves it. `with_terminal` takes one read-write
  handle, normally `/dev/tty`, so `tool 2>log` still prompts and animates on
  the user's terminal. `with_transient_writer` moves statuses and notices only.
- `stream` writes zero or more values immediately and uses JSON Lines in JSON
  mode.

A finite result has one semantic value and two projections.

```rust,ignore
cx.output()
    .result(&scan_result)
    .text(|result| scan_view(result))
    .emit()?;
```

The `structured` feature uses `serde::Serialize` for the JSON projection. The
text closure is only evaluated in text mode. Climax encodes and holds the
selected projection, then writes and flushes it only after the application
handler returns success. A later handler error discards it. All `Output`
handles from `Context::output` share the result slot, so a second finite result
or mixing finite and streaming output is an output-policy error.

Streams have weaker atomicity. Values may already have reached stdout when
later application work fails. Making every command inherit streaming semantics
would hide that distinction.

## Boundary rule

A lower-level type may appear in a `climax` public signature only when it is an
unavoidable part of the ordinary application workflow and has no facade-owned
equivalent. Advanced widget, session, rendering, and parser APIs fail that test.
The ordinary prompt outcome and its configuration are the exception, since
`climax` re-exports `PromptOutcome`, `Configurable`, the prompt config types,
and the review types.

Those APIs stay reachable without a manifest entry, because `climax` re-exports
each component crate whole behind its feature.

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

A cancelled `select`, `multi_select`, `search`, `text`, `password`, `confirm`,
`date`, `number`, or `review` resolves to `PromptOutcome::Leave`, leaving the
handler to decide whether leaving is an error, and a custom interaction driver
cannot reach past that. Exit 130 without a signal comes from
`with_terminal_application` or a direct `bang` dependency that itself builds a
cancelled error. Ctrl-C and Ctrl-D are offered to the widget first. A widget or
`ActionBinding` that claims either key keeps the session running, and only an
ignored Ctrl-C cancels or an ignored Ctrl-D ends input.

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
- `anyhow` converts `anyhow::Error` into `Error` with `?`, keeping its chain as
  the source. It is not a default feature.

The five features above are on by default. The dependency fixtures under
`tests/compile-fixtures` build and test `climax` with no default features and
with each of `derive`, `interactive`, `parse`, `render` and `structured` alone.
Another fixture checks that a direct dependency on a component unifies with the
one `climax` re-exports.

The README code blocks are compiled as `climax` doctests only when `derive`,
`interactive`, `render` and `structured` are all on.

## Text measurement and tables

Screw measures text per grapheme cluster, so `screw::width`, `screw::truncate`,
`screw::pad` and `Surface::write` agree on emoji sequences, flags and combining
marks. A tab expands to the next multiple of eight columns in `width`,
`truncate` and `Surface::write`.

Human table presentation remains separate from the result contract. Screw has
an aligned `Table` with display-width-aware columns. A caller can build one
through `climax::screw` and render it into the text projection of a result,
and the JSON shape is unaffected. Climax itself does not construct tables. A
cell may span several rows. A table wider than the available columns shrinks
its flexible columns and truncates or wraps their cells, and it fits itself to
the columns left when it follows other content on a row.
