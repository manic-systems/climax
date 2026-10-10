# climax

## max your CLI.

This workspace contains four user-facing Rust libraries.

- `pound` parses arguments, derive first.
- `screw` renders the terminal from retained frames.
- `bang` asks typed interactive prompts.
- `climax` is an application facade that composes the other three.

Each component is useful on its own. Applications that want the integrated path
can depend on `climax` alone, which re-exports each component whole behind the
feature that enables it. Exact version pins keep one `climax` release on one
tested set of component versions.

## Platforms and Rust version

The suite targets Unix. It is tested on Linux and macOS, and Windows is not
supported yet because terminal input, raw mode and signal handling use Unix
file descriptors. The minimum supported Rust version is 1.88, set as
`rust-version` in the workspace package metadata. The crates use edition 2024
and let chains, and development follows current stable.

## Typed interaction with Bang

```rust,no_run
#[derive(Debug)]
enum Shell {
    Bash,
    Zsh,
}

let shell = climax::bang::select("Shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?
    .or_cancel()?;
# Ok::<(), climax::bang::Error>(())
```

Bang is an interactive layer over screw. Every bang widget is also a
`screw::Widget` that draws into a screw `Surface`, so any terminal renderer can
show one by copying the surface cells, and `bang::screw` re-exports screw whole
for custom widget authors. Terminal input, raw mode, screen and signal handling
live in `bang::terminal`.

The crate root is the normal typed workflow. Custom widgets, raw values,
sessions, action bindings, event replay, scripted interactions, and custom
session drivers live under `bang::advanced`.

Each prompt implements `bang::Configurable` with its own configuration type,
and offers the same settings (header, page size, wrap, initial selection and so
on) as direct builder methods. Choices and their typed values remain on the
prompt builder. `select`, `multi_select`, `search` and `review` take the visible
header text, and a list prompt with no choices is rejected. `password`,
`confirm`, `date` and `number::<T>` cover masked text, booleans, dates and any
`FromStr` type. An action-free review returns
`PromptOutcome<Vec<Reviewed<T>>>`. Adding its first intrinsic review action
transitions to `ReviewPromptWithActions<T, A>` and returns `ReviewOutcome<T, A>`.

## The Climax application path

```rust,no_run
use climax::prelude::*;

fn main() -> std::process::ExitCode {
    climax::main_with(|cx| {
        let shell = cx
            .select("Shell")
            .choice("bash", "bash")
            .choice("zsh", "zsh")
            .interact()?
            .or_cancel()?;

        cx.output()
            .result(&shell)
            .text(|shell| *shell)
            .emit()
    })
}
```

`main_with` runs a handler that takes no arguments, reports its error the way
`climax::main` does and returns the `ExitCode`.

```rust,no_run
use climax::prelude::*;

fn main() -> std::process::ExitCode {
    climax::main_with(|cx| {
        let proceed = cx.confirm("Deploy now?").interact()?.or_cancel()?;
        cx.output().result(&proceed).text(|proceed| proceed.to_string()).emit()
    })
}
```

`Context` also hands out `password`, `date` and `number::<T>`. A cancelled
prompt resolves to `PromptOutcome::Leave`, and `or_cancel` turns that into a
`Cancelled` error that ends the handler with exit 130. `into_option` and
`unwrap_or` cover the cases where leaving has a sensible fallback, and a handler
that wants to act on leaving matches on the outcome.

```rust,no_run
use climax::prelude::*;

fn main() -> std::process::ExitCode {
    climax::main_with(|cx| match cx.text("Name").interact()? {
        PromptOutcome::Submit(name) => cx.output().result(&name).text(|name| name.clone()).emit(),
        PromptOutcome::Leave => cx.output().result(&"anonymous").text(|name| *name).emit(),
    })
}
```

`confirm` accepts `y` and `n` as immediate answers and highlights No unless
`default(true)` says otherwise. A submitted prompt leaves a dimmed one-line
summary in the scrollback, such as `Deploy now? › no`, and leaving leaves
nothing. A prompt's `summary(false)` turns it off for that prompt, and
`bang::Interaction::with_summaries(false)` turns it off for every prompt on that
driver. `NO_COLOR` removes the dimming. A signal during a prompt restores the
terminal and surfaces as a `Cancelled` error whose `Error::signal()` holds the
signal number, and `climax::main` exits with 128 plus that number. The process
is never killed by the signal itself, and a signal the host ignored before the
session stays ignored. Any other cancellation that reaches `main` exits 130.

