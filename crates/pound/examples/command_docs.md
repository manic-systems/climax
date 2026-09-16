`command_docs` exercises the public `Parse::SPEC` API with nested flattened arguments,
global options, aliases, enum values, hidden entries, and nested subcommands. Its
command selector lives inside recursively flattened structs, and a flattened
enum variant contributes the download commands without a wrapper name. The demo
prints parsed requests without downloading anything.

Run its custom help renderer for the root or a command path.

```sh
nix develop --command cargo run -p pound --example command_docs -- help
nix develop --command cargo run -p pound --example command_docs -- help fetch
nix develop --command cargo run -p pound --example command_docs -- help cache clean
```

Generate a completion script, then source the matching file in Bash or Fish.

```sh
nix develop --command cargo build -p pound --example command_docs
target/debug/examples/command_docs completions bash > /tmp/command_docs.bash
target/debug/examples/command_docs completions fish > /tmp/command_docs.fish
```

Add `target/debug/examples` to your shell's `PATH` to invoke the example as
`command_docs`. The scripts contain the generator's absolute executable path.
Regenerate them after moving or rebuilding the executable at another location.
Each completion request invokes the example's hidden `__complete` command, which
walks the metadata for the words already entered.

Command walkers use `CommandSpec::subcommands()` and `find_sub()` to see the
effective children. The raw `subs` slice also contains virtual enum wrappers and
does not include selectors contributed through flattened structs.

The generator suggests option names, subcommand names, and declared enum values.
It supports aliases, short option clusters, inherited globals, and the `--`
delimiter. It leaves freeform values and filesystem paths to the user and does not
enforce groups or conflicts while completing a partial command.

Run the Rust checks and exercise both generated scripts in real shells.

```sh
nix develop --command cargo test -p pound --example command_docs
nix develop --command nix shell nixpkgs#bashInteractive nixpkgs#fish --command nu --no-config-file crates/pound/examples/smoke_command_docs.nu
```
