# Ecosystem overview

## Products

| product  | responsibility                                                         |
|----------|------------------------------------------------------------------------|
| `pound`  | argument parsing, derives, command specs, raw matches                  |
| `screw`  | terminal surfaces, rendering, styles, widgets, terminal size, runtimes |
| `bang`   | typed interactive prompts, layered over `screw`                        |
| `caress` | shell completion, completion scripts and man pages                     |
| `climax` | application lifecycle, policy, output, status, composition             |

the products are independent. `climax` is the integrated path and re-exports each component whole
behind its feature, so it can be an application's only dependency

## Workspace roles

- `pound-derive-impl` holds pound's derive expansion, parameterised by a root path and a help
  switch
- `pound-derive` is the proc-macro wrapper rooted at `::pound`
- `climax-derive` is the proc-macro wrapper rooted at `::climax::pound`
- `screw-pty` is an unpublished test model of the terminal output screw emits
- `bang-core` holds bang's widget, event, value and session model
- `bang-terminal` is the terminal toolkit, reached as `bang::terminal`
- `bang-run` is the executable and configuration product built on pound and bang

`bang-core` and `bang-terminal` are implementation contracts versioned with their consumers, users
start at `bang`

## Dependency direction

```text
climax  ----> pound* ------> pound-derive* ----> pound-derive-impl
   |                                                    ^
   +--------> climax-derive* ---------------------------+
   |
   +--------> caress* -----> pound
   |
   +--------> screw*
   |
   +--------> bang* ------> bang-core -------> screw
                |               ^                ^
                +--> bang-terminal --------------+
                |
                +--> screw

bang-run ----> bang, bang-core, pound
screw-pty ---> screw
```

an asterisk marks an optional dependency. `screw` has no normal dependency on our crates

- component products do not depend on `climax`
- `pound` and `screw` do not acquire bang or climax concepts
- `climax` consumes the `bang` product, not bang's implementation crates

## Pins

every edge to a climax-family crate is pinned exactly (`=x.y.z`) and
`crates/climax/tests/workspace_pins.rs` enforces it, so one `climax` release names one tested set
of `bang` and `screw` versions. the dev edge from `pound-derive` to `pound` is the exception, it
stays path-only. `pound` and `caress` release on their own and are required at the range
`climax` was tested against. `climax-derive` requires `pound-derive-impl` at a caret range
that resolves to the version `pound` pins

## Derive paths

`#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` expand to paths rooted at
`::climax::pound`, so `climax` alone is enough. the derives under `climax::pound` expand to
`::pound` and need a direct `pound` dependency. `#[climax::serde(..)]` roots serde's derives and
helper attributes at `climax::serde`, and without `derive` a plain serde derive needs
`#[serde(crate = "climax::serde")]`

## Fixtures

`crates/climax/tests/dependency_fixtures.rs` builds and tests each fixture under
`tests/compile-fixtures` on its own, so `climax` builds with no default features and with each
feature alone

- `climax-no-default-features`, and `climax-anyhow-only`, `climax-complete-only`,
  `climax-derive-only`, `climax-interactive-only`, `climax-parse-only`, `climax-render-only` and
  `climax-structured-only`
- `climax-only`, an application built from `climax` alone, which also runs the README snippets
  and uses `climax::testing`
- `climax-with-components`, which passes values from direct `pound`, `screw` and `bang`
  dependencies into `climax` to prove the versions unify
- `pound-only`, `pound-no-derive`, `pound-no-help`, `pound-no-std`, `pound-color-only` and
  `pound-unicode-only`
- `screw-only`, `bang-only`, `bang-core-only`, `bang-terminal-only` and `caress-only`
