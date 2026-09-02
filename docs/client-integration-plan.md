# Client integration plan

Started: 2026-08-26
Last updated: 2026-09-02

This plan combines the `ok-editor` integration report with the pressure from
Autonym. It records decisions and open design questions before implementation;
the report remains evidence from one client rather than the framework roadmap
by itself.

## Current priorities

The first implementation tranche is Screw composition because it blocks
`ok-editor`'s intended permanent action pane and supplies the compositor already
anticipated by `bang-screw-pty`.

1. Establish one physical viewport model and one protected-last-column rule.
2. Add display-cell geometry and low-level `Surface` composition.
3. Add generic floating layers using composable edge constraints.
4. Exercise open, change, shrink, move, removal, and resize through the retained
   renderer.
5. Move `ok-editor` from allocated action rows to a floating pane and make its
   displayed bindings derive from the real keymap.

The terminal tranche followed composition. It now includes the configurable
application screen guard, handle-based terminal setup, decoder completion, a
reusable fd-polled event source consumed by the Bang driver, Screw runtime
viewport parity, and a Climax terminal-application lease.

Screw's application-oriented documentation is deliberately deferred until the
composition and terminal APIs settle. A PTY application harness also follows a
reuse decision about the existing Beer emulator work rather than beginning as a
new emulator immediately.

The display-width-aware Screw table remains a separate CLI-driven tranche. It
may share measurement and geometry primitives with composition, but it does not
depend on floating layers and must not change Climax's structured result shape.

## Physical viewport and geometry

Screw should convert raw terminal dimensions into a usable physical viewport at
one boundary. `Viewport::columns` is the drawable display-cell width after the
protected final terminal column is accounted for. Wrapping, clipping, base
layout, local child constraints, and floating placement all consume this value;
nested children must not apply the reservation again.

The geometry vocabulary is:

```rust
pub struct Size {
    pub width: usize,
    pub height: usize,
}

pub struct Rect {
    pub origin: Position,
    pub size: Size,
}

pub struct Insets {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

pub struct Viewport {
    pub columns: usize,
    pub rows: usize,
}
```

Rectangle arithmetic is saturating. Empty rectangles are valid no-ops.

## Surface composition

Composition operates on physical display cells after child wrapping and
clipping. The initial operation accepts a source surface, a destination
rectangle, a canvas/clip rectangle, missing-cell fill policy, and explicit
cursor policy. It writes no ANSI and leaves the retained renderer responsible
for terminal output and stale-cell restoration.

Required behavior includes:

- transparent gaps preserve the base while explicit spaces overwrite it;
- opaque fill materializes styled spaces across the destination rectangle;
- wide cells and their combining marks remain atomic;
- a wide source cell is copied whole or dropped;
- intersecting any part of a wide base cell removes the whole cell without
  shifting later cells;
- style is copied cell-for-cell with no colour blending;
- insertion order is z-order; and
- base, overlay-preferred, and hidden cursor policies are explicit.

The rectangle's *composition footprint* is the requested intersection expanded
to contain any base wide cell it intersects. Expansion is allowed only when the
whole base cell remains inside the composition canvas. If a base wide cell
straddles the canvas boundary, a conflicting overlay write or fill cell is
dropped and the base cell remains intact. This is the only result which both
keeps wide cells atomic and leaves every column outside the canvas unchanged.
Uncovered columns from a removed wide base cell retain its style as one-column
spaces unless the overlay or fill replaces them.

This footprint is local `Surface::overlay` correctness vocabulary, not retained
renderer damage tracking or a dirty-rectangle API. It becomes necessary because
a floating child can begin or end over one half of an existing two-column base
cell. The compositor must rewrite that whole base cell into valid column
occupants before the existing renderer compares the completed surface with the
previous frame in its normal way.

Property-style coverage should initially use deterministic exhaustive fixtures
and a small local generator. A new property-testing dependency is not assumed.

## Floating placement by edge constraints

Floating placement uses OR-able edge flags rather than a closed enum of named
anchors:

