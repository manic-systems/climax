# climax

## max your CLI.

`climax` is a batteries-included CLI crate that ties together..

- `pound` for derive-first argument parsing
- `screw` for retained terminal rendering
- `bang` for interactive terminal widgets
- `caress` for shell completions and man pages

every `climax::main` program answers shell completion for bash, zsh, fish,
nu, elvish and PowerShell. `climax::install` renders the completion scripts
and mdoc man pages a package installs beside the binary. every TAB runs the
program, so link it statically for the fastest completion, with
`RUSTFLAGS='-C target-feature=+crt-static'` or `pkgsStatic` in nix, which
spares each TAB the dynamic loader's third of a millisecond.

it is still a work in progress.