`Context` prompts return `climax::PromptOutcome`, and `climax` also re-exports
`Configurable`, the prompt config types, and the review types. The components
themselves are reachable as `climax::pound` under `parse`, `climax::bang` under
`interactive`, `climax::screw` under `render`, and `climax::serde` under
`structured`, so an application needs no direct dependency on any of them.

The `derive` feature, on by default, provides `#[derive(climax::Parse)]` and
`#[derive(climax::ValueEnum)]` with expansion rooted at `::climax::pound`. With
`structured` it also provides `#[climax::serde(Serialize, Deserialize)]`, which
derives serde's traits rooted at `climax::serde` and keeps the item and its
other attributes. Put it above any other derive.

```rust
use climax::prelude::*;

#[climax::serde(Serialize)]
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Shell {
    Bash,
    Zsh,
}

/// configure a shell
#[derive(Parse)]
struct Args {
    /// shell to configure
    #[pound(long)]
    shell: Shell,
}

climax::try_run_from(["--shell", "zsh"], |context, args: Args| {
    context
        .output()
        .result(&args.shell)
        .text(|shell| format!("{shell:?}"))
        .emit()
})?;
# Ok::<(), climax::Error>(())
```

`Context` owns terminal policy and logical output channels. Interactive prompts
require terminal-capable stdin and transient stderr by default, and applications
can force or disable interaction explicitly. Status widgets share one transient
renderer, so multiple animations compose safely and prompts temporarily suspend
them while taking exclusive input. Policy builders which may need to tear down
active presentation, including `with_terminal_capabilities` and
`with_status_mode`, return `Result<Context>` rather than hiding cleanup errors.

Successful commands may register one finite result. Climax buffers that result
until the handler succeeds, renders its text projection for people, and
serializes its natural data shape in JSON mode. Human-only notices use the
transient channel (stderr by default) in text mode and are suppressed in JSON
mode. A notice blocks until it is written, except while a prompt or terminal
application holds the terminal, when it is queued and written on release.
Streaming is explicit and emits JSON Lines because it cannot share the finite
result's commit-on-success guarantee. Streamed output is written around the
live status region, and while a prompt holds the terminal a stream write from
another thread waits for the release.

## Recipes

### A `--json` flag

`Context` starts in text mode. Switch it with `Context::set_output_format`, or
build one with `Context::with_output_format` when you construct the context
yourself, before taking `cx.output()` so the handle you hold sees the format.
In `Format::Json` a registered result is serialized as its natural JSON shape
rather than its text projection, a `stream` writes JSON Lines, and `notice`
output is suppressed so stdout and stderr stay machine readable.

```rust,no_run
use climax::prelude::*;

#[climax::serde(Serialize)]
struct Report {
    files: usize,
}

/// scan the tree
#[derive(Parse)]
struct Args {
    /// print the result as JSON
    #[pound(long)]
    json: bool,
}

fn main() -> std::process::ExitCode {
    climax::main(|mut cx, args: Args| {
        if args.json {
            cx.set_output_format(Format::Json);
        }
        cx.output().notice("scanning")?;
        let report = Report { files: 3 };
        cx.output()
            .result(&report)
            .text(|report| format!("{} files", report.files))
            .emit()
    })
}
```

### A `--yes` flag that skips prompts

A prompt is ordinary control flow, so skip it by not asking. Without a
terminal on stdin a prompt fails with `ErrorKind::InteractionUnavailable`
instead of guessing an answer, which is what a script that forgot `--yes`
should see.

```rust,no_run
use climax::prelude::*;

/// deploy the build
#[derive(Parse)]
struct Args {
    /// do not ask for confirmation
    #[pound(long)]
    yes: bool,
}

fn main() -> std::process::ExitCode {
    climax::main(|cx, args: Args| {
        if !args.yes {
            if !cx.confirm("Deploy now?").interact()?.unwrap_or(false) {
                return Ok(());
            }
        }
        cx.output().result(&"deployed").text(|state| *state).emit()
    })
}
```

### Exit codes

`climax::main` and `climax::main_with` exit with

| Code | Meaning |
|---|---|
| 0 | success, and help or version output |
| 1 | an application error, printed to stderr as `error: ...` |
| 2 | a parse failure (`main` only) |
| 130 | a cancellation, silent unless related errors are attached |
| 128 plus N | a prompt interrupted by signal N, silent like 130 |
| any non-zero `u8` | the code set with `Error::with_exit_code`, for an error of any kind, where 0 becomes 1 |

