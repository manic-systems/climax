# climax

## max your CLI.

This workspace contains four user-facing Rust libraries:

- `pound`: derive-first argument parsing;
- `screw`: retained terminal rendering;
- `bang`: typed interactive prompts;
- `climax`: an application facade that composes the other three.

Each component is useful on its own. Applications that want the integrated path
can use `climax`; applications that need a component's full API should depend on
that component directly.

## Typed interaction with Bang

```rust
#[derive(Debug)]
enum Shell {
    Bash,
    Zsh,
}

let outcome = bang::select("shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let bang::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
# Ok::<(), bang::Error>(())
```

The crate root is the normal typed workflow. Custom widgets, raw values,
sessions, action bindings, event replay, scripted interactions, and custom
session drivers live under `bang::advanced`.

Each prompt implements `bang::Configurable` with its own configuration type.
Configuration controls presentation and initial input state; choices and their
typed values remain on the prompt builder. An action-free review returns
`PromptOutcome<Vec<Reviewed<T>>>`. Adding its first intrinsic review action
transitions to `ReviewPromptWithActions<T, A>` and returns `ReviewOutcome<T, A>`.

## The Climax application path

```rust
use climax::prelude::*;

climax::run_with((), |cx, ()| {
    let outcome = cx
        .select("shell")
        .choice("bash", "bash")
        .choice("zsh", "zsh")
        .interact()?;
    let bang::PromptOutcome::Submit(shell) = outcome else {
        return Ok(());
    };

    cx.output()
        .result(&shell)
        .text(|shell| *shell)
        .emit()
})?;
# Ok::<(), climax::Error>(())
```

The `climax` root and prelude intentionally contain only the application happy
path. They do not re-export the component crates. Add `pound`, `screw`, or
`bang` as a direct dependency when using its standalone or advanced API.

Deriving `pound::Parse` requires an explicit dependency on `pound` with its
`derive` feature. The generated code refers to `::pound`, so a transitive
dependency through `climax` is not sufficient.

`Context` owns terminal policy and logical output channels. Interactive prompts
require terminal-capable stdin and transient stderr by default; applications
can force or disable interaction explicitly. Status widgets share one transient
renderer, so multiple animations compose safely and prompts temporarily suspend
them while taking exclusive input. Policy builders which may need to tear down
active presentation, including `with_terminal_capabilities` and
`with_status_mode`, return `Result<Context>` rather than hiding cleanup errors.

Successful commands may register one finite result. Climax buffers that result
until the handler succeeds, renders its text projection for people, and
serializes its natural data shape in JSON mode. Human-only notices use stderr
in text mode and are suppressed in JSON mode. Streaming is explicit and emits
JSON Lines because it cannot share the finite result's commit-on-success
guarantee.

The suite is at an early stage and its APIs are not yet stable.