```rust
Floating::new(Edge::TOP | Edge::RIGHT)
Floating::new(Edge::TOP)
Floating::new(Edge::TOP | Edge::LEFT | Edge::RIGHT)
```

An edge on only one side of an axis anchors the measured child to that side.
Neither edge centres the measured child on that axis. Opposing edges constrain
the child to the full available span on that axis.

This gives the intended distinctions:

| Edges | Placement |
| --- | --- |
| none | centred at measured size |
| `TOP` | top-centred |
| `TOP | LEFT` | top-left at measured size |
| `TOP | RIGHT` | top-right at measured size |
| `LEFT | RIGHT` | full-width, vertically centred region |
| `TOP | BOTTOM` | full-height, horizontally centred region |
| `TOP | LEFT | RIGHT` | full-width top region |
| `LEFT | TOP | BOTTOM` | full-height left region |
| all four | fill the inset viewport |

Margins inset the parent viewport before resolving edges. A single-edge or
edge-free axis uses the child's measured size, limited by its constraints. An
opposed axis supplies the whole inset span as the child's local constraint and
destination span.

Decision: opposing edges win on their axis. An axis with neither edge remains
centred, including when the other axis is stretched. Thus `LEFT | RIGHT` is a
full-width box centred vertically rather than an underspecified horizontal
anchor.

`Edge` can be a small in-crate bitmask with `BitOr`; no bitflag dependency is
needed. The type name should communicate constraint semantics rather than imply
that every combination is merely a point anchor.

Floating children do not participate in `Stack` height allocation and do not
alter base measurement, wrapping, scroll position, cursor ownership, or
`vertical_size`. Later insertions render above earlier ones. Numeric z-indices,
focus, mouse routing, caching, and dirty rectangles are out of the first
tranche.

## `ok-editor` integration

The editor will render the document and status as the base and its existing
contextual action model as an opaque bottom-right child with a one-row bottom
margin. The pane is omitted at unusable sizes and in full-frame help. Changing
or hiding it must not change document width, height allocation, scroll position,
or caret position.

The current action discovery already proves speculative legality filtering.
The remaining client-local correction is to expose stable ordered binding
metadata from `ok-input`, including display labels, so dispatch and presentation
cannot drift. A universal WhichKey model does not belong in Screw. A Bang action
adapter may be added in `bang-screw` only when a Bang consumer needs it.

## Reusable terminal event pump

This item was implemented after the immediate composition details were
resolved.

The reusable portion of the current Bang terminal runner includes byte reads,
decoder buffering, multi-event queues, resize detection, signal polling,
interrupted reads, and standalone-Escape timing. The reusable layer must not
own Bang `Session` state or impose Ctrl-C, Ctrl-D, cancellation, save, or quit
policy.

The TTY backend requires an input implementing `AsFd`, puts it
in a blocking raw configuration (`VMIN = 1`, `VTIME = 0`), and waits with
`poll(2)`:

- readable means call `read` and decode bytes;
- hangup means return end-of-input;
- interruption means check the installed signal source;
- timeout means check the Escape deadline, terminal size, and signals; and
- error readiness becomes an I/O error.

The poll timeout is the nearest pending deadline, rather than a fixed 100 ms
wake-up. With no lone Escape it may use a modest resize/signal check interval,
unless those sources later gain wakeable file descriptors. A lone Escape makes
its configured ambiguity deadline the nearer timeout; a second byte arriving
first makes the descriptor readable and continues Alt/CSI decoding.

The resolved and implemented public shape is a blocking pull API:

- `TerminalEvents::blocking(Read + Send + 'static)` moves a generic blocking
  reader to a worker and receives byte chunks through a channel. This permits
  the pull side to wake at the Escape deadline even while the reader remains
  open and blocked; zero still means stream EOF;
- `TerminalEvents::tty(Read + AsFd)` polls a descriptor intended to be placed
  in blocking raw mode;
