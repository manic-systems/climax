# climax

## max your CLI.

five libraries, each usable on its own

- `pound` parses arguments, derive first
- `screw` renders the terminal from retained frames
- `bang` asks typed interactive prompts
- `climax` is the application facade over `pound`, `screw` and `bang`
- `caress` answers shell completion and renders completion scripts and man pages

`bang-run` is the `bang` command line tool, the 0.0.1 `bang` command is now `cargo install bang-run`

depend on `climax` alone, it re-exports each component whole behind its feature

## The climax application path

```rust,no_run
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

```rust,no_run
use std::process::ExitCode;

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

fn main() -> ExitCode {
    climax::main(run)
}

fn run(cx: Context, args: Args) -> climax::Result<()> {
    cx.output()
        .result(&args.shell)
        .text(|shell| format!("{shell:?}"))
        .emit()
}
```

`crates/climax/examples/pkg.rs` is a larger derived app with subcommands, `--json`, `--color`,
progress and a confirm

an error from another crate converts with `?` when it is an `io::Error`, a `String`, a `&str` or a
`Box<dyn Error + Send + Sync>`. any other error type needs `ResultExt::app_err` or
`ResultExt::context`, and the `anyhow` feature adds `From<anyhow::Error>`. an i/o error anywhere
in the source chain keeps its kind, so a wrapped `BrokenPipe` still exits 141

exit codes, output formats, status lines and testing are in the crate docs

## Completion

`climax::main` answers shell completion for bash, zsh, fish, nu, elvish and PowerShell, and
`climax::install` renders the scripts and man pages a package installs. link statically for the
fastest completion

## Platforms

unix only. `pound` and `caress` are tested on Linux and macOS, `screw`, `bang` and `climax` on
Linux only. minimum Rust version 1.89, edition 2024
