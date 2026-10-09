# Ecosystem overview

The workspace has four intended library products.

| Product | User-facing responsibility |
| --- | --- |
| `pound` | Argument parsing, derives, command specifications, and raw matches |
| `screw` | Terminal surfaces, rendering, styles, widgets, terminal size, and runtimes |
| `bang` | Typed interactive prompts and advanced custom interaction, layered over `screw` |
| `climax` | Application lifecycle, policy, output, status, and composition |

These are independent products, not layers that must always be consumed
together. `climax` is the convenient integrated path and re-exports each
component whole behind its feature, so it can be an application's only
dependency. Depending on a component directly remains fully supported.

## Workspace roles

The remaining crates have narrower roles.

- `pound-derive-impl` is the plain library crate holding Pound's derive
  expansion, parameterised by a root path and a help switch.
- `pound-derive` is the proc-macro wrapper around it, rooted at `::pound`.
- `climax-derive` is the proc-macro wrapper around it for `climax`, rooted at
  `::climax::pound`.
- `screw-pty` is an unpublished test-support model of the terminal output Screw
  emits (`publish = false`).
- `bang-core` holds Bang's widget, event, value, and session model, where
  `Widget` has `screw::Widget` as a supertrait.
- `bang-terminal` is the terminal toolkit, reached as `bang::terminal`. It holds
  `Decoder`, `TerminalEvents`, the raw mode, screen and signal guards, and one
  `CleanupStage`/`CleanupFailure`/`CleanupFailures` teardown vocabulary.
  Syscalls go through `rustix`, and signals are bang's own `bang::terminal::Signal`
  newtype (`INT`, `TERM`, `HUP`, `QUIT`).
- `bang-run` is the executable and configuration product built on Pound and
  Bang.

`bang-core` and `bang-terminal` contain public Rust items because separate
workspace crates must communicate. That does not make those items the friendly
standalone Bang interface. They are implementation contracts, versioned with
their consumers, and ordinary users start at `bang`. The live session runner and
session driver live inside `bang` itself.

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

An asterisk marks an optional dependency. `screw` has no normal dependency on
our crates, and its only workspace edge is a dev-dependency on `screw-pty`,
which depends back on `screw`. `bang`, `bang-core`, and `bang-terminal` all
depend on `screw`, and `bang-terminal` reads terminal size through
`screw::Viewport`.
Bang layers over `screw` instead of staying renderer-neutral because the
realistic embedder is another terminal UI, which consumes a cell grid more
easily than semantic views. List widgets also need their own layout to page
correctly, and they learn their page size from the layout measured on their last
render. A bang widget draws into a `screw::Surface` using screw's `Role`,
`Span`, `Style`, and `Theme`, and a renderer shows it by copying those cells.

Climax's Pound and Screw edges are direct optional dependencies enabled by its
`parse` and `render` features. The Bang edge is likewise optional under
`interactive`, and Climax does not reach through Bang to name its
implementation crates. The `climax-derive` edge is optional under `derive`.
Each component is re-exported whole as `climax::pound`, `climax::screw`, and
`climax::bang`, and `climax::serde` follows `structured`. Every workspace edge,
including dev-dependencies, is pinned exactly (`=x.y.z`) and a test enforces it,
so one `climax` release names one tested set of component versions.

The exact internal edges may evolve, but the product rule is stable.

- Component products do not depend on `climax`.
- `pound` and `screw` do not acquire Bang or Climax concepts.
- `screw` never depends on `bang`, and `bang` builds on `screw` directly.
- `climax` consumes the coherent `bang` product instead of assembling Bang's
  implementation crates itself.

`bang-run` is separate from the `bang` library name. It owns executable argument
and configuration plumbing, while prompt behavior and typed results stay in
`bang`.

## Public API policy

Three categories decide visibility.

1. A product interface is required to use a crate for its stated purpose and
   documented as a normal, supported path.
2. An advanced seam is available for custom integration, but outside the
   prelude and happy path.
3. Incidental implementation is public only for convenience and a candidate for
   narrowing before stabilization.

For Bang, the typed builders are the product interface.

```rust
let outcome = bang::select("Shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let bang::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
```

`bang::advanced` groups custom widgets, actions, raw `Value`, sessions, and
event replay. `bang` re-exports `screw` whole as `bang::screw` and the terminal
toolkit as `bang::terminal`, so a custom widget author needs no separate `screw`
dependency. Those items are advanced seams and typed prompt use does not need
them.

For Climax, the root and prelude endorse `main`, the non-exiting run variants,
`Context`, `Error`, `Result`, `PromptOutcome`, `Configurable`, the prompt
config types, and the review types. A normal facade signature should not
expose `bang_core::Value`, `ActionBinding`, `Session`, `screw::Runtime`, or
`pound::Matches`. Advanced users reach the full component APIs through
`climax::bang`, `climax::screw`, and `climax::pound` without a manifest entry,
and ordinary typed prompts, plain or configured, use the types the facade
re-exports at its root.

