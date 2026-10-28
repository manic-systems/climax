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
- `Status::widget` reports a panicking widget once through `finish`
- minimum Rust version 1.89
- typed prompts on `Context` returning `PromptOutcome`, with the config types and review types re-exported
- structured output through `Context::output` with `result`, `stream` and `notice`, and `Context::diagnostic` for a second handle
- json output writes a non-finite float as the string `"NaN"`, `"Infinity"` or `"-Infinity"`, as `bang` does
- `Context::status` coordinator, `Status::during`, `Status::widget` and `StatusMode`
- terminal policy through `TerminalCapabilities`, `InteractionMode` and `StatusMode`, and `with_terminal`, `with_terminal_application` and `with_*_writer` to redirect channels
- `Context::with_prompt_summaries` and `set_prompt_summaries`
- `set_interaction_mode` keeps an interaction injected with `set_interaction`
- `climax::Error` with `ErrorKind`, `signal()` and `related_errors()`, plus `Error::application`, `application_context`, `cancelled` and `with_exit_code`
- `ResultExt` for `.context` and `.app_err`, kept out of the prelude, and `From` for boxed errors, where a `climax::Error` or `bang::Error` inside keeps its kind, signal and exit code
- `anyhow` feature, off by default, adds `From<anyhow::Error>`
- `#[derive(climax::Parse)]` and `#[derive(climax::ValueEnum)]` through `derive`, and `#[climax::serde(..)]` with `structured`
- `complete` feature answers shell completion in `main` and `try_run`, and `climax::install` renders completion scripts and man pages
- `climax::testing` with `run`, `run_with`, `Script`, `Capture` and `Outcome`, where `Script::select_nth` and `multi_select_nth` fail on a row the prompt does not have, `run_with` needs no feature, and a script left unread panics once and names the error that ended the run

#### bang

- typed prompt library with `select`, `multi_select`, `search`, `review`, `text`, `password`, `confirm`, `date` and `number`
- `Configurable` config type per prompt, and the same settings as builder methods
- `PromptOutcome` with `or_cancel`, `into_option`, `unwrap_or`, `is_submit` and `is_leave`, all `#[must_use]` like `ReviewExit` and `ReviewOutcome`
- dimmed one-line summary after a submitted prompt, `summary(bool)` and `Interaction::with_summaries` to turn it off
- `confirm` answers on `y` and `n`
- `ReviewPromptWithActions` once a review gains its first action
- `bang::Error` with `ErrorKind`, and a signal during a prompt returns `Interrupted` without re-raising
- `bang::advanced`, `bang::terminal` and `bang::screw`
- `checked_scripted_interaction` and `ScriptCheck` in `bang::advanced` read what a scripted driver left unread, and `Headed`, `format_output` and `OutputFormat` are re-exported there
- `Interaction::interact` refuses control character and repeated action keys as `InvalidConfiguration`
- `bang-core` gains `Reaction::Action`, `Event::UnknownEscape`, `Event::Row`, `Key::Function`, `Key::Insert`, `SessionReaction`, `TextInput::with_mask`, `Date::unix_days`, `ReviewList::with_exit_output`, `with_leave_output`, `is_reserved_key` and `fold_key`, `Widget::takes_tab`, and `Debug` for `TextInput`, `SearchSelect`, `Form` and `Session`
- word movement and deletion in `TextInput`
- action help on the key help line
- named errors with remedies, an unavailable error names the stream that is not a terminal, and escape sequences and bidi controls are stripped from summaries

#### screw

- a widget panic is reported once and `finish()` returns only `a widget panicked on the render thread`
- text laid out per grapheme cluster, tabs to eight columns, `width`, `truncate`, `pad` and `Align` agree with `Surface::write`, and bidi formatting characters and other controls are dropped without splitting a cluster
- `Table` with `header`, `aligns`, `gap`, `flexible` and `overflow`
- `Span`, `Spans`, and `Style` gains `italic`, `underline` and `strikethrough`
- `Layers`, `Floating`, `Edge`, `Fill`, `CursorMerge`, `Surface::overlay`, `Rect`, `Size` and `Insets`
- `VerticalViewport`, `VerticalSize` and `ViewportReport`
- `Renderer` and `Runtime` take a height, `CursorVisibility`, `LiveRuntime::finish_recovering`
- `local_widget`, `LocalWidgetRef` and `local_` layout variants for `Rc` and borrowed widgets
- `Renderer::colors` and `colors_enabled`, `NO_COLOR` drops colours and keeps attributes
- `Viewport::of` and `Viewport::FALLBACK`
- `single_line` for text that must stay on one line

#### bang-terminal

- `ScreenGuard` with `ScreenOptions`, `TerminalEvents`, `TerminalPoll` and `RawModeOptions`
- `CleanupFailures`, `CleanupFailure` and `CleanupStage` report each failed restore step
- xterm function keys and `Insert` decode, invalid UTF-8 is reported at once and a double escape decodes
- Ctrl keys decode for NUL and 0x1C to 0x1F
- handlers install with `SA_RESTART`, `SIGWINCH` wakes resize at once with a 100ms poll as fallback
- an ambiguous escape resolves 50ms after its latest byte

#### bang-run

- unknown config keys, keys that do not apply to the type, duplicate form field names, empty names and labels and malformed or impossible dates are rejected
- the prompt opens `/dev/tty` when stdin or stderr is redirected and a terminal is available
- a failed final write exits 1 and a closed pipe exits 141
- config parsing uses the workspace `toml`
- exit 2 for usage or configuration errors, 130 for cancellation, 128 plus the signal number and 1 otherwise

