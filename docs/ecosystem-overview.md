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

every edge to a climax-family crate, dev-dependencies included, is pinned exactly (`=x.y.z`) and
`crates/climax/tests/workspace_pins.rs` enforces it, so one `climax` release names one tested set
of `bang` and `screw` versions. `pound` and `caress` release on their own and are required at the
range `climax` was tested against. `climax-derive` requires `pound-derive-impl` at a caret range
that resolves to the version `pound` pins

## Derive paths

`#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` expand to paths rooted at
`::climax::pound`, so `climax` alone is enough. serde derives reached through `climax` need
`#[serde(crate = "climax::serde")]`

## Fixtures

`crates/climax/tests/dependency_fixtures.rs` builds and tests each fixture under
`tests/compile-fixtures` on its own

- `climax-no-default-features`, and `climax-complete-only`, `climax-derive-only`,
  `climax-interactive-only`, `climax-parse-only`, `climax-render-only` and
  `climax-structured-only`
- `climax-only`, an application built from `climax` alone, which also runs the README snippets
- `climax-with-components`, which passes values from direct `pound`, `screw` and `bang`
  dependencies into `climax` to prove the versions unify
- `pound-only`, `pound-no-derive`, `pound-no-help`, `pound-no-std`, `screw-only` and `bang-only`