- `next_event()` returns one queued `TerminalPoll` at a time;
- decoded input and resize are `TerminalPoll::Event(Event)`;
- signals remain `TerminalPoll::Signal`, outside application input policy;
- the decoder exposes only a lone ambiguous Escape for deadline flushing, and
  the event source owns the configurable deadline and clock;
- readers, signal sources, size sources, and clocks are injectable; and
- the existing Bang session runner consumes this source and retains only Bang
  submission/cancellation policy.

The generic-reader worker cannot synchronously cancel an arbitrary blocked
`Read`. Dropping `TerminalEvents` closes its receiver, and the worker exits
after that read next returns. Code which needs prompt teardown should use the
pollable TTY backend or pass a reader with its own cancellation mechanism.

Complete CSI/SS3 sequences outside the supported key vocabulary produce
`Event::UnknownEscape(bytes)`. They never masquerade as `Key::Esc`, and the
terminal layer never writes them back verbatim because doing so could execute
terminal controls. A client may ignore them, show a quoted representation, or
bind them through its own policy.

The terminal implementation is explicitly Unix-only. Its public handle
contract uses `AsFd`, termios, `poll(2)`, signals, and `ioctl`; there is no
conditional non-Unix compatibility facade.

## Terminal lifecycle follow-up

The terminal tranche was implemented in this dependency order:

1. Handle-based raw mode and terminal measurement, with process convenience
   wrappers.
2. A configurable screen guard covering inline/alternate screen, cursor policy,
   and bracketed paste, with partial-entry rollback and exhaustive cleanup.
3. Decoder support for Alt characters, modified named keys, xterm CSI modifier
   parameters, arbitrary chunk boundaries, and explicit Escape ambiguity.
4. The event pump after its design questions are resolved.
5. A Climax terminal-application scope using the same exclusivity coordinator
   as prompts and statuses.

Raw-mode guards restore through an owned duplicate descriptor, so acquiring raw
mode no longer borrows the input handle for the guard's lifetime. Context keeps
terminal-presentation exclusivity separate from handles:
`with_terminal_application` supplies process stdin and the configured transient
writer, while `with_terminal_application_on` accepts caller-supplied input and
output. This permits raw mode, the event source, a screen guard, and a renderer
to be nested without self-referential ownership or erased access to `AsFd`.

`SignalGuard` is a process singleton because signal dispositions and the
self-pipe write endpoint are process-global. Installation uses an atomic
reservation, all pipe descriptors are owned from creation, and early setup
failures close them automatically. Screen entry conservatively assumes a mode
may have changed after a partial `write_all`, cancels an incomplete control
sequence, and attempts every inverse. Explicit live-session teardown collects
renderer, screen, signal, and raw-mode failures while retaining a primary run
failure. Climax status scopes likewise retain cleanup failures as related
errors.

## Widget locality and threaded runtimes

Screw's former base `Widget: Send + Sync` bound was not justified by
synchronous rendering. The renderer is a UI-thread consumer; allowing `Rc`,
`RefCell`, borrowed widgets, and other immediate local bindings follows normal
UI ownership. Threaded consumers impose their boundary where data actually
crosses to the rendering thread.

The implemented split is:

- `Widget` has no `Send` or `Sync` supertraits;
- `LocalWidgetRef<'a>` and `local_widget` use `Rc<dyn Widget + 'a>` for local
  composition;
- `SharedWidgetRef` uses `Arc<dyn Widget + Send + Sync>`, while the historical
  `WidgetRef` and `widget` names remain aliases for this compatible shared
  path;
- `Stack`, `Line`, `Stateful`, `VerticalViewport`, `LayoutBuilder`, templates,
  and floating layers are generic over their erased child handle, so both
  ownership paths share one implementation; and
- `Runtime` is generic over its root. Synchronous drawing has no thread bound;
  `start` and automatic live selection require the writer and root to be
  `Send + 'static` because they are moved to the background renderer.

An exclusively owned live root need not be `Sync`; a concrete
`Widget + Send + 'static` is sufficient. Cloneable shared trait objects still
require both `Send` and `Sync` because their `Arc` may be retained and observed
from multiple threads. Compile and runtime fixtures cover a local
`Rc<RefCell<_>>` tree, a send-only owned live root, and the existing shared
Climax status path.

