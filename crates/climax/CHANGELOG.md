# Changelog

all notable changes to the climax family are documented here, the format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html)

the argument parser has its own changelog in `crates/pound/CHANGELOG.md`

## [0.1.0] - Unreleased

the family (`climax`, `climax-derive`, `screw`, `bang`, `bang-core`, `bang-terminal`, `bang-run`) releases at one version, 0.0.1 was an early cut. `pound` and `caress` release on their own

### Added

#### climax

- application facade that re-exports each component whole, `climax::pound` under `parse`, `climax::bang` under `interactive`, `climax::screw` under `render`, `climax::caress` under `complete` and `climax::serde` under `structured`
- exact pins between the climax family crates, `pound` and `caress` at the caret range climax was tested against and `climax-derive` taking `pound-derive-impl` at a caret range, and the dev edge from `pound-derive` to `pound` is path-only
- `main` and `main_with` report and return an `ExitCode`, help and version exit 0, parse failures 2, application errors 1, cancellation 130 and an interrupting signal 128 plus its number
- `try_run`, `try_run_from` and `run_with` skip the reporting
- `main` exits 141 on a closed stdout pipe everywhere, including a `BrokenPipe` i/o error the handler returns unless `with_exit_code` sets a code, prints a parse error as pound's full report and prints the cause chain of an application error
- a final report that cannot be written keeps its intended exit code, except that a closed stdout exits 141
- `Result<T, E = Error>` alias
- `Error::application`, `application_context`, `ResultExt` and the boxed and `anyhow` conversions give an error with an i/o error anywhere in its source chain the kind `Io`, so a wrapped `BrokenPipe` exits 141 like a bare one
- `Output::write_message` writes to the handle's destination, so `testing::run` captures it, and with `render` and `structured` fails instead of drawing into a prompt after ten seconds
- `main_with` answers `--help` and exits 2 on any other argument under `parse`
- removing a status while the status thread is stalled still takes effect once the thread answers, so the status stops drawing
- `Status::widget` reports a panicking widget once through `finish`
- minimum Rust version 1.89
- typed prompts on `Context` returning `PromptOutcome`, with the review types re-exported
- structured output through `Context::output` with `result`, `stream` and `notice`, and `Context::diagnostic` for a second handle
- `.emit()` without `.text` prints the value as a json line in text mode, and `.display()` prints it with `Display`
- json output writes a non-finite float as the string `"NaN"`, `"Infinity"` or `"-Infinity"`, as `bang` does
- `Context::status` coordinator, `Status::during`, `Status::widget` and `StatusMode`
- `Status::progress` draws a counted bar and `set_message` and `finish_with` change the message of a running status
- terminal policy through `TerminalCapabilities`, `InteractionMode` and `StatusMode`, and `with_terminal` and `with_terminal_application` to redirect channels
- `ColorChoice` and `Context::set_color` choose colour for statuses, prompts and the `error:` line, and `Context::color` answers for application output
- `Context::set_prompt_summaries`
- `set_interaction_mode` keeps an interaction injected with `set_interaction`
- `climax::Error` with `ErrorKind`, `signal()` and `related_errors()`, plus `Error::application`, `application_context`, `cancelled` and `with_exit_code`
- `ResultExt` for `.context` and `.app_err`, kept out of the prelude, and `From` for boxed errors, where a `climax::Error` or `bang::Error` inside keeps its kind, signal and exit code
- `anyhow` feature, off by default, adds `From<anyhow::Error>`
- `complete` feature answers shell completion in `main` and `try_run`, and `climax::install` renders completion scripts and man pages
- `climax::testing` with `run`, `run_with`, `Script`, `Capture` and `Outcome`, where `Script::select_nth` and `multi_select_nth` fail on a row the prompt does not have, `Script::unavailable` runs without a terminal, `run_with` needs no feature, and a script left unread panics once and names the error that ended the run
- `Format` and `ColorChoice` are value enums under `derive`, and the prelude brings `ArgValue` into scope
- the facade docs gain structured results, a completion table and recipes, and `examples/pkg.rs` is a derived application

#### climax-derive

- new crate behind the `derive` feature, `#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` expand to paths rooted at `::climax::pound`
- `#[climax::serde(..)]` roots serde's derives at `::climax::serde`, with `structured`

#### bang

