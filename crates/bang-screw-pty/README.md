# bang-screw-pty

`bang-screw-pty` is the PTY overlay integration crate for `bang` and `screw`.

it is intentionally separate from `climax`. `climax` may expose this crate as an
optional component, but this crate owns the integration loop.

## boundary plan

core crates define nouns, adapter crates translate nouns, facade crates package
workflows.

- `pound` owns argv to typed command parsing.
- `bang-core` owns semantic events, widget/session state, logical views, and
  submitted values.
- `bang-terminal` owns terminal mode, input decoding, session driving, screens,
  signals, and size handling.
- `screw` owns widgets, surfaces, retained diff rendering, styles, and runtime.
- `bang-screw` owns `bang_core::View` to `screw::Surface` rendering.
- `screw-pty` owns PTY screen state as `screw` surfaces or widgets.
- `bang-screw-pty` owns child process output, `bang` overlay input routing, and
  composition of PTY and overlay surfaces.
- `climax` owns convenient opt-in facades over these crates.

## dependency direction

allowed:

- `bang-terminal -> bang-core`
- `bang-screw -> bang-core + bang-terminal + screw`
- `screw-pty -> screw`
- `bang-screw-pty -> bang-core + bang-terminal + bang-screw + screw + screw-pty`
- `climax -> optional deps on public components`

forbidden:

- `bang-core -> screw`
- `bang-core -> bang-terminal`
- `bang-core -> PTY crates`
- `screw -> bang-*`
- `screw -> climax`
- `bang-terminal -> screw`
- `pound -> bang, screw, or climax`

## overlay responsibilities

the overlay runtime must make input ownership explicit:

- passthrough mode sends keys to the child PTY.
- overlay mode sends keys to the `bang` session.
- command mode is reserved for runtime bindings such as opening or closing the
  overlay.

rendering should compose surfaces:

```text
child PTY screen surface
+ optional bang overlay surface
= composed screw surface
-> screw renderer
```

if composition needs generic primitives such as rectangles, clipping,
translation, or transparent cells, add those to `screw` without mentioning
`bang` or PTYs.

## climax integration

`climax` should expose this as an optional component only:

```toml
pty-overlay = ["parse", "interactive", "dep:bang-screw-pty"]
```

that lets users choose `pound + bang`, `pound + screw`, `bang` with a custom
renderer, `screw` without `bang`, or the complete `climax` facade.
