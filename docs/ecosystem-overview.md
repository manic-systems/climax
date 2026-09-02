# Ecosystem overview

Last updated: 2026-09-02

The workspace has four intended library products:

| Product | User-facing responsibility |
| --- | --- |
| `pound` | Argument parsing, derives, command specifications, and raw matches |
| `screw` | Terminal surfaces, rendering, styles, widgets, and runtimes |
| `bang` | Typed interactive prompts and advanced custom interaction |
| `climax` | Application lifecycle, policy, output, status, and composition |

These are independent products, not layers that must always be consumed
together. `climax` is the convenient integrated path; direct dependencies are
the supported path to the complete component APIs.

## Workspace roles

The remaining crates have narrower roles:

- `pound-derive`: Pound's procedural macro implementation;
- `screw-pty`: PTY frame and screen integration for Screw (WIP);
- `bang-core`: Bang's renderer-neutral widget, event, value, view, and session
  model;
- `bang-terminal`: terminal input, guards, signals, and session driving;
- `bang-screw`: the adapter and live runtime joining Bang to Screw;
- `bang-screw-pty`: the Bang/Screw PTY overlay adapter (WIP);
- `bang-cli`: the executable and configuration product built on Pound and Bang.

`bang-core`, `bang-terminal`, and `bang-screw` contain public Rust items because
separate workspace crates must communicate. That does not make those items the
friendly standalone Bang interface. They are implementation/adapter contracts,
versioned with their consumers; ordinary users start at `bang`.

## Dependency direction

```text
climax -----> pound
   |
   +--------> screw
   |
   +--------> bang -----> bang-core
                |
                +------> bang-screw -----> screw
                            |   |
                            |   +--------> bang-terminal -----> bang-core
                            +------------> bang-core
```

Climax's Pound and Screw edges are direct optional dependencies enabled by its
`parse` and `render` features. The Bang edge is likewise optional under
`interactive`; Climax does not reach through Bang to name its adapter crates.

The exact internal edges may evolve, but the product rule is stable:

- component products do not depend on `climax`;
- `pound` and `screw` do not acquire Bang or Climax concepts;
- adapters may depend on both sides they join;
- `climax` consumes the coherent `bang` product instead of assembling Bang's
  implementation crates itself.

`bang-cli` is separate from the `bang` library name. It owns executable argument
and configuration plumbing, while prompt behavior and typed results stay in
`bang`.

## Public API policy

Use three categories when deciding visibility:

1. **Product interface:** required to use a crate for its stated purpose and
   documented as a normal, supported path.
2. **Advanced or adapter seam:** deliberately available for custom integration,
   but outside the prelude and happy path.
3. **Incidental implementation:** public only for convenience and a candidate
   for narrowing before stabilization.

For Bang, the typed builders are the product interface:

```rust
let outcome = bang::select("shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let bang::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
```

`bang::advanced` groups custom widgets, actions, raw `Value`, sessions, and
event replay. Renderer-facing structures below that boundary are adapter seams,
not concepts required for typed prompt use.

For Climax, the root and prelude endorse `main`, the non-exiting run variants,
`Context`, `Error`, and `Result`. A normal facade signature should not expose
`bang_core::Value`, `ActionBinding`, `Session`, `screw::Runtime`, or
`pound::Matches`. Advanced users add `bang`, `screw`, or `pound` directly rather
than receiving broad Climax re-exports.

## Derive dependency

Using `#[derive(pound::Parse)]` requires an explicit `pound` dependency with its
`derive` feature. `pound-derive` currently emits paths rooted at `::pound`, so a
transitive Pound dependency through `climax` cannot satisfy generated code.

## Verified build shape

The obsolete feature-failure note has been removed. The following were verified
on 2026-09-02:

- `climax` with no default features;
- parse-only, render-only, and interactive-only `climax`;
- all-feature `climax`;
- standalone `bang`;
- `bang-cli`.

This is a point-in-time development check, not a stability guarantee.

## Implemented migration

- A user-facing `bang` facade now owns typed select, multi-select, search, text,
  and review workflows.
- Prompt-specific `Configurable` implementations cover ordinary presentation
  and initial input state without moving typed choices out of their builders.
- Action-free reviews use `PromptOutcome`; adding the first intrinsic action
  transitions to an action-bearing builder with `ReviewOutcome`.
- Lower-level widget/session operations are grouped under `bang::advanced`.
- `climax` now depends on `bang`, delegates prompt construction to it, and no
  longer owns a duplicate prompt implementation.
- `climax` has a narrow root and prelude and does not re-export the component
  crates.
- Climax errors use facade-owned categories with opaque dependency sources.
- Climax owns help/version, parse diagnostics, application diagnostics, and
  process exit status through its executable entry point.
- Climax Context owns terminal capability, noninteractive behavior, logical
  output channels, and shared status presentation policy.
- Climax finite results commit and flush only after successful handler
  completion; notices, diagnostics, transient status, and streaming output
  have explicit separate channels and semantics.
- Bang interactions are injectable: live, scripted, and advanced custom drivers
  use the same typed prompt builders.
- Multiple status animations share one transient renderer; prompts suspend and
  restore that presentation around exclusive interaction.
- Screw owns physical viewport measurement and allocation; Bang list widgets
  consume renderer-neutral visibility and page-navigation feedback.
- Application and I/O errors retain honest categories and source chains.
- Pound flattened fields preserve declaration order and reject command-level
  name collisions; Screw runtime roots and final widgets have independent
  concrete types.
- The executable/configuration package remains `bang-cli`, distinct from the
  library product.

## Remaining WIP

- Continue strengthening Bang-to-Screw adapter tests as its view vocabulary
  grows.
- Continue arbitrary-child PTY transport and emulation work in `screw-pty` and
  `bang-screw-pty`; the focused emitted-ANSI acceptance model is complete, but
  neither crate is a general terminal-emulation/overlay runtime yet.
- Review incidental public items crate by crate before 1.0 rather than treating
  facade width and component width as one decision.

The current topology gives each user-facing name one coherent meaning. Further
refactoring should deepen these products without making their internal crate
boundaries part of the ordinary user experience.