- typed prompt library with `select`, `multi_select`, `search`, `review`, `text`, `password`, `confirm`, `date` and `number`
- `default(..)` sets the initial value of `text`, `password`, `number`, `confirm` and `date`
- `Interaction::with_colors` sets colour per interaction
- `page_size(0)` and an out of range `selected` or `checked` are `InvalidConfiguration` errors
- `PromptOutcome` with `or_cancel`, `into_option`, `unwrap_or`, `is_submit` and `is_leave`, all `#[must_use]` like `ReviewExit` and `ReviewOutcome`
- dimmed one-line summary after a submitted prompt, `summary(bool)` and `Interaction::with_summaries` to turn it off
- `confirm` answers on `y` and `n`
- `ReviewPromptWithActions` once a review gains its first action
- `bang::Error` with `ErrorKind`, and a signal during a prompt returns `Interrupted` without re-raising
- `bang::advanced`, `bang::terminal` and `bang::screw`
- `checked_scripted_interaction` and `ScriptCheck` in `bang::advanced` read what a scripted driver left unread, and `Headed`, `format_output` and `OutputFormat` are re-exported there
- `Interaction::live_or_tty` opens `/dev/tty` when stdin or stderr is not a terminal, as `bang-run` does
- a password mask that is a control or zero-width character is refused as `InvalidConfiguration`
- `Interaction::interact` refuses control character and repeated action keys as `InvalidConfiguration`
- `bang` fails to compile on a non-unix target
- `bang-core` gains `Reaction::Action`, `Event::UnknownEscape`, `Event::Row`, `Event::text`, `Key::Function`, `Key::Insert`, `SessionReaction`, `InvalidConfiguration`, `ParseDateError`, `TextInput::with_mask`, `Date::unix_days`, `Date::year`, `month` and `day`, `ReviewList::with_exit_output`, `with_leave_output`, `is_reserved_key` and `fold_key`, `InputWidget::takes_tab`, `MultiSelect::set_header`, `Select::with_help`, and `Debug` for `TextInput`, `SearchSelect`, `Form` and `Session`
- `KeyEvent::new` takes the key and its modifiers, and `KeyEvent` and `Key` are `Copy` and `Hash`
- text inputs scroll to keep the cursor visible, take Ctrl-A, Ctrl-E, Ctrl-B, Ctrl-F, Ctrl-U, Ctrl-K, Alt-B and Alt-F and word movement and deletion, and join a pasted block into one line
- text prompts draw a dim `›` after the label and drop a trailing `:` the caller wrote
- the date picker steps a year with `[` and `]`
- `WidgetId` converts from any `&str`
- list rows and headers end in `…` when clipped, and the help lines name the arrow keys
- action help on the key help line
- named errors with remedies, an unavailable error names the stream that is not a terminal, and escape sequences and bidi controls are stripped from summaries

#### screw

- a widget panic is reported once and `finish()` returns only `a widget panicked on the render thread`
- text laid out per grapheme cluster, tabs to eight columns, `width`, `truncate`, `pad` and `Align` agree with `Surface::write`, and bidi formatting characters and other controls are dropped without splitting a cluster
- `Table` with `header`, `aligns`, `gap`, `flexible` and `overflow`
- `Span`, `Spans`, and `Style` gains `italic`, `underline` and `strikethrough`
- `screw::layers` with `Layers`, `Floating`, `Edge`, `Fill`, `CursorMerge`, `Rect`, `Size` and `Insets`, and `Surface::overlay`
- `VerticalViewport`, `VerticalSize` and `ViewportReport`
- `Renderer` and `Runtime` take a height, `CursorVisibility`, `LiveRuntime::finish_recovering`
- `local_widget` and `LocalWidgetRef` for `Rc` and borrowed widgets
- `println` on `RuntimeHandle`, `LiveRuntime` and `AutoRuntime` prints above a live frame, `AutoRuntime::handle` is added, and live runtimes on stderr follow terminal resizes
- `finish` and drop end a terminal frame with a newline, and the final frame is no longer clipped to the terminal height
- a live runtime dropped while panicking leaves the terminal as it is instead of erasing the panic message
- `is_dumb_terminal`, `sanitised_line` and `render_plain_with` are added, and a dead render thread reports `ErrorKind::Other`
- `Renderer::colors` and `colors_enabled`, `NO_COLOR` drops colours and keeps attributes
- `Viewport::of` and `Viewport::FALLBACK`
- `single_line` for text that must stay on one line
- `Renderer::terminal`, set from stderr by `Renderer::stderr`, and a first frame off a terminal starts without a carriage return

