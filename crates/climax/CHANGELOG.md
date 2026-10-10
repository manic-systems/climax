# Changelog

all notable changes to the climax family are documented here, the format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html)

the argument parser has its own changelog in `crates/pound/CHANGELOG.md`

## [0.1.0] - Unreleased

the family (`climax`, `climax-derive`, `screw`, `bang`, `bang-core`, `bang-terminal`, `bang-run`) releases at one version, 0.0.1 was an early cut

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

- typed prompt library with `select`, `multi_select`, `search`, `review`, `text`, `password`, `confirm`, `date` and `number`
- `Configurable` config type per prompt, and the same settings as builder methods
- `PromptOutcome` with `or_cancel`, `into_option`, `unwrap_or`, `is_submit` and `is_leave`
- dimmed one-line summary after a submitted prompt, `summary(bool)` and `Interaction::with_summaries` to turn it off
- `confirm` answers on `y` and `n`
- `ReviewPromptWithActions` once a review gains its first action
- `bang::Error` with `ErrorKind`, and a signal during a prompt returns `Interrupted` without re-raising
- `bang::advanced`, `bang::terminal` and `bang::screw`
- `bang-core` gains `Reaction::Action`, `Event::UnknownEscape`, `Key::Function`, `SessionReaction`, `TextInput::with_mask`, `Date::unix_days`, `ReviewList::with_exit_output` and `with_leave_output`

#### screw

- text laid out per grapheme cluster, tabs to eight columns, `width`, `truncate`, `pad` and `Align` agree with `Surface::write`
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

#### bang-run

- duplicate form field names are rejected
- exit 2 for usage or configuration errors, 130 for cancellation, 128 plus the signal number, 141 on a closed pipe and 1 otherwise

### Changed

#### climax

- **Breaking** `climax::run` becomes `try_run`, and `main` reports
- **Breaking** `climax::Error` is a struct with `kind()`, and the old variants become `ErrorKind` values
- **Breaking** `Context::prompt()`, `PromptContext` and `OutputContext` are removed, prompts are methods on `Context` taking a header, `.option` and `.item` become `.choice`, `.run` becomes `.interact`
- **Breaking** `write_value`, `print_value`, `text` and `json` are removed for `cx.output().result(&value).text(..).emit()`
- **Breaking** `climax::bang` is the `bang` crate, and the prelude no longer re-exports `bang_core` and `screw` items, use `climax::bang::advanced` and `climax::screw`
- **Breaking** `pty-overlay` and `climax::overlay` are removed
- **Breaking** `interactive` no longer implies `render`, and `parse` and `structured` are in the defaults
- **Breaking** `Context` is not `Send`, `Sync` or `Clone`, its setters take `&mut self` and `Context::new` is not `const`
- **Breaking** `climax::app` is private, use `climax::Context`
- **Breaking** `Status::new` is removed, use `cx.status(..)`
- **Breaking** the prelude no longer re-exports `output` and `status`

#### bang

- **Breaking** `confirm` highlights No unless `default(true)` is set
- **Breaking** `bang` on crates.io was the command-line tool, it is now the prompt library without `run_from_env`, `run_from_args` or `run`

#### bang-run

- **Breaking** the command-line tool is the `bang-run` package and still installs `bang`, `run` returns `Result<String, CliError>` and `run_from_env` and `run_from_args` are removed

#### bang-core

- `Form::push_field` and `with_field` panic on a repeated name
- `ReviewList::selected_index` returns `None` when every row is removed
- **Breaking** the View layer is removed, widgets draw through `screw::Widget`
- **Breaking** `Session::handle` returns `SessionReaction`
- **Breaking** `Event`, `Reaction` and `Key` are `#[non_exhaustive]`
- **Breaking** `ReviewAction`, and `with_prompt` on `Select`, `MultiSelect` and `ReviewList` are removed, `with_action_output` becomes `with_exit_output` and `with_leave_output`, `cursor_char_index` becomes `cursor_grapheme_index`

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

### Removed

- `bang-screw` and `bang-screw-pty`
- `screw-pty` is a test-only model with `publish = false`
