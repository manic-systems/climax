# Changelog

All notable changes to the climax family (`climax`, `climax-derive`, `screw`, `bang`, `bang-core`, `bang-terminal` and `bang-run`) are documented here. The family releases together at one version and the format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The argument parser has its own changelog in `crates/pound/CHANGELOG.md`.

## [0.1.0] - Unreleased

The first release of the facade. The crates published at 0.0.1 were an early cut, and this release reshapes most of their public API. The Breaking section groups what moved by crate so an upgrade can work through one crate at a time.

### Added

#### climax

- `climax` is an application facade. An application needs it as its only dependency, since each component is re-exported whole behind the feature that enables it, as `climax::pound` under `parse`, `climax::bang` under `interactive`, `climax::screw` under `render` and `climax::serde` under `structured`. Every workspace dependency is pinned to an exact version, so one `climax` release names one tested set of component versions.
- `climax::main` runs a `pound::Parse` command and returns a `std::process::ExitCode`. Help and version go to stdout with status 0, parse failures go to stderr with status 2 and application failures go to stderr with status 1. A cancellation exits 130, or 128 plus the signal number when a signal interrupted a live prompt. `try_run` and `try_run_from` skip the reporting for embedding and tests, and `run_with` starts from a command you already built.
- Typed prompts on `Context`, namely `select`, `multi_select`, `search`, `review`, `text`, `password`, `confirm`, `date` and `number::<T>`. Each returns `PromptOutcome::Submit` with the user's own type or `PromptOutcome::Leave` when cancelled. The prompt config types, `Configurable`, `ReviewOutcome`, `ReviewExit`, `ReviewState` and `Reviewed` are re-exported at the crate root and in the prelude.
- Structured output through `Context::output`. `result(&value).text(...).emit()` registers one finite result, holds it until the handler succeeds, prints the text projection for people and serialises the value with serde in JSON mode. `stream` writes values as they arrive, using JSON Lines in JSON mode. `notice` writes human-only context to the transient channel and is suppressed in JSON mode. `Context::diagnostic` gives a second handle that defaults to stderr. These need the `structured` feature for serialisation.
- A status coordinator behind `Context::status`. Every status shares one renderer, so several animations compose, and prompts suspend them while holding the terminal. `Status::during` runs scoped work and prints `final_message` on success and `failure_message` on error, unwind or an early drop. `Status::widget` draws a `screw::WidgetRef` in the live region. `StatusMode` selects auto, plain, live or silent presentation. `Auto` animates on a terminal and prints one plain line per finished status otherwise, and `Silent` stays an explicit choice for a `--quiet` flag.
- Terminal policy through `Context`. `TerminalCapabilities`, `InteractionMode` and `StatusMode` decide whether prompts and animation run. `with_terminal` moves prompts and status to one read-write handle such as `/dev/tty`, `with_terminal_application` hands out the terminal for a full-screen program, and `with_output_writer`, `with_diagnostic_writer` and `with_transient_writer` redirect each channel. Setters that may need to tear down live presentation return `Result`.
- `climax::Error` reports a stable `ErrorKind` through `kind()`, keeps dependency errors as opaque sources, carries the interrupting signal in `signal()` and attaches cleanup failures as `related_errors()`. `Error::application` and `Error::application_context` wrap your own errors.
- `climax-derive` provides `#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` through the default `derive` feature, with expansion rooted at `::climax::pound`. Together with `structured` it provides `#[climax::serde(Serialize, Deserialize)]`, an attribute that derives serde's traits rooted at `::climax::serde`, adds `#[serde(crate = "::climax::serde")]` and keeps the item and its other attributes. It takes plain derive names, rejects an empty list and must come before other derives that share serde's helper attributes. Without `derive`, `#[climax::serde(..)]` does not exist and fails with `expected attribute, found module`, so serde derives reached through climax need `#[serde(crate = "climax::serde")]`.
- `climax::main` exits 141, as pound's `Error::exit` does, when writing help or version output fails because stdout is a closed pipe. Other write failures exit 1.
- `climax::main_with` runs a handler that takes no arguments and reports its result with the same output and exit codes as `climax::main`.
- `Error::cancelled()` builds a cancellation that `main` and `main_with` report like a cancelled prompt, silently with exit 130. `Error::with_exit_code(code)` and `Error::exit_code()` choose the exit code for an error of any kind, and a code of 0 is stored as 1 so an error never exits successfully. The full exit-code mapping is listed in the crate docs and the README.
- `climax::ResultExt`, imported by name and kept out of the prelude so it cannot collide with `anyhow::Context`, adds `.context(message)` and `.app_err()` to any `Result` whose error is a `Send + Sync` `std::error::Error`, turning it into an application error that keeps the original as its source. `Box<dyn std::error::Error + Send + Sync>` converts into `Error` with `?`.
- An optional `anyhow` feature, off by default, adds `impl From<anyhow::Error> for climax::Error`. The anyhow error and its context chain are kept as the source, so `?` works on anyhow results inside a climax handler.
- `climax::testing` runs an application in-process with the reporting and exit codes of `main`. `testing::run(args, script, handler)` and `testing::run_with(script, handler)` return an `Outcome` with `exit_code`, `stdout`, `stderr` and the error. `Script` answers prompts one method per prompt (`select_nth`, `multi_select_nth`, `text`, `text_attempts`, `date`, `confirm`, `enter`, `esc`, `keys`) and fails the run when input is left over. `Capture` is a cloneable `Write + Send` buffer for tests that build their own `Context`.
- `Context::with_prompt_summaries` and `set_prompt_summaries` choose whether prompts leave a summary line. The choice lives on the context, so it applies after `with_terminal`, a capability change or a mode change rebuilds the interaction driver.
- A `structured` feature for serde-backed results and JSON output. All five features (`derive`, `parse`, `render`, `interactive` and `structured`) are on by default and work alone.

