# `ok-editor` terminal integration handoff

Date: 2026-08-26

This is a coordination note for the owning `ok-editor` repository. No further
client changes should be made from the Climax checkout.

The current `ok-editor` floating action-pane integration has been validated
against the Climax workspace. Its local terminal entry point can now choose to
migrate to these newer component APIs:

- `bang_terminal::TerminalModeGuard::activate(handle, RawModeOptions::blocking())`
  targets the actual input handle and pairs with fd polling;
- `bang_terminal::ScreenGuard::enter(writer, options)` supports inline or DEC
  1049 alternate screen, cursor policy, bracketed paste, partial-entry rollback,
  and exhaustive restoration attempts;
- `bang_terminal::TerminalEvents::tty(reader)` provides a blocking pull source
  with a decoder queue, configurable Escape deadline, resize source, and signal
  sideband outcomes;
- Alt+Unicode, Alt controls, SS3 navigation keys, xterm `1;2` through `1;8`
  modifiers, modified navigation keys, and arbitrarily chunked bracketed paste
  are decoded;
- Screw synchronous/live/auto/plain runtimes now expose viewport dimensions,
  two-dimensional resize, and cursor-visibility policy; and
- `climax::Context::with_terminal_application` leases the configured transient
  writer and process input while suspending statuses and excluding prompts.

Application policy remains with the editor: save, quit, Ctrl-C handling, signal
reraising, screen choice, and how primary versus cleanup errors are presented.
Function-key decoding also remains deliberately open because `bang_core::Key`
does not yet model function keys; expanding that public enum should be
coordinated with exhaustive downstream matches first.
The client should migrate only when useful and should report any semantic gaps
back through a Markdown handoff rather than relying on the earlier report as an
unchanging specification.
