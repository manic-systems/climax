# climax

## max your CLI.

five libraries, each usable on its own

- `pound` parses arguments, derive first
- `screw` renders the terminal from retained frames
- `bang` asks typed interactive prompts
- `climax` is the application facade over `pound`, `screw` and `bang`
- `caress` answers shell completion and renders completion scripts and man pages

depend on `climax` alone, it re-exports each component whole behind its feature

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