#### bang-terminal

- `ScreenGuard` with `ScreenOptions`, `TerminalEvents`, `TerminalPoll` and `RawModeOptions`
- `CleanupFailures`, `CleanupFailure` and `CleanupStage` report each failed restore step
- xterm function keys and `Insert` decode, invalid UTF-8 is reported at once and a double escape decodes
- Ctrl keys decode for NUL and 0x1C to 0x1F
- handlers install with `SA_RESTART`, `SIGWINCH` wakes resize at once with a 100ms poll as fallback
- an ambiguous escape resolves 50ms after its latest byte and is delivered before any input queued behind it, and an overlong csi or ss3 is one unknown escape
- a pollable source reads a bracketed paste in chunks, so input typed right after the paste end is consumed with it instead of being left for the next reader
- the decoder reads modifier parameters 9 to 16, so Super bindings fire
- `ScreenOptions::leave_bytes` and a panic hook recipe in the crate docs, and re-exports of `Event`, `Key`, `KeyEvent` and `Modifiers`
- `TerminalEvents::resize_from` takes `&impl AsFd` and returns `io::Result`, and the crate fails to compile on a non-unix target

#### bang-run

- unknown config keys, keys that do not apply to the type, duplicate form field names, empty names and labels, malformed or impossible dates and more than one `selected = true` on a single-choice type are rejected
- `review-list` actions need `--action-output` or `action_output`, which also binds g, s and a, and without it the result stays the bare rows and esc exits 130
- `--action ::x` binds `:`
- the prompt opens `/dev/tty` when stdin or stderr is redirected and a terminal is available
- a failed final write exits 1 and a closed pipe exits 141
- config parsing uses the workspace `toml`
- exit 2 for usage or configuration errors, 130 for cancellation, 128 plus the signal number and 1 otherwise
- `--prompt` on `select`, `multi-select`, `search` and `review-list`, and the default text prompt is `text` shown as `text ›`
- the library is hidden from the docs and fails to compile on a non-unix target

### Changed

#### climax

- **Breaking** `climax::run` becomes `try_run`, and `main` reports
- **Breaking** `climax::Error` is a struct with `kind()`, and the old variants become `ErrorKind` values
- **Breaking** the `climax::prompt` functions are removed, prompts are methods on `Context` taking a header, `.option` and `.item` become `.choice`, `.run` becomes `.interact`
- **Breaking** `write_value`, `print_value`, `text` and `json` are removed for `cx.output().result(&value).text(..).emit()`
- **Breaking** `climax::bang` is the `bang` crate, and the prelude no longer re-exports `bang_core` and `screw` items, use `climax::bang::advanced` and `climax::screw`
- **Breaking** the `json` feature is now `structured`, so `features = ["json"]` stops resolving, and `structured` is in the defaults
- **Breaking** `pound` is optional behind `parse`, so `default-features = false` drops `climax::pound` and the entry points
- **Breaking** `interactive` no longer implies `render`
- **Breaking** `Context` is not `Send`, `Sync` or `Clone`, its setters take `&mut self`, and `Context::new` and `Default` are not public, tests use `climax::testing`
- **Breaking** `climax::app` is private, use `climax::Context`
- **Breaking** `Status::new` and `climax::status::message` are removed, use `cx.status(..)`
- **Breaking** `Format` is `#[non_exhaustive]`
- **Breaking** the prelude no longer re-exports `output` and `status`

#### bang

- **Breaking** `confirm` highlights No unless `default(true)` is set
- **Breaking** `bang` on crates.io was the command-line tool, it is now the prompt library without `run_from_env`, `run_from_args` or `run`

#### bang-run

- **Breaking** the command-line tool is the `bang-run` package and still installs `bang`, `run` returns `Result<String, CliError>` and `run_from_env` and `run_from_args` are removed
- **Breaking** `-o` always means `--output`, `--option` has no short flag
- config `prompt` is drawn above `date`, `form` and date fields in a form, and the action names `ctrl-m`, `ctrl-i`, `ctrl-j` and `ctrl-h` are rejected since terminals send other keys

#### bang-core

