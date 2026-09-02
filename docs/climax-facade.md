# Climax Facade Shape

Date: 2026-07-04

This is an internal design note for the intended `climax` facade. It describes
the application-facing API we want, not a stabilized public contract.

## Facade Promise

`climax` is the application crate for the ecosystem. `pound`, `screw`, and
`bang` remain reusable standalone crates; `climax` composes their public
interfaces into a boring path for common CLI applications.

The common path should let an application:

- parse a command type;
- run app logic with a facade `Context`;
- ask prompts;
- show status/progress;
- write text or JSON output;
- return one facade error type.

Users should not need to assemble `bang_terminal`, `bang_screw`, or `screw`
runtime objects for ordinary prompt/status/output flows. The lower-level crates
should still be reachable as escape hatches behind their feature gates.

## Current Shape

The main parsed entry point is:

```rust
climax::run::<Args, _>(|cx, args| {
    // app logic
    Ok(())
})
```

`run` is available behind the `parse` feature and requires `Args: pound::Parse`.
For code that already has a command value, the lower-level entry point is:

```rust
climax::run_with(args, |cx, args| {
    // app logic
    Ok(())
})
```

`Context` is currently a small facade handle. It carries default output
formatting policy and exposes:

- `cx.prompt()` behind `interactive`;
- `cx.status(message)` behind `render`;
- `cx.output()` behind `interactive`.

Prompt facade:

```rust
let shell = cx
    .prompt()
    .select("shell")
    .option("bash")
    .option("zsh")
    .run_string()?;
```

Status facade:

```rust
let status = cx.status("working").spinner().start();
// work
status.finish()?;
```

Output facade:

```rust
cx.output().print_value(&value)?;
cx.output()
    .with_format(climax::output::Format::Json)
    .print_value(&value)?;
```

Default output format can be configured on `Context`:

```rust
let cx = cx.with_output_format(climax::output::Format::Json);
cx.output().print_value(&value)?;
```

The free modules still exist:

- `climax::prompt`
- `climax::status`
- `climax::output`

These are convenience APIs and implementation building blocks for `Context`.

## Feature Shape

The facade should remain feature-shaped rather than treating any one lower-level
crate as the base:

- `parse`: enables the parsed `run` path and `pound` integration.
- `render`: enables status rendering and `screw` integration.
- `interactive`: enables prompts/output and `bang` integration; it currently
  implies `render` because live prompts render through `screw`.
- `pty-overlay`: enables overlay integration.

There should not be a separate JSON feature unless JSON output gains a real
dependency or behavior split. Current value-to-JSON formatting is owned by
`bang-core` and is available through `interactive` output.

## Prelude Policy

The prelude should favor the facade path:

- `Context`
- `Error`
- `Result`
- `run`
- `run_with`
- derive/parser names when `parse` is enabled
- facade modules and facade context types

Lower-level public interfaces from `pound`, `screw`, and `bang` can remain
available as escape hatches, but the prelude should not become a dumping ground
for every adapter/runtime type. If the re-export surface gets noisy, prefer an
explicit escape-hatch module such as `climax::parts` or direct crate re-exports
such as `climax::pound`, `climax::screw`, and `climax::bang`.

## Error Shape

`climax::Error` should wrap lower-level failures by facade concern:

- argument parsing failures behind `parse`;
- drawing/stream failures behind `render` or `interactive`;
- interactive session failures behind `interactive`;
- facade validation/message errors always.

The exact variant names are less important than preserving feature hygiene and
not making `screw`, `bang`, or `pound` implicit base dependencies.

## Ownership Rules

Facade implementation should reuse lower-level public interfaces, but ownership
of core behavior should stay with the lower-level crate that owns the concern:

- parsing belongs to `pound`;
- retained rendering belongs to `screw`;
- widget state and value formatting belong to `bang-core`;
- live prompt/session orchestration for `bang` rendered by `screw` belongs to
  `bang-screw`;
- `climax` owns the application-facing composition and error mapping.

Duplication in `climax` is acceptable only for facade ergonomics, not for
reimplementing lower-level behavior.

## Near-Term Direction

Likely next steps:

1. Keep deepening `Context` as the main application surface.
2. Decide whether `Context` should carry more configuration such as terminal
   policy, noninteractive behavior, stderr/stdout routing, or prompt replay.
3. Keep free module APIs available where they are useful, but make examples lead
   with `climax::run` and `Context`.
4. Tighten prelude exports once the facade surface is clearer.
5. Add compile-time facade examples later, after the API shape settles.

Directions to avoid:

- making `screw` the implicit base crate for all `climax` users;
- making `climax` users assemble adapter internals for common flows;
- moving PTY emulation or widget state into `climax`;
- hiding lower-level crates so thoroughly that advanced users lose escape
  hatches.
