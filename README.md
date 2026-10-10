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

The suite is at an early stage and its APIs are not yet stable.