`Error::cancelled()` reports a cancellation of your own, and `with_exit_code`
picks the code for any error. A cancellation with an explicit code stays silent
and any other error is still printed.

```rust,no_run
use climax::{ResultExt as _, prelude::*};

fn main() -> std::process::ExitCode {
    climax::main_with(|cx| {
        let count = "7".parse::<u32>().context("reading the count")?;
        if count > 5 {
            return Err(Error::message("too many").with_exit_code(3));
        }
        let PromptOutcome::Submit(true) = cx.confirm("Continue?").interact()? else {
            return Err(Error::cancelled());
        };
        Ok(())
    })
}
```

`ResultExt::context`, imported as `climax::ResultExt`, turns any `Send + Sync`
error into an application error that reads `message: source` and keeps the
source, and `app_err` keeps the source's own text. The prelude leaves it out so
that `anyhow::Context` can sit beside `climax::prelude::*`. A
`Box<dyn std::error::Error + Send + Sync>` converts with `?`, and with the
`anyhow` feature an `anyhow::Error` does too, keeping its chain as the source.

An application that wants a mapping beyond that calls `try_run` and converts the
`climax::Error` itself. Help and version arrive as a parse error that asks to
exit, which this recipe prints and treats as success.

```rust,no_run
use std::process::ExitCode;

use climax::prelude::*;

/// check the tree
#[derive(Parse)]
struct Args {}

fn run(_cx: Context, _args: Args) -> climax::Result<()> {
    Err(Error::message("tree is dirty"))
}

fn main() -> ExitCode {
    let Err(error) = climax::try_run(run) else {
        return ExitCode::SUCCESS;
    };
    let parse = error
        .source_error()
        .and_then(|source| source.downcast_ref::<climax::pound::Error>());
    if let Some(parse) = parse.filter(|parse| parse.is_exit()) {
        println!("{}", parse.render());
        return ExitCode::SUCCESS;
    }
    eprintln!("error: {error}");
    ExitCode::from(match error.kind() {
        ErrorKind::Parse => 2,
        ErrorKind::Cancelled => 130,
        ErrorKind::InteractionUnavailable => 3,
        _ => 1,
    })
}
```

### A `--quiet` flag

`StatusMode::Auto` animates on a terminal and prints one plain line per
finished status elsewhere. A `--quiet` flag selects `StatusMode::Silent`, which
prints no status or final-message text at all.

```rust,no_run
use climax::prelude::*;

/// build the project
#[derive(Parse)]
struct Args {
    /// print no status lines
    #[pound(long)]
    quiet: bool,
}

fn main() -> std::process::ExitCode {
    climax::main(|mut cx, args: Args| {
        if args.quiet {
            cx.set_status_mode(StatusMode::Silent)?;
        }
        cx.status("building").spinner().final_message("built").during(|| Ok(()))
    })
}
```

### Testing

`climax::testing` runs an application in-process the way `main` does. A
`Script` answers prompts, one method per prompt, `Capture` buffers collect
output, and the `Outcome` carries the exit code that `main` would return, the
text of both streams and the error. Leftover script input fails the run with a
panic that names how much was unused, so a test cannot script more than the flow
asks for.

```rust
use climax::{prelude::*, testing::{self, Script}};

/// deploy a build
#[derive(Parse)]
struct Args {
    /// the environment to deploy to
    env: String,
}

fn deploy(cx: Context, args: Args) -> climax::Result<()> {
    if !cx.confirm(format!("Deploy to {}?", args.env)).interact()?.or_cancel()? {
        return Err(Error::message("declined").with_exit_code(3));
    }
    cx.output().result(&args.env).text(|env| format!("deployed to {env}")).emit()
}

let yes = testing::run(["dev"], Script::new().confirm(true), deploy);
assert_eq!(yes.exit_code, 0);
assert_eq!(yes.stdout, "deployed to dev\n");

let no = testing::run(["dev"], Script::new().confirm(false), deploy);
assert_eq!((no.exit_code, no.stderr.as_str()), (3, "error: declined\n"));

let esc = testing::run(["dev"], Script::new().esc(), deploy);
assert_eq!(esc.exit_code, 130);
```

`Script` also has `select_nth`, `multi_select_nth`, `text`, `text_attempts`, `date`,
`enter` and `keys` for anything else. `testing::run_with` is the form for an
application without arguments, and `Capture` can be handed to
`Context::with_output_writer` when a test builds its own `Context`.

The suite is at an early stage and its APIs are not yet stable.