- an empty select is refused
- `Form` delivers ticks and resizes to every field and the active field sees Esc, Tab and Backtab first
- `Form` scrolls to keep the active field in view with its help line pinned, and the help line fits the active field
- Ctrl, Alt and Super chords on Enter, Esc, Space, Tab and the arrows are ignored
- `ReviewList` refuses custom keys that collide with built-ins, are control characters or repeat in any case, and matches keys whatever the case
- `format_output` writes non-finite floats as `NaN`, `Infinity` and `-Infinity`, strings in JSON and plain in text, an empty list prints nothing in text, and a text list prints dates unquoted and null as an empty line
- `DatePicker` header alignment, `Date` parsing and saturation, and a negative `Date` prints as `-0001-01-01`
- `DatePicker` stays within years 1 to 9999
- `ReviewList::selected_index` returns `None` when every row is removed
- **Breaking** the View layer is removed, widgets draw through `screw::Widget`
- **Breaking** `Widget` is renamed `InputWidget`
- **Breaking** `Session::handle` returns `SessionReaction`
- **Breaking** `Event`, `Reaction`, `Key`, `KeyEvent`, `SessionReaction`, `SessionStatus`, `FocusTarget`, `OutputFormat` and `ReviewState` are `#[non_exhaustive]`
- **Breaking** `KeyEvent::new` takes the key and its modifiers and replaces `with_modifiers`, and `Event::Resize` carries `usize` columns and rows
- **Breaking** `Date` fields are private, read them with `year()`, `month()` and `day()`, and `Date::from_str` fails with `ParseDateError`
- **Breaking** `Form::with_field` and `push_field`, `ActionLayer::with_actions` and `push_action` and `ReviewList::with_custom_actions` return `InvalidConfiguration` for a repeated name, a control character key or a repeated key
- `Reaction::changed` and `SessionReaction::changed` borrow the reaction
- **Breaking** `ReviewAction`, and `with_prompt` on `Select`, `MultiSelect` and `ReviewList` are removed, `with_action_output` becomes `with_exit_output` and `with_leave_output`, `cursor_char_index` becomes `cursor_grapheme_index`
- **Breaking** `Context::request_focus`, `take_focus` and `Session::focus` are removed, `Context` is an empty `#[non_exhaustive]` struct, and a `Form` passes a field's `Focus(Widget(id))` request up instead of resolving it among its own fields

#### bang-terminal

- **Breaking** `Signal` replaces raw `i32`, `restore_default_and_raise` is removed and `SignalGuard::poller` is added
- **Breaking** `SignalGuard::install_terminal_handlers` becomes `SignalGuard::enter`
- **Breaking** rustix backend, `activate_stdin` becomes `TerminalModeGuard::enter`, the inline screen guard becomes `ScreenGuard::enter`, `TerminalSize` becomes `screw::Viewport::of` and `decode_all` is removed
- **Breaking** `RunOutcome`, `SessionRenderer` and the `drive_*` functions are removed

#### screw

- **Breaking** `Cell::new` takes one grapheme cluster as `&str`, and `Cell` exposes `text()`, `width()` and `style()`
- **Breaking** `Widget` no longer requires `Send + Sync`, and the runtimes are generic over their widget handle
- **Breaking** `RenderCtx` has private fields, read its accessors
- **Breaking** `terminal_width`, `terminal_width_or_default`, `stderr_is_terminal` and `FALLBACK_WIDTH` are removed for `Viewport`
- **Breaking** `Grid` and `GridCell` become `Table`
- **Breaking** `Style` and `Color` gain fields and variants
- **Breaking** `Role`, `LayoutMode`, `TickInterest`, `RenderStats`, `TemplateError` and `Style` are `#[non_exhaustive]`
- **Breaking** `Looping::new` takes any iterable and `Widget` is implemented for any `&str`
- **Breaking** depends on rustix, `unicode-segmentation` and `unicode-width` and not libc
- **Breaking** `Runtime::stderr_auto` is now `Runtime::stderr` and the old `Runtime::stderr` is `Runtime::stderr_live`
- **Breaking** `Cell`, `Row` and `RowBreak` move to `screw::surface`
- **Breaking** `TextInput`, `write_plain`, `render_plain_with_frame` and `render_plain_with_frame_and_theme` are removed
- `Runtime::stderr` keeps redirected output at full width, draws plain text off a terminal and stays plain on `TERM=dumb`
- `Renderer::terminal(false)` writes plain rows without styles, `ESC [ K` or carriage returns
- screw and screw-pty fail to compile on a non-unix target
- the first frame starts with a carriage return and each frame reaches the writer in one write

### Removed

- `bang-screw` and `bang-screw-pty`
- `screw-pty` is a test-only model with `publish = false`