#### bang

- `bang` is the typed prompt library, with free functions `select`, `multi_select`, `search`, `review`, `text`, `password`, `confirm`, `date` and `number`. Each builder runs with `interact()` and returns `PromptOutcome<T>`. Prompts implement `Configurable` with a config type per prompt, and the same settings are also direct builder methods. List prompts reject an empty choice list with `ErrorKind::InvalidConfiguration`.
- `PromptOutcome` has `or_cancel`, which turns leaving into an `ErrorKind::Cancelled` error so `.interact()?.or_cancel()?` ends a handler with exit 130, `into_option`, `unwrap_or`, `is_submit` and `is_leave`.
- A submitted prompt leaves a dimmed one-line summary in the scrollback, such as `Deploy to prod? › no`, `Shell › bash`, a comma list for multi-select, a fixed mask for a password and a confirmed count for a review. Leaving writes nothing, and `NO_COLOR` drops the dimming. Every prompt and prompt config takes `summary(bool)`, and `Interaction::with_summaries` sets the default for a driver. Only live terminal drivers write summaries, and only to a terminal.
- `confirm` answers at once on `y`, `Y`, `n` or `N`.
- A review prompt returns `PromptOutcome<Vec<Reviewed<T>>>`. Adding its first `action` turns it into `ReviewPromptWithActions<T, A>`, which returns `ReviewOutcome<T, A>`.
- `bang::Error` with an `ErrorKind` of `Cancelled`, `InputEnded`, `InteractionBusy`, `InteractionUnavailable`, `InvalidConfiguration`, `Interrupted`, `Terminal` or `UnexpectedValue`. A signal during a prompt restores the terminal and returns `Interrupted` with the signal in `Error::signal()`. Bang never re-raises it, so the host keeps running, and a signal the host had ignored stays ignored.
- `bang::advanced` holds custom widgets, sessions, action bindings, `Interaction` drivers, `scripted_interaction` for deterministic tests and `interact_widget` for running your own widget. `bang::terminal` re-exports `bang-terminal`, and `bang::screw` re-exports screw whole.
- `bang-core` gains `Reaction::Action`, which exits past enclosing containers to the session boundary, `Event::UnknownEscape`, `Key::Function`, `SessionReaction`, `TextInput::with_mask`, `Date::from_unix_days` and `Date::unix_days`, and `ReviewList::with_exit_output` and `with_leave_output`.