## Adapter and introspection contracts

Bang adapters are statically linked Rust crates, so dynamic protocol version
negotiation would add machinery without solving a real compatibility problem.
`View` is exhaustive instead: adding a semantic view is a semver-breaking
change which makes adapters fail to compile until they handle it. Semantic
`Role` may remain non-exhaustive because mapping a new role to normal styling is
a valid degradation; dropping an entire view is not.

Pound's command-level introspection now traverses recursively flattened
arguments through `CommandSpec::arguments`; `find_long` and `find_short` use the
same complete view. Help and the introspection example consume that public
iterator, so manpage and completion authors no longer accidentally omit
flattened options.

## PTY scope and existing emulator work

"PTY harness" contains two independent facilities which should not be bundled
by default:

1. a pseudo-terminal transport which gives a child real terminal file
   descriptors, sends input, reads output, changes window size, delivers
   signals, and reports exit status; and
2. a terminal screen model which interprets the child's output bytes as cells,
   cursor state, and negotiated modes.

Most planned coverage needs neither facility:

- surface composition is a pure `Surface` test;
- retained rendering can capture writer bytes directly;
- screen guards can assert exact enter/leave bytes and injected write failures;
- decoder tests feed arbitrary byte chunks directly; and
- the event pump can use fake readers, clocks, size sources, and signal sources.

A small terminal screen model is useful for retained-renderer acceptance tests.
It only needs the alphabet Climax currently emits:

- printable UTF-8, carriage return, and line feed;
- relative cursor movement: `CSI n A/B/C/D`;
- erase to end of line and whole line: `CSI K` and `CSI 2 K`;
- the current SGR subset: reset, bold, dim, reverse, and basic foreground and
  background colours;
- cursor visibility: `DECSET`/`DECRST ?25`; and
- for application lifecycle tests, alternate screen `?1049` and bracketed
  paste `?2004` mode state.

It does not need scroll regions, horizontal margins, scrollback, resize reflow,
OSC/DCS/APC, mouse protocols, Kitty protocols, images, hyperlinks, truecolour,
font shaping, or a window system. This subset is small enough for a focused
in-repository interpreter; using `vte` remains an option if manual escape
chunking becomes the larger or less reliable implementation.

Only the final lifecycle acceptance layer needs a real PTY pair. That transport
needs to open master/slave handles, attach child stdin/stdout/stderr to the
slave, set and change `TIOCSWINSZ`, exchange raw bytes through the master,
inspect terminal modes before and after execution, deliver signals, and wait
for the child. It does not itself emulate a terminal.

The future `bang-screw-pty` goal is different: displaying the screen of an
arbitrary child application under a Screw/Bang overlay does require a broad VT
emulator. Beer becomes directly relevant to that product, but should not set the
scope of the nearer Climax application test harness.

There is no extra Climax worktree and no PTY/emulator branch in the Climax Git
repository. `screw-pty::EmittedScreen` is the strict focused output model;
the old lossy `PtyScreen` collector has been removed. `PtyFrame`/`PtyWidget`
remain only as a bridge for callers which already own decoded lines.
`bang-screw-pty` remains configuration vocabulary rather than a runtime.

The relevant existing emulator is `/home/bolt/code/beer`:

- it is a full Wayland terminal emulator using `vte`;
- it owns a real PTY implementation and resize propagation;
- its `Term` and `Grid` model cursor movement, erase/insert operations, scrolling,
  margins, wide and combining cells, SGR, alternate screen, cursor visibility,
  bracketed paste, and substantially more than Screw emits;
- `vt/conformance.rs` already contains headless golden tests which feed escape
  bytes and assert grid state; and
- a local `local` branch contains an older line of unpushed PTY/input/rendering
  work, but it is a branch, not an attached worktree.

