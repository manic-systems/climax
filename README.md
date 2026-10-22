# climax

## max your CLI.

five libraries, each usable on its own

- `pound` parses arguments, derive first
- `screw` renders the terminal from retained frames
- `bang` asks typed interactive prompts
- `climax` is the application facade over `pound`, `screw` and `bang`
- `caress` answers shell completion and renders completion scripts and man pages

Each component is useful on its own. Applications that want the integrated path
can depend on `climax` alone, which re-exports each component whole behind the
feature that enables it. Exact version pins keep one `climax` release on one
tested set of component versions.

With the default `complete` feature, `climax::main` and `climax::try_run`
answer shell completion for bash, zsh, fish, nu, elvish and PowerShell before
they parse anything. `climax::install` renders the completion scripts and mdoc
man pages a package installs beside the binary. Every TAB runs the program, so
link it statically for the fastest completion, with
`RUSTFLAGS='-C target-feature=+crt-static'` or `pkgsStatic` in nix, which
spares each TAB the dynamic loader's third of a millisecond.

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

a cancelled prompt resolves to `PromptOutcome::Leave`, and `or_cancel` turns that into a
`Cancelled` error that exits 130

with `derive`, a struct deriving `climax::Parse` is the command, and `climax::main` parses,
runs the handler and picks the exit code

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

exit codes, output formats, status lines and testing are in the crate docs

## Completion

`climax::main` answers shell completion for bash, zsh, fish, nu, elvish and PowerShell, and
`climax::install` renders the scripts and man pages a package installs. link statically for the
fastest completion

## Platforms

unix only, tested on Linux and macOS, minimum Rust version 1.88, edition 2024