#### screw

- Text is laid out per extended grapheme cluster using the width the `unicode-width` crate reports for the whole cluster, so emoji sequences, flags and combining marks measure as one cell. A tab expands to the next multiple of eight columns. `width`, `truncate`, `pad` and `Align` measure text the way `Surface::write` lays it out.
- `Table` aligns columns by display width, with `header`, `aligns`, `gap`, `flexible` and `overflow` (`CellOverflow`). A table wider than its space shrinks its flexible columns and truncates or wraps their cells, and cells may span several lines.
- `Span` and `Spans` mix styles on one row, and `Style` gains `italic`, `underline` and `strikethrough`.
- `Layers`, `Floating`, `Edge`, `Fill` and `CursorMerge` draw floating panes over a base widget, and `Surface::overlay` composes surfaces. `Rect`, `Size` and `Insets` describe geometry.
- `VerticalViewport`, `VerticalSize` and `ViewportReport` for scrolling containers that report what they showed. `Renderer` and `Runtime` accept a height as well as a width, and `CursorVisibility` controls the cursor. `LiveRuntime::finish_recovering` hands the writer back when finishing fails.
- `local_widget`, `LocalWidgetRef` and `local_` variants of `layout`, `template`, `screw!` (`local_screw!`) and `Layers` for widgets that hold `Rc` or borrow local data. Such a tree renders through a `Renderer` or a synchronous `Runtime`, and cannot be started on a thread.
- `Renderer::colors` and `screw::colors_enabled`. A renderer drops foreground and background colours, including `Indexed` and `Rgb`, when `NO_COLOR` is set to a non-empty value, and keeps bold, dim, italic, underline, strikethrough and reverse. `Renderer::new` reads the environment once, so bang prompts, climax statuses and `Runtime` follow it, and `colors(true)` or `colors(false)` overrides it. `Renderer::new` is no longer `const`.
- `Viewport::of` measures a terminal from any file descriptor, and `Viewport::FALLBACK` is 80 by 24.
- `single_line` drops the controls `Surface::write` drops and turns every line break or tab into a space, for text that must stay on one terminal line. Prompt summary lines use it, so a label or answer carrying an escape or a newline prints as one clean line.

#### bang-terminal

- `ScreenGuard` with `ScreenOptions` for inline or full-screen use, `CursorPolicy` and bracketed paste. `TerminalEvents` and `TerminalPoll` give a pollable input source that yields decoded events, resizes and caught signals. `RawModeOptions` set the read minimum and timeout.
- `CleanupFailures`, `CleanupFailure` and `CleanupStage` report every step that failed when a guard restores the terminal. Guards restore on drop, or earlier through `restore` and `leave` when the caller wants the error.

#### bang-run

- A form config with two fields of the same name is rejected with `duplicate form field name`.
- `bang` binaries exit with status 2 for a usage or configuration error, 130 when the prompt is cancelled, 128 plus the signal number when a signal ended it and 1 for any other failure. A closed stdout pipe exits 141. Errors print as `error: <message>`.

### Changed

#### climax