Beer is not immediately reusable as a Climax dev dependency. Its `grid` and
`vt` modules are private modules of the `beer` binary crate, and the terminal
model currently reaches into Beer graphics, theme, protocol, and application
types. The binary crate also carries the full Wayland/font/image dependency
set. Running the Beer application in tests would add irrelevant window-system
requirements.

For a future arbitrary-child overlay, the emulator decision is between:

1. extracting Beer's headless terminal model into a small reusable crate, then
   adapting its cells and modes for Screw tests;
2. reusing selected model code inside `screw-pty`, accepting maintenance and
   divergence costs; or
3. implementing only the ANSI subset Screw emits, possibly on `vte`, while
   using Beer's conformance tests and behavior as the reference.

The near-term harness should not extract or import all of Beer. Beer is a useful
behavioral oracle, and selected tests may inform the small emitted-ANSI model.
If arbitrary-child overlays resume, prefer extraction or deliberate reuse of
the Beer model over independently growing another general terminal emulator.
That later decision must inspect the coupling cut and decide which repository
owns the reusable headless crate. No PTY work starts as part of the
floating-layer tranche.

## Deferred documentation

Application-oriented Screw documentation is deliberately later. Once APIs have
settled it should cover physical versus logical layout, full viewports, Clip
versus Wrap, retained resize behavior, cursors, floating composition, and the
division between `Renderer`, runtimes, and terminal lifecycle guards.

## Implementation checkpoint

The composition tranche was implemented on 2026-08-26 after settling:

- opposing edges stretch and override a maximum on their axis;
- wide-cell damage uses the local atomic composition footprint described above;
- the event pump will use the fd-polling shape described above; and
- Beer remains a behavioral reference for the near-term focused harness.

Implemented in Screw:

- one renderer-owned usable viewport calculation and constrained `RenderCtx`;
- saturating display-cell geometry;
- transparent/opaque `Surface` composition with explicit cursor policy;
- OR-able edge constraints and generic floating `Layers`;
- deterministic Unicode, clipping, z-order, placement, retained shrink/removal,
  resize, and cursor tests; and
- a public floating-layers example.

The already-started `ok-editor` integration was completed and validated against
the workspace: its action pane now floats over the document, and ordered labels
for dispatched core bindings come from `ok-input::Keymap` metadata. Future
cross-repository requirements discovered here are to be recorded in Markdown
handoffs for the owning repository rather than implemented directly from this
checkout.

The terminal lifecycle/event-pump tranche is also implemented: handle-based raw
mode and size measurement, configurable screen negotiation, decoder modifier
and chunk-boundary coverage, deadline-conformant stream and fd-polled terminal
events, runtime viewport/cursor parity, configurable Climax terminal handles,
and the exclusive terminal-application lease are present. Live-session and
status teardown retain cleanup failures alongside the primary failure.

The display-width-aware table remains subsequent work. The PTY/emulator and
application-oriented Screw documentation items retain the scope and deferral
decisions above.

## Hardening checkpoint

The immediately actionable follow-up tranche was implemented on 2026-08-27:

- the shared Bang session driver has deterministic tests for scripted input,
  resize, renderer feedback, EOF, cancellation, signals, submission, and
  renderer failure;
- isolated Cargo fixtures automatically compile the Pound-only, Screw-only,
  Bang-only, Climax-only, and Climax-plus-components dependency stories;
- Screw renderer/runtime regressions cover floating movement and restoration,
  conservative full redraw after resize, cursor visibility across height
  changes, and matching
  viewport constraints through synchronous, live, and plain runtimes; and
- `screw-pty::EmittedScreen` provides the agreed focused output interpreter for
  headless acceptance tests, including strict rejection of escape sequences
outside the emitted alphabet.

The 2026-08-31 review additionally corrected cursorless full-height frame
scrolling, canvas-bounded wide-cell composition, end-of-child overlay cursors,
soft-row-boundary reflow, final-column combining marks in `EmittedScreen`,
signal singleton/fd lifecycle, inline bracketed paste negotiation, and removal
of the obsolete lossy PTY screen type.

This does not begin arbitrary-child emulation or PTY process transport. Those
remain the broader Beer/reuse decision described above.