### Changed

#### climax

- **Breaking** `climax::run` becomes `try_run`, and `main` reports
- **Breaking** `climax::Error` is a struct with `kind()`, and the old variants become `ErrorKind` values
- **Breaking** `Context::prompt()`, `PromptContext` and `OutputContext` are removed, prompts are methods on `Context` taking a header, `.option` and `.item` become `.choice`, `.run` becomes `.interact`
- **Breaking** `write_value`, `print_value`, `text` and `json` are removed for `cx.output().result(&value).text(..).emit()`
- **Breaking** `climax::bang` is the `bang` crate, and the prelude no longer re-exports `bang_core` and `screw` items, use `climax::bang::advanced` and `climax::screw`
- **Breaking** `pty-overlay` and `climax::overlay` are removed
- **Breaking** `interactive` no longer implies `render`, `structured` is in the defaults and `climax::prompt` is removed
- **Breaking** `Context` is not `Send`, `Sync` or `Clone`, its setters take `&mut self` and `Context::new` is not `const`
- **Breaking** `climax::app` is private, use `climax::Context`
- **Breaking** `Status::new` is removed, use `cx.status(..)`
- **Breaking** the prelude no longer re-exports `output` and `status`

#### bang

- **Breaking** `confirm` highlights No unless `default(true)` is set
- **Breaking** `bang` on crates.io was the command-line tool, it is now the prompt library without `run_from_env`, `run_from_args` or `run`

#### bang-run

- **Breaking** the command-line tool is the `bang-run` package and still installs `bang`, `run` returns `Result<String, CliError>` and `run_from_env` and `run_from_args` are removed
- **Breaking** `-o` always means `--output`, `--option` has no short flag
- **Breaking** `--hide-removed` is removed, it never hid a row
- config `prompt` is drawn above `date`, `form` and date fields in a form, and the action names `ctrl-m`, `ctrl-i`, `ctrl-j` and `ctrl-h` are rejected since terminals send other keys

#### bang-core

- `Form::push_field` and `with_field` panic on a repeated name, an empty select is refused
- `Form` delivers ticks and resizes to every field and the active field sees Esc, Tab and Backtab first
- `Form` scrolls to keep the active field in view with its help line pinned, and the help line fits the active field
- Ctrl, Alt and Super chords on Enter, Esc, Space, Tab and the arrows are ignored
- `ReviewList` refuses custom keys that collide with built-ins, are control characters or repeat in any case, and matches keys whatever the case
- `ActionLayer` and `ReviewList::with_custom_actions` panic on control character keys and repeated keys
- `format_output` writes non-finite floats as `NaN`, `Infinity` and `-Infinity`, strings in JSON and plain in text, an empty list prints nothing in text, and a text list prints dates unquoted and null as an empty line
- `DatePicker` header alignment, `Date` parsing and saturation, and a negative `Date` prints as `-0001-01-01`
- `ReviewList::selected_index` returns `None` when every row is removed
- **Breaking** the View layer is removed, widgets draw through `screw::Widget`
- **Breaking** `Session::handle` returns `SessionReaction`
- **Breaking** `Event`, `Reaction`, `Key`, `SessionReaction`, `SessionStatus`, `FocusTarget`, `OutputFormat` and `ReviewState` are `#[non_exhaustive]`
- **Breaking** `ReviewAction`, and `with_prompt` on `Select`, `MultiSelect` and `ReviewList` are removed, `with_action_output` becomes `with_exit_output` and `with_leave_output`, `cursor_char_index` becomes `cursor_grapheme_index`
- **Breaking** `Context::request_focus`, `take_focus` and `Session::focus` are removed, `Context` is an empty `#[non_exhaustive]` struct, and a `Form` passes a field's `Focus(Widget(id))` request up instead of resolving it among its own fields

#### bang-terminal

- **Breaking** `Signal` replaces raw `i32`, `restore_default_and_raise` is removed and `SignalGuard::poller` is added
- **Breaking** rustix backend, `activate_stdin` becomes `activate`, the inline screen guard becomes `ScreenGuard::enter`, `TerminalSize` becomes `screw::Viewport::of` and `decode_all` is removed
- **Breaking** `RunOutcome`, `SessionRenderer` and the `drive_*` functions are removed

#### screw

- **Breaking** `Cell::new` takes one grapheme cluster as `&str`, and `Cell` exposes `text()`, `width()` and `style()`
- **Breaking** `Widget` no longer requires `Send + Sync`, and the runtimes are generic over their widget handle
- **Breaking** `RenderCtx` has private fields, read its accessors
- **Breaking** `terminal_width`, `terminal_width_or_default`, `stderr_is_terminal` and `FALLBACK_WIDTH` are removed for `Viewport`
- **Breaking** `Grid` and `GridCell` become `Table`
- **Breaking** `Style` and `Color` gain fields and variants
- **Breaking** `Looping::new` takes any iterable, `Widget` is implemented for any `&str` and `PlainRuntime::resize` returns `io::Result`
- **Breaking** depends on rustix, `unicode-segmentation` and `unicode-width` and not libc
- `Runtime::stderr_auto` keeps redirected output at full width and stays plain on `TERM=dumb`
- the first frame starts with a carriage return and each frame reaches the writer in one write

### Removed

- `bang-screw` and `bang-screw-pty`
- `screw-pty` is a test-only model with `publish = false`