- **Breaking** `climax::run` is replaced by `climax::try_run`, which keeps its behaviour, and `climax::main`, which reports and returns an `ExitCode`.
- **Breaking** `climax::Error` is a struct with `kind()`, and no longer an enum. The variants `ArgParse`, `Draw`, `Interact`, `Cancelled`, `InputEnded`, `UnexpectedValue` and `Message` become `ErrorKind` values (`Parse`, `Output`, `Io`, `Interactive`, `Cancelled`, `InputEnded`, `InteractionUnavailable`, `InteractionBusy`, `Application` and `Message`), and `ErrorKind` is non-exhaustive. Matching on the old variants must switch to matching `error.kind()`.
- **Breaking** `Context::prompt()`, `PromptContext` and `OutputContext` are removed. Prompts are methods on `Context`, such as `cx.select("Shell").choice("bash", "bash").interact()`, and take the header text where the old builders took an id. `.option` and `.item` become `.choice`, `.run` and `.run_string` become `.interact`, and the result is a `PromptOutcome<T>` and no longer a `bang_core::Value`. The old `climax::prompt` module and its `run_widget` and `replay_events` are replaced by `climax::bang::advanced::interact_widget` and `scripted_interaction`.
- **Breaking** the output helpers `write_value`, `print_value`, `text` and `json` on `climax::output` and on `OutputContext` are removed. Use `cx.output().result(&value).text(...).emit()`. `output::Format` stays, and `Output` handles come only from `Context`.
- **Breaking** `climax::bang` is the `bang` crate, where it was `bang_core`. The prelude and crate root no longer re-export `Value`, `Event`, `Key`, `KeyEvent`, `Modifiers`, `Number`, `Date`, `Reaction`, `Session`, `SessionStatus`, `Widget`, `ActionBinding` or the built-in widgets, nor the `screw` items `Color`, `Role`, `Style` and `Theme`, nor `ParseTrait`. Reach them through `climax::bang::advanced` and `climax::screw`. The prelude brings `pound::Parse` in anonymously, so `Args::try_parse_from(...)` works, and `Parse` and `ValueEnum` in the prelude are the derives.
- **Breaking** the `pty-overlay` feature and `climax::overlay` are removed along with `bang-screw-pty`.
- **Breaking** feature defaults changed. `interactive` no longer implies `render`, `parse` is listed in the defaults, and `structured` is new and on by default. An application that turned off default features and relied on `interactive` for status rendering must enable `render` too.
- **Breaking** `Context` is neither `Send` nor `Sync`, has no `Clone`, and its policy setters take `&mut self`. `Context::new` is no longer a `const fn`.
- **Breaking** the `climax::app` module is private. Its items are re-exported at the crate root, so `climax::app::Context` becomes `climax::Context`.
- **Breaking** `Status::new` is gone. Start a status with `cx.status("message")`, or with `climax::status::message` outside a `Context`.
- **Breaking** the prelude no longer re-exports the `output` and `status` modules. Name them as `climax::output` and `climax::status`.

#### bang

- **Breaking** `confirm` now highlights No unless `default(true)` is set, so pressing Enter declines, and `ConfirmConfig` defaults to `false`. Callers that relied on the old default must pass `default(true)`.
- **Breaking** `bang` 0.0.1 on crates.io was the command-line tool, a binary with a library of the same name. The `bang` crate is now the typed prompt library described above, ships no binary, and has none of the old library items (`run_from_env`, `run_from_args` and `run`).

#### bang-run

- **Breaking** the command-line tool is the `bang-run` package, `cargo install bang-run`, and still installs a binary named `bang`. Its library function `run` returns `Result<String, CliError>`, and `run_from_env` and `run_from_args` are removed. Build a `Cli` with pound's `Parse::parse` or `try_parse_from`, then call `run`.

#### bang-core

- `Form::push_field` and `with_field` panic when a field name is used twice, since the submitted object keeps one value per name and would drop the earlier answer. `ReviewList::selected_index` returns `None` when every row is removed and hidden, and the state keys ignore that case.
- **Breaking** the View layer is removed, namely `View`, `ViewContext`, `ListView`, `ListRow`, `CalendarView`, `CalendarWeek`, `CalendarDay`, `TextInputView`, `CursorPlacement`, `plain_snapshot`, `ViewId`, `CursorAnchor`, `Session::view` and `Session::view_context`. Widgets now draw through screw. `bang_core::Widget` has `screw::Widget` as a supertrait, so a widget implements `screw::Widget::render` to draw into a `Surface` and then `bang_core::Widget` for `id`, `handle` and `current_value`, in place of `Widget::view`. The `Span` and `Role` types that bang-core used to export are screw's.
- **Breaking** `Session::handle` returns `SessionReaction` and no longer `Reaction`.
- **Breaking** `Event` and `Reaction` gained variants (`Event::UnknownEscape`, `Reaction::Action`) and `Key` gained `Function`. All three are now `#[non_exhaustive]`, so matches outside `bang-core` need a wildcard arm, and later input kinds such as mouse or focus will not break them again.
- **Breaking** `ReviewAction` is removed. `with_prompt` is removed from `Select`, `MultiSelect` and `ReviewList`. `ReviewList::with_action_output` is replaced by `with_exit_output` and `with_leave_output`. `TextInput::cursor_char_index` is now `cursor_grapheme_index`.

