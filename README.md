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

## Typed interaction with Bang

```rust,no_run
#[derive(Debug)]
enum Shell {
    Bash,
    Zsh,
}

let outcome = climax::bang::select("Shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let climax::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
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

climax::run_with((), |cx, ()| {
    let outcome = cx
        .select("Shell")
        .choice("bash", "bash")
        .choice("zsh", "zsh")
        .interact()?;
    let PromptOutcome::Submit(shell) = outcome else {
        return Ok(());
    };

    cx.output()
        .result(&shell)
        .text(|shell| *shell)
        .emit()
})?;
# Ok::<(), climax::Error>(())
```

```rust,no_run
use climax::prelude::*;

climax::run_with((), |cx, ()| {
    let PromptOutcome::Submit(proceed) = cx.confirm("Deploy now?").interact()? else {
        return Ok(());
    };
    cx.output().result(&proceed).text(|proceed| proceed.to_string()).emit()
})?;
# Ok::<(), climax::Error>(())
```

`Context` also hands out `password`, `date` and `number::<T>`. A cancelled
prompt resolves to `PromptOutcome::Leave`. A signal during a prompt restores the
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
`#[derive(climax::ValueEnum)]` with expansion rooted at `::climax::pound`. Serde
derives reached through climax need `#[serde(crate = "climax::serde")]`.

```rust
use climax::prelude::*;

#[derive(Clone, Copy, Debug, climax::serde::Serialize, ValueEnum)]
#[serde(crate = "climax::serde")]
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

#[derive(climax::serde::Serialize)]
#[serde(crate = "climax::serde")]
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
            let PromptOutcome::Submit(true) = cx.confirm("Deploy now?").interact()? else {
                return Ok(());
            };
        }
        cx.output().result(&"deployed").text(|state| *state).emit()
    })
}
```

### Exit codes

`climax::main` exits 0 on success, 1 for an application error, 2 for a parse
failure and 130, or 128 plus the signal number, for a cancellation. An
application that wants its own mapping calls `try_run` and converts the
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

The suite is at an early stage and its APIs are not yet stable.
