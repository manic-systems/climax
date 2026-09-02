# Climax facade

Last updated: 2026-09-02

This is the current facade policy. The API remains pre-1.0 and may change.

## Promise

`climax` is the application product. It composes parsing, rendering, typed
interaction, output routing, and application policy without making users learn
the adapter crates.

The ordinary root surface is deliberately small:

```text
climax::{main, try_run, try_run_from, run_with, Context, Error, Result}
```

`climax::prelude` contains only those ordinary application imports. Feature
specific facade modules such as `output` and `status` remain explicitly named;
component and adapter internals do not belong in the prelude.

## Typed prompts

`Context` delegates prompt construction to the public `bang` product:

```rust
let outcome = cx
    .select("shell")
    .choice("bash", Shell::Bash)
    .choice("zsh", Shell::Zsh)
    .interact()?;
let bang::PromptOutcome::Submit(shell) = outcome else {
    return Ok(());
};
```

The submitted value is the user's `Shell`, not `bang_core::Value`; leaving is a
typed `PromptOutcome::Leave`. Select, multi-select, search, and text prompts
follow the same path. Each builder implements `bang::Configurable` with an
associated prompt-specific config: list presentation cannot accidentally be
applied to a text prompt, while choices and their typed values stay on the
builder.

An action-free review follows the ordinary shape and returns
`PromptOutcome<Vec<bang::Reviewed<T>>>`. Calling its first intrinsic `.action`
transitions to `bang::ReviewPromptWithActions<T, A>`; further actions use the
same `A`, and interaction returns `bang::ReviewOutcome<T, A>`. Arbitrary action
layers remain an advanced widget facility. Prompt implementation and
live-session orchestration belong to `bang`; `climax` supplies application
policy and maps errors at the facade boundary.

Bang prompt builders carry an opaque, cloneable `bang::Interaction`. The
default driver uses stdin/stderr; deterministic and custom drivers are available
through `bang::advanced`. Climax injects its configured driver into every prompt
created by `Context`.

## Terminal and stream policy

`Context` snapshots process terminal capabilities and owns three logical output
channels:

- durable application output, stdout by default;
- diagnostics, stderr by default;
- transient prompts and status presentation, stderr by default.

Automatic prompt interaction requires terminal-capable stdin, a suitable
transient terminal, and ANSI presentation support. `InteractionMode` can force
or disable interaction. Unavailable prompts return an explicit error; Climax
never invents a typed fallback value.

`StatusMode::Auto` animates only on a suitable transient terminal and is silent
otherwise. Plain, live, and silent modes are explicit overrides; silent mode
never emits status or final-message text. Status handles
register widgets with one coordinator and one renderer, allowing multiple
animations to coexist without competing cursor writes. An interactive prompt
temporarily suspends status presentation; a simultaneous second prompt is an
interaction-busy error.

Scoped work uses:

```rust
let value = cx
    .status("scanning history")
    .spinner()
    .during(|| scan())?;
```

Status cleanup occurs on success, error, and unwinding. The operation error
takes precedence over a cleanup error. Cleanup continues after an earlier
failure and retains subsequent failures as related errors. Context builders
which can trigger that cleanup return `Result<Context>`:

```rust
let cx = Context::new()
    .with_terminal_capabilities(capabilities)?
    .with_status_mode(climax::terminal::StatusMode::Silent)?;
```

## Application lifecycle

The normal executable entry point lets Climax own parse signals, diagnostics,
and process status:

```rust
fn main() -> std::process::ExitCode {
    climax::main::<Cli, _>(run)
}
```

Help and version go to stdout with status 0, parse failures go to stderr with
status 2, and application failures go to stderr with status 1. `try_run` and
`try_run_from` are the non-reporting paths for embedding and tests. `run_with`
starts from an already constructed command.

## Results and sideband output

Durable stdout contains application results. Human context, diagnostics, and
transient status are separate concerns:

- `result` registers the invocation's one canonical finite result;
- `notice` writes human-only context to stderr in text mode and is suppressed
  in JSON mode;
- application errors remain diagnostics on stderr;
- status and prompts use the transient presentation channel;
- `stream` writes zero or more values immediately and uses JSON Lines in JSON
  mode.

A finite result has one semantic value and two projections:

```rust
cx.output()
    .result(&scan_result)
    .text(|result| scan_view(result))
    .emit()?;
```

The `structured` feature uses `serde::Serialize` for the JSON projection. The
text closure is only evaluated in text mode. Climax encodes and holds the
selected projection, then writes and flushes it only after the application
handler returns success. A later handler error discards it. All cloned `Context` and `Output`
handles share the result slot, so a second finite result or mixing finite and
streaming output is an output-policy error.

Streams deliberately have weaker atomicity. Values may already have reached
stdout when later application work fails. This distinction is explicit rather
than making every command inherit streaming semantics.

## Boundary rule

A lower-level type may appear in a `climax` public signature only when it is an
unavoidable part of the ordinary application workflow and has no facade-owned
equivalent. Advanced widget, session, rendering, and parser APIs fail that test.

The supported escape hatch is an explicit dependency:

```toml
[dependencies]
bang = "..."   # typed prompts and bang::advanced
pound = "..."  # parser and derive API
screw = "..."  # rendering API
```

`climax` does not re-export these crates. This keeps their compatibility
contracts independent and makes advanced use visible in an application's
manifest.

## Parse derive detail

`climax::main` and `try_run` accept a `pound::Parse` command when the `parse`
feature is enabled. An application using `#[derive(pound::Parse)]` must still depend on
`pound` directly with its `derive` feature: the macro's generated paths target
`::pound`. Re-exporting the trait from `climax` would not remove that requirement.

## Ownership

- `pound` owns parsing, derives, specifications, and raw matches.
- `screw` owns surfaces, rendering, styles, widgets, and renderer runtimes.
- `bang` owns typed prompt builders, typed results, interaction sessions, and
  the public advanced escape hatch.
- `climax` owns application lifecycle, policy, output routing, status access,
  and high-level error mapping.
- Adapter crates own cross-product plumbing and are not ordinary user APIs.

`climax::Error` exposes facade-level categories and retains dependency errors as
opaque sources. General I/O, output failures, unavailable/busy interaction, and
application-owned source errors remain distinguishable. `Error::application`
and `Error::application_context` retain source chains without making dependency
error enums part of the facade contract. Uncaught cancellation exits silently
with status 130.

Pound preserves source declaration order across direct and flattened fields
for positional parsing, help, and introspection, and rejects ambiguous long
names, aliases, or short names within a flattened command level. Screw runtime
root and configured-final widgets have independent types; only widgets sent to
an already running renderer thread are type-erased.

## Features

- `parse`: lifecycle entry points and Pound integration;
- `render`: status rendering and Screw integration;
- `interactive`: typed Bang prompts;
- `structured`: Serde-backed application results and JSON/JSON Lines output;
- defaults: all four.

Verified on 2026-09-02: minimal, parse-only, render-only, interactive-only, and
all-feature `climax` configurations compile.

## Remaining WIP

Human table presentation remains separate from the result contract. Screw
still needs display-width-aware column measurement, alignment, multiline cell
layout, and an explicit constrained-width overflow policy. Climax can then map
a result's text projection to that renderer without changing its JSON shape.