#### bang-screw

- **Breaking** the `bang-screw` crate, published at 0.0.1, is removed and gets no 0.1.0. Its job moved into the widgets themselves, which now render into a screw `Surface`. `BangView` and `RetainedRenderer` have no replacement. To show a bang widget in your own screw layout, render it like any other screw widget, and to run one live use `bang::advanced::interact_widget`.

#### bang-terminal

- **Breaking** `bang-terminal` has its own `Signal` type (`INT`, `TERM`, `HUP` and `QUIT`, with `as_raw` and `name`), and `SignalGuard::poll_signal` returns `Option<Signal>` and no longer `Option<i32>`. A `SignalPoller` from `SignalGuard::poller` lets an event source poll while the guard stays owned elsewhere. `restore_default_and_raise` is removed, because a signal is no longer re-raised.
- **Breaking** the backend uses rustix for termios, polling and pipes. `TerminalModeGuard::activate_stdin` becomes `activate(&fd, RawModeOptions)` for any file descriptor, and `InlineScreenGuard`, `enter_inline_screen` and `leave_inline_screen` become `ScreenGuard::enter` with `ScreenOptions`. `TerminalSize` and `terminal_size` are replaced by `screw::Viewport::of`. `Decoder::feed` and `flush` remain, and `decode_all` is removed.
- **Breaking** `RunOutcome`, `SessionRenderer`, `drive_blocking_session`, `drive_tty_session` and `drive_tty_session_with_signals` are removed from `bang-terminal`. Session driving lives inside `bang`, and `TerminalEvents` is the building block for a loop of your own.

#### screw

- **Breaking** rendering is layered over a grapheme-aware `Surface`. `Cell::new` takes a `&str` holding exactly one grapheme cluster and no longer a `char`, and `Cell` exposes `text()`, `width()` and `style()` in place of public fields. `Surface::write` joins a combining mark or ZWJ sequence onto the previous cell, and drops control characters other than tab and newline.
- **Breaking** `Widget` no longer requires `Send + Sync`. `WidgetRef` is `Arc<dyn Widget + Send + Sync>`, and `widget()` requires `Send + Sync + 'static`. `Line`, `Stack`, `Stateful`, `LayoutBuilder`, `Runtime`, `AutoRuntime`, `PlainRuntime` and `AutoRuntimeBuilder` are generic over their widget handle, defaulting to `WidgetRef`. `Runtime::final_widget` and `finish_with` accept any widget and no longer only a `WidgetRef`, and `PlainRuntime::finish_with` takes it by reference.
- **Breaking** `RenderCtx` has private fields. Read `frame()`, `available_columns()`, `available_rows()`, `viewport()`, `layout_mode()` and `theme()`, and build one with `RenderCtx::new` and the `with_` methods. The old `ctx.width` field is `ctx.available_columns()`.
- **Breaking** `terminal_width`, `terminal_width_or_default`, `stderr_is_terminal` and `FALLBACK_WIDTH` are removed. Use `Viewport::of(&io::stderr())` and `Viewport::FALLBACK`.
- **Breaking** `Grid` and `GridCell` are replaced by `Table`.
- **Breaking** `Style` gains `italic`, `underline` and `strikethrough` fields, so a struct literal that names every field must add them or use `..Style::PLAIN`. `Color` gains bright, indexed and RGB variants, so an exhaustive `match` needs a new arm.
- **Breaking** `Looping::new` takes any iterable of frames and no longer a fixed-size array, `Widget` is implemented for `&str` of any lifetime and not only `&'static str`, and `PlainRuntime::resize` returns an `io::Result`.
- **Breaking** screw depends on rustix, `unicode-segmentation` and `unicode-width`, and no longer on libc.

### Removed

- `bang-screw` and `bang-screw-pty`, the render adapter and PTY overlay for bang widgets. See the Breaking items above for where their functionality went.
- `screw-pty`, the PTY adapter from the 0.0.1 workspace, is now a test-only terminal model with `publish = false` and is not part of this release.
