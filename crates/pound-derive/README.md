# pound-derive

Derive macros for [pound](https://crates.io/crates/pound). `#[derive(Parse)]`
turns a struct into a flat command and an enum into a subcommand tree, and
`#[derive(ValueEnum)]` makes a unit enum a choice type.

Depend on `pound` with its `derive` feature instead of using this crate
directly. The expansion lives in `pound-derive-impl`.