## Derive dependency

`#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]`, from the `derive`
feature, expand to paths rooted at `::climax::pound`, so `climax` alone is
enough. `#[derive(pound::Parse)]` with a direct `pound` dependency expands to
`::pound` and works as before. Serde derives reached through `climax` need
`#[serde(crate = "climax::serde")]`.

## Build shape

Each dependency fixture under `tests/compile-fixtures` is built and tested on
its own by `crates/climax/tests/dependency_fixtures.rs`. They cover `climax`
with no default features and with each of `derive`, `interactive`, `parse`,
`render` and `structured` alone, `climax-only`, which builds an application from
`climax` alone, and standalone `pound`, `screw` and `bang`. The
`climax-with-components` fixture passes values built through direct `pound`,
`screw` and `bang` dependencies into `climax`, which proves the versions unify.
`crates/climax/tests/workspace_pins.rs` checks the exact pins.

## Bang and Climax integration

- The user-facing `bang` facade owns typed select, multi-select, search, text,
  password, confirm, date, number, and review workflows. List prompts take a
  visible header and reject zero choices, and every prompt offers its settings
  as direct builder methods as well as `with_config`.
- Prompt-specific `Configurable` implementations cover ordinary presentation
  and initial input state without moving typed choices out of their builders.
- Action-free reviews use `PromptOutcome`. Adding the first intrinsic action
  transitions to an action-bearing builder with `ReviewOutcome`.
- Every prompt has a config type implementing `Configurable`. `PasswordConfig`,
  `ConfirmConfig`, `DateConfig` and `NumberConfig<T>` sit beside the list, text
  and review configs. List prompts clamp page keys at the ends of a list.
- `climax` depends on `bang` and delegates prompt construction to it rather
  than duplicating a prompt implementation.
- `climax` re-exports `PromptOutcome`, `Configurable`, the prompt config
  types, and the review types, and re-exports `bang` whole as `climax::bang`,
  so typed prompts, review, and `bang::advanced` need no direct `bang`
  dependency.
- Climax errors use facade-owned categories with opaque dependency sources.
- Climax owns help/version, parse diagnostics, application diagnostics, and
  process exit status through its executable entry point.
- Climax Context owns terminal capability, noninteractive behavior, logical
  output channels, and shared status presentation policy.
- Climax finite results commit and flush only after successful handler
  completion. Notices, diagnostics, transient status, and streaming output
  have separate channels and semantics.
- A notice blocks until it is written, except while a prompt or terminal
  application holds the terminal, when it is queued. `diagnostic().stream`
  follows the same rule, and every coordinator request is bounded by a ten
  second timeout. Streamed output from another thread is deferred until that lease releases, and the thread holding
  the lease gets an error.
- Bang interactions are injectable, and live, scripted, and advanced custom
  drivers use the same typed prompt builders.
- Multiple status animations share one transient renderer, and prompts suspend
  and restore that presentation around exclusive interaction.
- Screw owns physical viewport measurement and allocation. `screw::Viewport::of`
  measures a handle and `Viewport::FALLBACK` is the 80 by 24 default. Bang list
  widgets take their page size from the layout they measured on their last
  render.
- Screw measures text per grapheme cluster and expands tabs to eight-column
  stops, under the width policy documented in the `screw` crate docs.
  `Cell::new` takes a `&str` and returns `None` unless it is exactly one
  cluster, `Looping::new` takes any iterator of string-like frames, and
  `LiveRuntime::finish_recovering` returns the writer after a failed draw or a
  widget panic.
- Bang text input edits and masks by grapheme cluster, and
  `TextInput::cursor_grapheme_index` is the cursor position. The date prompt
  takes today and its default from the local time zone, live widgets receive
  ticks through `TerminalEvents::next_event_within`, and a form validates every
  field at submission.
- A second concurrent prompt on the default driver waits on the stdin lock and
  runs after the first. `InteractionBusy` comes only from a different terminal
  handle or an outside `SignalGuard` holding the signal handlers.
- A signal during a live prompt gives a Bang `Interrupted` error and restores
  the terminal without re-raising the signal. Climax maps it to `Cancelled`
  with `Error::signal()` set, and `climax::main` exits with 128 plus the signal
  number. Other cancellations exit 130.
- Application and I/O errors retain categories and source chains.
- Pound flattened fields preserve declaration order and reject command-level
  name collisions. Screw runtime roots and final widgets have independent
  concrete types.
- The executable/configuration package remains `bang-run`, distinct from the
  library product.

## Remaining WIP

- Continue arbitrary-child PTY transport and emulation work in a future overlay
  crate. `screw-pty` stays an unpublished model of emitted ANSI and is not a
  general terminal-emulation runtime.
- Review incidental public items crate by crate before 1.0 rather than treating
  facade width and component width as one decision.
