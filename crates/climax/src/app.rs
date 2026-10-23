// SPDX-License-Identifier: EUPL-1.2

use alloc::rc;
#[cfg(feature = "render")] use core::mem;
#[cfg(feature = "parse")] use core::result;
#[cfg(any(feature = "parse", feature = "interactive"))]
use core::str;
use core::{
    fmt,
    marker::PhantomData,
};
#[cfg(any(feature = "render", feature = "interactive"))]
use std::fs;
#[cfg(feature = "render")] use std::io::Error as IoError;
#[cfg(all(feature = "render", feature = "structured"))]
use std::io::IsTerminal as _;
#[cfg(any(feature = "render", feature = "interactive"))]
use std::os::fd::OwnedFd;
use std::{
    io,
    process::ExitCode,
};

#[cfg(all(feature = "render", feature = "structured"))]
use crate::output::PresentationRoute;
#[cfg(feature = "render")]
use crate::status::{
    Status,
    StatusCoordinator,
    TransientNotice,
    WidthSource,
};
#[cfg(all(feature = "interactive", feature = "render"))]
use crate::terminal::{
    TerminalApplication,
    TerminalInput,
};
use crate::{
    Result,
    error::ErrorKind,
    output::{
        Format,
        Output,
        SharedWriter,
    },
    terminal::{
        InteractionMode,
        StatusMode,
        TerminalCapabilities,
        TerminalPolicy,
    },
};

/// Run a parsed command-line application and report its process outcome.
///
/// Help and version are written to stdout with a successful exit code, parse
/// failures are written to stderr with exit code 2, and application failures
/// are written to stderr with exit code 1. A cancellation that reaches this
/// function exits with 130, or with 128 plus the signal number when a signal
/// interrupted a live prompt (see [`crate::Error::signal`]). Both are silent
/// unless related errors are attached, in which case those are printed.
/// [`crate::Error::with_exit_code`] replaces the code for any error kind.
///
/// With the `complete` feature a shell completion request, signalled by the
/// `COMPLETE` environment variable, is answered and exits before anything is
/// parsed.
///
/// The crate docs list the full exit-code mapping. [`main_with`] reports the
/// same way for an application that takes no arguments.
///
/// A typed prompt the user cancels resolves to `PromptOutcome::Leave` so the
/// handler decides what leaving means. A signal delivered during a live prompt
/// instead surfaces as an error that carries the signal. Exit 130 without a
/// signal comes from `Context::with_terminal_application` or a direct
/// `bang` dependency that builds a cancelled error.
#[cfg(feature = "parse")]
pub fn main<C, F>(application: F) -> ExitCode
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    complete(parse_env::<C>(), application).report()
}

/// Run an application that takes no arguments and report its process outcome.
///
/// Nothing is parsed. Errors are reported and mapped to an exit code exactly
/// as `main` does, so a failure prints `error: ...` on stderr and exits 1, a
/// cancellation exits 130, and [`crate::Error::with_exit_code`] picks any other
/// code.
///
/// ```no_run
/// use climax::prelude::*;
///
/// fn main() -> std::process::ExitCode {
///     climax::main_with(|cx: Context| -> climax::Result<()> {
///         cx.output().write_message(std::io::stdout(), "hello")
///     })
/// }
/// ```
pub fn main_with<F>(application: F) -> ExitCode
where
    F: FnOnce(Context) -> Result<()>,
{
    finish(&run_with((), |context, ()| application(context))).report()
}

/// Run a parsed application without handling output or process exit status.
///
/// With the `complete` feature a shell completion request answers and exits the
/// process, as `main` does.
///
/// # Errors
///
/// Fails with the parse error when the arguments are invalid or help was
/// requested, and otherwise with whatever the application returns.
#[cfg(feature = "parse")]
pub fn try_run<C, F>(application: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    run_with(parse_env::<C>()?, application)
}

/// Run an application from supplied arguments without handling process output.
///
/// # Errors
///
/// Fails with the parse error when the arguments are invalid or help was
/// requested, and otherwise with whatever the application returns.
#[cfg(feature = "parse")]
pub fn try_run_from<'a, C, F, I>(args: I, application: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
    I: IntoIterator<Item = &'a str>,
{
    run_with(C::try_parse_from(args)?, application)
}

/// Run an application from an already built command and report its result.
///
/// The handler gets a fresh `Context` and the command. When it succeeds the
/// registered results are written and transient output is flushed. When it
/// fails they are discarded and its error is returned, with any cleanup failure
/// attached as a related error. Nothing is parsed and no process exit status is
/// chosen, which `main` does for a parsed command.
///
/// # Errors
///
/// Fails with whatever `application` returns, or with the failure to close the
/// output and status lifecycles it opened.
pub fn run_with<C, F>(command: C, application: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    execute(Context::new(), command, application)
}

pub(crate) fn execute<C, F>(context: Context, command: C, application: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    let output = context.output();
    let diagnostic = context.diagnostic();
    #[cfg(feature = "render")]
    let transient = context.transient.clone();
    match application(context, command) {
        Ok(()) => {
            // Both lifecycles must close: `diagnostic` owns its own result
            // slot, so committing only `output` would silently drop
            // diagnostic results.
            let mut failure = output.commit().err();
            collect_related(&mut failure, diagnostic.commit());
            #[cfg(feature = "render")]
            collect_related(&mut failure, transient.flush_transient());
            failure.map_or(Ok(()), Err)
        },
        #[cfg_attr(
            not(feature = "render"),
            expect(unused_mut, reason = "only render mutates the error")
        )]
        Err(mut error) => {
            output.discard();
            diagnostic.discard();
            #[cfg(feature = "render")]
            if let Err(cleanup) = transient.flush_transient() {
                error = error.with_related(cleanup);
            }
            Err(error)
        },
    }
}

#[cfg(feature = "parse")]
fn parse_env<C: pound::Parse>() -> result::Result<C, pound::Error> {
    #[cfg(feature = "complete")]
    caress::handle(C::SPEC);
    C::try_parse()
}

#[cfg(feature = "parse")]
fn complete<C, F>(parsed: result::Result<C, pound::Error>, application: F) -> Completion
where
    F: FnOnce(Context, C) -> Result<()>,
{
    match parsed {
        Ok(command) => finish(&run_with(command, application)),
        Err(error) => parse_completion(&error, pound::Term::stdout(), pound::Term::stderr()),
    }
}

#[cfg(feature = "parse")]
pub(crate) fn parse_completion(
    error: &pound::Error,
    stdout: pound::Term,
    stderr: pound::Term,
) -> Completion {
    if error.is_exit() {
        Completion::output(error.render(stdout))
    } else {
        Completion {
            code:    2,
            stream:  Some(CompletionStream::Stderr),
            message: Some(error.render(stderr)),
        }
    }
}

pub(crate) fn finish(result: &Result<()>) -> Completion {
    match result {
        Ok(()) => Completion::success(),
        Err(error) if error.kind() == ErrorKind::Cancelled => {
            let code = error.exit_code().unwrap_or_else(|| cancelled_code(error));
            if error.related_errors().is_empty() {
                Completion::cancelled(code)
            } else {
                Completion::error(code, error)
            }
        },
        Err(error) => Completion::error(error.exit_code().unwrap_or(1), error),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Completion {
    code:    u8,
    stream:  Option<CompletionStream>,
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletionStream {
    #[cfg(feature = "parse")]
    Stdout,
    Stderr,
}

impl Completion {
    const fn success() -> Self {
        Self {
            code:    0,
            stream:  None,
            message: None,
        }
    }

    const fn cancelled(code: u8) -> Self {
        Self {
            code,
            stream: None,
            message: None,
        }
    }

    #[cfg(feature = "parse")]
    const fn output(message: String) -> Self {
        Self {
            code:    0,
            stream:  Some(CompletionStream::Stdout),
            message: Some(message),
        }
    }

    fn error(code: u8, error: impl fmt::Display) -> Self {
        Self {
            code,
            stream: Some(CompletionStream::Stderr),
            message: Some(format!("error: {error}")),
        }
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn into_parts(self) -> (u8, Option<(CompletionStream, String)>) {
        (self.code, self.stream.zip(self.message))
    }

    #[expect(
        clippy::expect_used,
        reason = "a completion with a stream always carries its message"
    )]
    fn report(self) -> ExitCode {
        let Some(stream) = self.stream else {
            return ExitCode::from(self.code);
        };
        let message = self.message.expect("a completion stream has a message");
        let result = match stream {
            #[cfg(feature = "parse")]
            CompletionStream::Stdout => write_message(io::stdout().lock(), &message),
            CompletionStream::Stderr => write_message(io::stderr().lock(), &message),
        };
        ExitCode::from(Self::code_after_write(self.code, stream, &result))
    }

    fn code_after_write(code: u8, stream: CompletionStream, result: &io::Result<()>) -> u8 {
        match result {
            Ok(()) => code,
            // 128 + SIGPIPE, what pound's Error::exit reports for the same failure
            Err(error)
                if stream != CompletionStream::Stderr
                    && error.kind() == io::ErrorKind::BrokenPipe =>
            {
                141
            },
            Err(_) => 1,
        }
    }
}

fn cancelled_code(error: &crate::Error) -> u8 {
    error
        .signal()
        .and_then(|signal| u8::try_from(128 + signal).ok())
        .unwrap_or(130)
}

fn write_message(mut writer: impl io::Write, message: &str) -> io::Result<()> {
    writer.write_all(message.as_bytes())?;
    writer.write_all(b"\n")
}

/// Application policy and access to the composed command-line facilities.
///
/// Not cloneable, because interaction and terminal policy must share one owner.
/// `Context` is not `Send`, in every feature configuration, and stays on the
/// thread that built it.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<climax::Context>();
/// ```
#[derive(Debug)]
pub struct Context {
    output:             Output,
    diagnostic:         Output,
    terminal:           TerminalPolicy,
    #[cfg(feature = "interactive")]
    interaction:        bang::Interaction,
    #[cfg(feature = "interactive")]
    custom_interaction: bool,
    #[cfg(feature = "interactive")]
    prompt_summaries:   Option<bool>,
    #[cfg(feature = "render")]
    transient:          StatusCoordinator,
    #[cfg(any(feature = "render", feature = "interactive"))]
    terminal_handle:    Option<OwnedFd>,
    _not_send:          PhantomData<rc::Rc<()>>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    #[must_use]
    /// A context for the current process, using stdin, stdout and stderr.
    ///
    /// Terminal capabilities are detected once here, and output starts in text
    /// mode.
    pub fn new() -> Self {
        let terminal = TerminalPolicy::process();
        #[cfg(feature = "render")]
        let transient =
            StatusCoordinator::new(SharedWriter::stderr(), terminal.effective_status_mode());
        #[cfg(feature = "interactive")]
        let interaction = interaction_for(terminal);
        #[cfg(all(feature = "render", feature = "structured"))]
        let output_route =
            if io::stdout().is_terminal() && terminal.capabilities().transient_terminal() {
                PresentationRoute::Around(transient.clone())
            } else {
                PresentationRoute::Direct
            };
        let routed_diagnostic =
            Output::new(Format::Text).with_shared_writer(SharedWriter::stderr());
        #[cfg(all(feature = "render", feature = "structured"))]
        let diagnostic = routed_diagnostic.with_route(PresentationRoute::Queued(transient.clone()));
        #[cfg(not(all(feature = "render", feature = "structured")))]
        let diagnostic = routed_diagnostic;
        #[cfg(feature = "render")]
        let transient_output = Output::new(Format::Text)
            .with_transient(TransientNotice::coordinator(transient.clone()));
        #[cfg(all(feature = "render", feature = "structured"))]
        let output = transient_output.with_route(output_route);
        #[cfg(all(feature = "render", not(feature = "structured")))]
        let output = transient_output;
        #[cfg(not(feature = "render"))]
        let output = Output::new(Format::Text);
        Self {
            output,
            diagnostic,
            terminal,
            #[cfg(feature = "interactive")]
            interaction,
            #[cfg(feature = "interactive")]
            custom_interaction: false,
            #[cfg(feature = "interactive")]
            prompt_summaries: None,
            #[cfg(feature = "render")]
            transient,
            #[cfg(any(feature = "render", feature = "interactive"))]
            terminal_handle: None,
            _not_send: PhantomData,
        }
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn select<T>(&self, header: impl Into<String>) -> bang::SelectPrompt<T> {
        bang::select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn multi_select<T>(&self, header: impl Into<String>) -> bang::MultiSelectPrompt<T> {
        bang::multi_select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn search<T>(&self, header: impl Into<String>) -> bang::SearchPrompt<T> {
        bang::search(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn review<T>(&self, header: impl Into<String>) -> bang::ReviewPrompt<T> {
        bang::review(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn text(&self, prompt: impl Into<String>) -> bang::TextPrompt {
        bang::text(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn password(&self, prompt: impl Into<String>) -> bang::PasswordPrompt {
        bang::password(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn confirm(&self, prompt: impl Into<String>) -> bang::ConfirmPrompt {
        bang::confirm(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn date(&self, prompt: impl Into<String>) -> bang::DatePrompt {
        bang::date(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// The builder keeps the interaction driver the context holds now, so a
    /// later `set_interaction_mode` or terminal change does not affect it.
    #[must_use]
    pub fn number<T>(&self, prompt: impl Into<String>) -> bang::NumberPrompt<T>
    where
        T: str::FromStr + 'static,
        T::Err: fmt::Display,
    {
        bang::number(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "render")]
    #[must_use]
    /// Start building a transient status line for a long operation.
    ///
    /// Statuses share one renderer on the transient channel, and prompts
    /// suspend them while they hold the terminal. Call `start`, `finish` or
    /// `during` on the result to show it. Off a terminal `StatusMode::Auto`
    /// prints the final message on success and the failure message on
    /// failure, one plain line each, and `StatusMode::Silent` prints
    /// nothing.
    ///
    /// Ctrl-C during a live status with no prompt open uses the default signal
    /// disposition, so the process dies and the spinner line is left on the
    /// screen. Only a prompt installs the handlers that restore the
    /// terminal and turn the signal into an error.
    pub fn status(&self, message: impl Into<String>) -> Status {
        Status::new(message, self.transient.clone())
    }

    #[must_use]
    /// The output handle for results, streams and notices.
    ///
    /// Every handle shares one lifecycle, so a second finite result through any
    /// of them is an error. Take it after choosing a format with
    /// `Self::set_output_format`.
    pub fn output(&self) -> Output {
        self.output.clone()
    }

    /// The format `output` handles from this context currently use.
    #[must_use]
    pub const fn output_format(&self) -> Format {
        self.output.format()
    }

    /// Build this context with `format`, so results and streams are written
    /// as JSON under [`Format::Json`](crate::output::Format::Json).
    ///
    /// A handle taken from [`Self::output`] before the change keeps the old
    /// format, so change it before asking for one. The crate docs have a
    /// recipe for a `--json` flag.
    #[must_use]
    pub fn with_output_format(mut self, format: Format) -> Self {
        self.output = self.output.with_format(format);
        self
    }

    /// Set the format for `output` handles taken after this call.
    ///
    /// Notices are suppressed in JSON mode, registered results serialize as
    /// their natural JSON shape and streams write JSON Lines.
    pub fn set_output_format(&mut self, format: Format) {
        self.output = self.output.clone().with_format(format);
    }

    /// Sideband output for human-facing context (stderr in text mode).
    ///
    /// Has its own result slot, separate from [`Self::output`], and writes to
    /// the diagnostic stream. Both slots are committed together when the
    /// application handler returns.
    ///
    /// Notice routing and suppression always match [`Self::output`], since a
    /// separately tracked diagnostic notice policy could drift from it.
    #[must_use]
    pub fn diagnostic(&self) -> Output {
        self.diagnostic.clone().with_notices_from(&self.output)
    }

    #[must_use]
    /// The terminal policy this context applies, with its capabilities and any
    /// mode overrides.
    pub const fn terminal(&self) -> TerminalPolicy {
        self.terminal
    }

    #[must_use]
    /// Whether prompts can run under the current policy.
    ///
    /// Forced interaction reports `true` and disabled interaction `false`.
    /// Otherwise it needs a terminal stdin, a terminal transient stream and
    /// ANSI support.
    pub const fn interaction_available(&self) -> bool {
        self.terminal.interaction_available()
    }

    /// Run a custom terminal application while prompts and live statuses are
    /// excluded from the configured transient stream.
    ///
    /// Requires both the `interactive` and `render` features.
    ///
    /// Reads from [`Self::with_terminal`]'s handle when one is configured,
    /// otherwise from process stdin.
    ///
    /// # Errors
    ///
    /// Fails when interaction is unavailable, another prompt or application
    /// holds the terminal, or `operation` fails.
    #[cfg(all(feature = "interactive", feature = "render"))]
    pub fn with_terminal_application<T>(
        &self,
        operation: impl FnOnce(&mut TerminalApplication<'_>) -> Result<T>,
    ) -> Result<T> {
        self.acquire_terminal_application()?;
        let _guard = self.transient.application_guard()?;
        let stdin = io::stdin();
        let input: Box<dyn TerminalInput + '_> = match &self.terminal_handle {
            Some(handle) => Box::new(fs::File::from(handle.try_clone()?)),
            None => Box::new(stdin.lock()),
        };
        let mut terminal = TerminalApplication::new(
            input,
            Box::new(self.transient.writer()),
            self.terminal.capabilities(),
        );
        operation(&mut terminal)
    }

    /// Move prompts, statuses, and notices onto a caller-owned terminal
    /// handle, such as `/dev/tty` opened for reading and writing, instead of
    /// process stdin and stderr.
    ///
    /// Available with `interactive`, `render` or both. Without `render` only
    /// prompts and notices move, since there is no status to place. The live
    /// region follows the handle's width as the terminal is resized.
    ///
    /// Diagnostics stay on process stderr, routed around the live region when
    /// stderr is a terminal. Output still routes around the transient region
    /// only when both stdout and `terminal` are terminals, the same check
    /// [`Self::new`] makes against stdout and stderr. A custom interaction set
    /// through `with_interaction` is left in place. Fails while a status is
    /// live, because the coordinator's writer cannot change out from under it.
    /// A status started from another thread while the replacement runs may
    /// stay on the previous coordinator.
    ///
    /// # Errors
    ///
    /// Fails while a status is live or when the handle cannot be adopted.
    #[cfg(any(feature = "render", feature = "interactive"))]
    pub fn with_terminal(mut self, terminal: impl Into<OwnedFd>) -> Result<Self> {
        #[cfg(feature = "render")]
        if !self.transient.is_idle() {
            return Err(crate::Error::with_source(
                ErrorKind::Output,
                IoError::other("the terminal handle cannot change while status output is in use"),
            ));
        }
        let handle = terminal.into();
        let capabilities = TerminalCapabilities::detect_on(&handle);
        self.terminal.set_capabilities(capabilities);

        #[cfg(feature = "render")]
        {
            let coordinator_writer = SharedWriter::new(fs::File::from(handle.try_clone()?));
            let mode = self.terminal.effective_status_mode();
            let width = WidthSource::Terminal(handle.try_clone()?);
            let previous = mem::replace(
                &mut self.transient,
                StatusCoordinator::with_width(coordinator_writer, mode, width),
            );
            previous.supersede(&self.transient);
            self.output = self
                .output
                .with_transient(TransientNotice::coordinator(self.transient.clone()));
            #[cfg(feature = "structured")]
            {
                let around = |stream_is_terminal: bool| {
                    if stream_is_terminal && capabilities.transient_terminal() {
                        PresentationRoute::Around(self.transient.clone())
                    } else {
                        PresentationRoute::Direct
                    }
                };
                let output_route = around(io::stdout().is_terminal());
                let diagnostic_route = around(io::stderr().is_terminal());
                self.output = self.output.with_stream_route(output_route);
                self.diagnostic = self.diagnostic.with_stream_route(diagnostic_route);
            }
        }
        #[cfg(not(feature = "render"))]
        {
            self.output = self
                .output
                .with_notice_writer(SharedWriter::new(fs::File::from(handle.try_clone()?)));
        }

        #[cfg(feature = "interactive")]
        if !self.custom_interaction {
            self.interaction = interaction_on(self.terminal, handle.try_clone()?);
        }

        self.terminal_handle = Some(handle);
        Ok(self)
    }

    /// Run a custom terminal application on caller-supplied handles while
    /// prompts and live statuses are excluded from the transient region.
    ///
    /// # Errors
    ///
    /// Fails when interaction is unavailable, another prompt or application
    /// holds the terminal, or `operation` fails.
    #[cfg(all(feature = "interactive", feature = "render"))]
    pub fn with_terminal_application_on<'a, T, I, O>(
        &self,
        input: I,
        output: O,
        operation: impl FnOnce(&mut TerminalApplication<'a>) -> Result<T>,
    ) -> Result<T>
    where
        I: TerminalInput + 'a,
        O: io::Write + 'a,
    {
        self.acquire_terminal_application()?;
        let _guard = self.transient.application_guard()?;
        let mut terminal = TerminalApplication::new(
            Box::new(input),
            Box::new(output),
            self.terminal.capabilities(),
        );
        operation(&mut terminal)
    }

    #[cfg(all(feature = "interactive", feature = "render"))]
    fn acquire_terminal_application(&self) -> Result<()> {
        if !self.terminal.interaction_available() {
            return Err(crate::Error::from(bang::Error::interaction_unavailable()));
        }
        Ok(())
    }

    #[cfg_attr(
        not(any(feature = "interactive", feature = "render")),
        expect(
            clippy::missing_const_for_fn,
            reason = "const only once the gated code is compiled in"
        )
    )]
    /// Replace the detected terminal capabilities, for tests or for a host that
    /// knows better.
    ///
    /// The interaction driver is rederived unless one was injected, and the
    /// status mode is reapplied, which fails when the status coordinator
    /// cannot switch.
    ///
    /// # Errors
    ///
    /// Fails when the status coordinator cannot switch to the resulting status
    /// mode.
    pub fn set_terminal_capabilities(&mut self, capabilities: TerminalCapabilities) -> Result<()> {
        self.terminal.set_capabilities(capabilities);
        // A caller who injected an interaction owns it; capability detection
        // is not authority to take it back.
        #[cfg(feature = "interactive")]
        if !self.custom_interaction {
            self.set_interaction_mode(self.terminal.interaction_mode());
        }
        #[cfg(feature = "render")]
        self.transient
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    /// Prompt builders made earlier keep the driver they captured, so only
    /// builders made after this call see the new mode.
    #[cfg_attr(
        not(feature = "interactive"),
        expect(
            clippy::missing_const_for_fn,
            reason = "const only once the gated code is compiled in"
        )
    )]
    pub fn set_interaction_mode(&mut self, mode: InteractionMode) {
        self.terminal.set_interaction_mode(mode);
        #[cfg(feature = "interactive")]
        {
            self.interaction = self.policy_interaction();
            self.custom_interaction = false;
        }
    }

    #[cfg(feature = "interactive")]
    fn policy_interaction(&self) -> bang::Interaction {
        if let Some(handle) = &self.terminal_handle {
            return handle.try_clone().map_or_else(
                |_| bang::Interaction::disabled(),
                |duplicate| interaction_on(self.terminal, duplicate),
            );
        }
        interaction_for(self.terminal)
    }

    #[cfg_attr(
        not(feature = "render"),
        expect(
            clippy::missing_const_for_fn,
            reason = "const only once the gated code is compiled in"
        )
    )]
    /// Override how statuses present, see
    /// [`StatusMode`](crate::terminal::StatusMode).
    ///
    /// # Errors
    ///
    /// Fails when the status coordinator cannot apply the resulting mode.
    pub fn set_status_mode(&mut self, mode: StatusMode) -> Result<()> {
        self.terminal.set_status_mode(mode);
        #[cfg(feature = "render")]
        self.transient
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    #[cfg(feature = "interactive")]
    /// Use `interaction` for every prompt made afterwards and force interaction
    /// on, since the caller supplied the driver.
    ///
    /// Later capability changes no longer replace it. Prompts made earlier keep
    /// the driver they captured.
    pub fn set_interaction(&mut self, interaction: bang::Interaction) {
        self.terminal.set_interaction_mode(InteractionMode::Force);
        self.interaction = interaction;
        self.custom_interaction = true;
    }

    /// Choose whether submitted prompts leave a one-line summary in the
    /// scrollback, such as `Deploy to prod? › no`.
    ///
    /// The choice is kept on the context and applied to every prompt built
    /// afterwards, so it survives [`Self::with_terminal`], capability changes
    /// and [`Self::set_interaction`]. A prompt's own `summary` setting still
    /// wins. Without a call each driver keeps its own default, which is on.
    ///
    /// ```
    /// use climax::Context;
    ///
    /// let mut context = Context::new();
    /// context.set_prompt_summaries(false);
    /// ```
    #[cfg(feature = "interactive")]
    pub const fn set_prompt_summaries(&mut self, summaries: bool) {
        self.prompt_summaries = Some(summaries);
    }

    /// Builder form of [`Self::set_prompt_summaries`].
    ///
    /// ```
    /// use climax::Context;
    ///
    /// let context = Context::new().with_prompt_summaries(false);
    /// # drop(context);
    /// ```
    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn with_prompt_summaries(mut self, summaries: bool) -> Self {
        self.set_prompt_summaries(summaries);
        self
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    /// Builder form of [`Self::set_interaction`].
    pub fn with_interaction(mut self, interaction: bang::Interaction) -> Self {
        self.set_interaction(interaction);
        self
    }

    #[must_use]
    /// Write finite results and streams to `writer` instead of stdout, directly
    /// and not around a live status. Notices keep their own destination.
    pub fn with_output_writer(mut self, writer: impl io::Write + Send + 'static) -> Self {
        self.output = self.output.with_writer(writer);
        self
    }

    /// Without the `render` feature notices follow this writer too. With it
    /// they belong to the transient channel, moved by `with_transient_writer`.
    #[must_use]
    pub fn with_diagnostic_writer(mut self, writer: impl io::Write + Send + 'static) -> Self {
        let shared = SharedWriter::new(writer);
        #[cfg(not(feature = "render"))]
        {
            self.output = self.output.with_notice_writer(shared.clone());
        }
        self.diagnostic = self.diagnostic.with_shared_writer(shared);
        #[cfg(all(feature = "render", feature = "structured"))]
        {
            self.diagnostic = self.diagnostic.with_route(PresentationRoute::Direct);
        }
        self
    }

    /// Move status presentation and notices to `writer`.
    ///
    /// Prompts keep the configured interaction driver, including a
    /// caller-owned terminal set with [`Self::with_terminal`], and capability
    /// detection still inspects stderr. The live region has the fallback
    /// width, since a writer says nothing about its terminal. Fails while
    /// statuses are live, a prompt holds the terminal or failed transient
    /// lines are still queued, because those belong to the current writer.
    /// A status started from another thread while the replacement runs may
    /// stay on the previous coordinator.
    ///
    /// # Errors
    ///
    /// Fails while statuses are live, a prompt holds the terminal or failed
    /// transient lines are still queued.
    #[cfg(feature = "render")]
    pub fn with_transient_writer(
        mut self,
        writer: impl io::Write + Send + 'static,
    ) -> Result<Self> {
        if !self.transient.is_idle() {
            return Err(crate::Error::with_source(
                ErrorKind::Output,
                IoError::other("the transient writer cannot change while status output is in use"),
            ));
        }
        let mode = self.terminal.effective_status_mode();
        let previous = mem::replace(
            &mut self.transient,
            StatusCoordinator::with_width(SharedWriter::new(writer), mode, WidthSource::FALLBACK),
        );
        previous.supersede(&self.transient);
        self.output = self
            .output
            .with_transient(TransientNotice::coordinator(self.transient.clone()));
        #[cfg(feature = "structured")]
        {
            self.output = self.output.with_route(PresentationRoute::Direct);
            self.diagnostic = self.diagnostic.with_route(PresentationRoute::Direct);
        }
        Ok(self)
    }

    /// Builder form of [`Self::set_terminal_capabilities`].
    ///
    /// # Errors
    ///
    /// Fails as [`Self::set_terminal_capabilities`] does.
    pub fn with_terminal_capabilities(
        mut self,
        capabilities: TerminalCapabilities,
    ) -> Result<Self> {
        self.set_terminal_capabilities(capabilities)?;
        Ok(self)
    }

    #[must_use]
    /// Builder form of [`Self::set_interaction_mode`].
    pub fn with_interaction_mode(mut self, mode: InteractionMode) -> Self {
        self.set_interaction_mode(mode);
        self
    }

    /// Builder form of [`Self::set_status_mode`], failing for the same reason.
    ///
    /// # Errors
    ///
    /// Fails as [`Self::set_status_mode`] does.
    pub fn with_status_mode(mut self, mode: StatusMode) -> Result<Self> {
        self.set_status_mode(mode)?;
        Ok(self)
    }

    #[cfg(feature = "interactive")]
    fn prompt_interaction(&self) -> bang::Interaction {
        let interaction = self.prompt_summaries.map_or_else(
            || self.interaction.clone(),
            |summaries| self.interaction.clone().with_summaries(summaries),
        );
        #[cfg(feature = "render")]
        {
            guarded_interaction(interaction, &self.transient)
        }
        #[cfg(not(feature = "render"))]
        interaction
    }
}

fn collect_related(failure: &mut Option<crate::Error>, result: Result<()>) {
    if let Err(error) = result {
        *failure = Some(match failure.take() {
            Some(primary) => primary.with_related(error),
            None => error,
        });
    }
}

#[cfg(feature = "interactive")]
fn interaction_for(terminal: TerminalPolicy) -> bang::Interaction {
    match terminal.interaction_mode() {
        InteractionMode::Auto if terminal.interaction_available() => bang::Interaction::live(),
        InteractionMode::Auto | InteractionMode::Disabled => bang::Interaction::disabled(),
        InteractionMode::Force => bang::Interaction::forced(),
    }
}

#[cfg(feature = "interactive")]
fn interaction_on(terminal: TerminalPolicy, handle: OwnedFd) -> bang::Interaction {
    match terminal.interaction_mode() {
        InteractionMode::Auto if terminal.interaction_available() => {
            bang::Interaction::live_on(handle)
        },
        InteractionMode::Auto | InteractionMode::Disabled => bang::Interaction::disabled(),
        InteractionMode::Force => bang::Interaction::forced_on(handle),
    }
}

#[cfg(all(feature = "interactive", feature = "render"))]
fn guarded_interaction(
    interaction: bang::Interaction,
    transient: &StatusCoordinator,
) -> bang::Interaction {
    let shared = transient.clone();
    interaction.with_guard(move || shared.prompt_guard())
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;
    #[cfg(any(
        feature = "parse",
        all(feature = "interactive", feature = "render", feature = "structured")
    ))]
    use std::io::{
        Error as IoError,
        ErrorKind as IoErrorKind,
    };
    use std::sync::Mutex;

    #[cfg(feature = "interactive")]
    use bang::advanced::{
        Event,
        Key,
        scripted_interaction,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_poison| io::Error::other("sink lock poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(not(feature = "render"))]
    #[test]
    fn diagnostic_writer_carries_notices_without_render() {
        let sink = Sink::default();
        let mut context = Context::new().with_diagnostic_writer(sink.clone());
        context.output().notice("heads up").unwrap();
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"heads up\n");

        sink.0.lock().unwrap().clear();
        context.diagnostic().notice("heads up").unwrap();
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"heads up\n");

        sink.0.lock().unwrap().clear();
        context.set_output_format(Format::Json);
        context.diagnostic().notice("heads up").unwrap();
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[cfg(feature = "render")]
    #[test]
    fn diagnostic_notices_follow_the_transient_writer() {
        let sink = Sink::default();
        let mut context = Context::new().with_transient_writer(sink.clone()).unwrap();
        context.diagnostic().notice("heads up").unwrap();
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"heads up\n");

        sink.0.lock().unwrap().clear();
        context.set_output_format(Format::Json);
        context.diagnostic().notice("heads up").unwrap();
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[cfg(feature = "render")]
    #[test]
    fn transient_writer_cannot_change_under_a_live_status() {
        let context = Context::new()
            .with_transient_writer(Sink::default())
            .unwrap();
        let status = context.status("working").start();
        let error = context.with_transient_writer(Sink::default()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        status.finish().unwrap();
    }

    #[cfg(feature = "render")]
    #[test]
    fn terminal_handle_cannot_change_under_a_live_status() {
        let context = Context::new();
        let status = context.status("working").start();
        let handle = OwnedFd::from(fs::File::open("/dev/null").unwrap());
        let error = context.with_terminal(handle).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        status.finish().unwrap();
    }

    #[cfg(all(feature = "interactive", feature = "render", feature = "structured"))]
    #[test]
    fn cleanup_reports_a_failed_notice_queued_on_a_replaced_coordinator() {
        struct BrokenPipe;

        impl io::Write for BrokenPipe {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(IoError::from(IoErrorKind::BrokenPipe))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let context = Context::new()
            .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
            .unwrap()
            .with_interaction_mode(InteractionMode::Force);
        let result = execute(context, (), |scoped, ()| {
            let piped = scoped.with_transient_writer(BrokenPipe)?;
            piped.with_terminal_application(|_| piped.diagnostic().notice("queued"))
        });
        result.unwrap_err();
    }

    #[cfg(any(feature = "structured", feature = "render"))]
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    #[cfg(any(feature = "structured", feature = "render"))]
    impl Capture {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[cfg(any(feature = "structured", feature = "render"))]
    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_poison| io::Error::other("sink lock poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn context_carries_output_policy() {
        let context = Context::new().with_output_format(Format::Json);
        assert_eq!(context.output_format(), Format::Json);
        assert_eq!(context.output().format(), Format::Json);
    }

    #[cfg(all(feature = "interactive", feature = "render", feature = "structured"))]
    #[test]
    fn terminal_application_uses_configured_output_and_excludes_nested_owners() {
        use io::Write as _;

        let capture = Capture::default();
        let context = Context::new()
            .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
            .unwrap()
            .with_interaction_mode(InteractionMode::Force)
            .with_transient_writer(capture.clone())
            .unwrap();

        context
            .with_terminal_application(|terminal| {
                terminal.write_all(b"application").unwrap();
                let nested = context.with_terminal_application(|_| Ok(()));
                assert_eq!(nested.unwrap_err().kind(), ErrorKind::InteractionBusy,);
                Ok(())
            })
            .unwrap();
        assert_eq!(capture.text(), "application");
    }

    #[cfg(feature = "render")]
    #[test]
    fn auto_status_off_a_terminal_prints_plain_lines_and_silent_stays_explicit() {
        let run = |requested: Option<StatusMode>, fail: bool| {
            let capture = Capture::default();
            let mut context = Context::new()
                .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
                .unwrap()
                .with_transient_writer(capture.clone())
                .unwrap();
            if let Some(mode) = requested {
                context.set_status_mode(mode).unwrap();
            }
            drop(
                context
                    .status("working")
                    .final_message("done")
                    .failure_message("failed")
                    .during(|| {
                        if fail {
                            Err(crate::Error::message("x"))
                        } else {
                            Ok(())
                        }
                    }),
            );
            capture.text()
        };
        assert_eq!(run(None, false), "done\n");
        assert_eq!(run(None, true), "failed\n");
        assert_eq!(run(Some(StatusMode::Silent), false), "");
        assert_eq!(run(Some(StatusMode::Silent), true), "");
    }

    #[cfg(all(feature = "interactive", feature = "render"))]
    #[test]
    fn terminal_application_releases_exclusivity_after_an_error() {
        let context = Context::new()
            .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
            .unwrap()
            .with_interaction_mode(InteractionMode::Force);
        let failed = context.with_terminal_application::<()>(|_| Err("failed".into()));
        assert!(failed.is_err());
        context.with_terminal_application(|_| Ok(())).unwrap();
    }

    #[cfg(all(feature = "interactive", feature = "render"))]
    #[test]
    fn terminal_application_accepts_caller_supplied_handles() {
        use std::{
            io::Write as _,
            os::unix::net::UnixStream,
        };

        let (input, _peer) = UnixStream::pair().unwrap();
        let mut output = Vec::new();
        let context = Context::new()
            .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
            .unwrap()
            .with_interaction_mode(InteractionMode::Force);

        context
            .with_terminal_application_on(input, &mut output, |terminal| {
                terminal.write_all(b"custom").unwrap();
                Ok(())
            })
            .unwrap();

        assert_eq!(output, b"custom");
    }

    #[test]
    fn run_with_supplies_the_command_and_context() {
        let result = run_with(42, |context, command| {
            assert_eq!(command, 42);
            assert_eq!(context.output_format(), Format::Text);
            Ok(())
        });
        result.unwrap();
    }

    #[cfg(feature = "structured")]
    #[test]
    fn lifecycle_commits_a_finite_result_after_success() {
        let capture = Capture::default();
        let context = Context::new().with_output_writer(capture.clone());
        execute(context, (), |scoped, ()| {
            scoped
                .output()
                .result(&42)
                .text(|value| format!("answer: {value}"))
                .emit()
        })
        .unwrap();
        assert_eq!(capture.text(), "answer: 42\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn lifecycle_discards_a_finite_result_after_failure() {
        let capture = Capture::default();
        let context = Context::new().with_output_writer(capture.clone());
        let error = execute(context, (), |scoped, ()| {
            scoped.output().result(&42).text(|value| value).emit()?;
            Err(crate::Error::message("later failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "later failure");
        assert_eq!(capture.text(), "");
    }

    #[cfg(feature = "parse")]
    #[test]
    fn a_closed_stdout_pipe_exits_141_and_other_write_failures_exit_1() {
        let broken = Err(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(
            Completion::code_after_write(0, CompletionStream::Stdout, &Ok(())),
            0
        );
        assert_eq!(
            Completion::code_after_write(0, CompletionStream::Stdout, &broken),
            141
        );
        let full = Err(io::Error::from(io::ErrorKind::StorageFull));
        assert_eq!(
            Completion::code_after_write(0, CompletionStream::Stdout, &full),
            1
        );
        assert_eq!(
            Completion::code_after_write(0, CompletionStream::Stderr, &broken),
            1
        );
    }

    #[test]
    fn finish_maps_cancellation_and_explicit_exit_codes() {
        let cancelled = finish(&Err(crate::Error::cancelled()));
        assert_eq!((cancelled.code, cancelled.stream), (130, None));

        let silent = finish(&Err(crate::Error::cancelled().with_exit_code(7)));
        assert_eq!((silent.code, silent.stream), (7, None));

        let reported = finish(&Err(
            crate::Error::message("nothing to do").with_exit_code(3)
        ));
        assert_eq!(reported.code, 3);
        assert_eq!(reported.stream, Some(CompletionStream::Stderr));
        assert_eq!(reported.message.as_deref(), Some("error: nothing to do"));

        assert_eq!(finish(&Err(crate::Error::message("boom"))).code, 1);
        assert_eq!(
            finish(&Err(crate::Error::message("x").with_exit_code(0))).code,
            1
        );
        assert_eq!(
            finish(&Err(crate::Error::cancelled().with_exit_code(0))).code,
            1
        );
        assert_eq!(finish(&Ok(())).code, 0);
    }

    #[cfg(feature = "parse")]
    struct Demo;

    #[cfg(feature = "parse")]
    impl pound::Parse for Demo {
        const SPEC: &'static pound::CommandSpec = &DEMO_SPEC;

        fn from_matches(
            _: &'static pound::CommandSpec,
            _: &pound::Matches<'_>,
        ) -> result::Result<Self, pound::Error> {
            Ok(Self)
        }

        fn update_from_matches(
            &mut self,
            _: &'static pound::CommandSpec,
            _: &pound::Matches<'_>,
        ) -> result::Result<(), pound::Error> {
            Ok(())
        }
    }

    #[cfg(feature = "parse")]
    const DEMO_ARGS: &[pound::ArgSpec] = &[pound::ArgSpec::new(pound::Kind::Flag).long("watch")];

    #[cfg(feature = "parse")]
    const DEMO_SPEC: pound::CommandSpec = pound::CommandSpec::new("demo").args(DEMO_ARGS);

    #[cfg(feature = "parse")]
    #[test]
    fn lifecycle_distinguishes_early_exit_parse_and_application_errors() {
        use pound::Parse as _;

        let plain = pound::Term::plain();

        let help = parse_completion(
            &Demo::try_parse_from(["--help"]).err().unwrap(),
            plain,
            plain,
        );
        assert_eq!(help.code, 0);
        assert_eq!(help.stream, Some(CompletionStream::Stdout));
        assert!(
            help.message
                .as_deref()
                .is_some_and(|text| text.contains("Usage: demo"))
        );

        let parse = parse_completion(
            &Demo::try_parse_from(["--watc"]).err().unwrap(),
            plain,
            plain,
        );
        assert_eq!(parse.code, 2);
        assert_eq!(parse.stream, Some(CompletionStream::Stderr));
        assert!(
            parse.message.as_deref().is_some_and(|text| {
                text.starts_with("error: unrecognized argument '--watc'")
                    && text.contains("tip: did you mean '--watch'")
                    && text.contains("Usage: demo")
            }),
            "got {:?}",
            parse.message
        );

        let colored = parse_completion(
            &Demo::try_parse_from(["--watc"]).err().unwrap(),
            plain,
            pound::Term::new(None, true),
        );
        assert!(colored.message.is_some_and(|text| text.contains("\x1b[")));

        let application = complete(Ok(()), |_, ()| Err(crate::Error::message("boom")));
        assert_eq!(application.code, 1);
        assert_eq!(application.stream, Some(CompletionStream::Stderr));
        assert_eq!(application.message.as_deref(), Some("error: boom"));
    }

    #[cfg(feature = "parse")]
    #[test]
    fn lifecycle_reports_cancellation_with_signal_and_cleanup_codes() {
        let cancelled = complete(Ok(()), |_, ()| {
            Err(crate::Error::with_source(
                ErrorKind::Cancelled,
                IoError::new(IoErrorKind::Interrupted, "cancelled"),
            ))
        });
        assert_eq!(cancelled.code, 130);
        assert_eq!(cancelled.stream, None);

        let interrupted = complete(Ok(()), |_, ()| {
            let mut error = crate::Error::with_source(
                ErrorKind::Cancelled,
                IoError::new(IoErrorKind::Interrupted, "interrupted by signal SIGTERM"),
            );
            error.signal = Some(15);
            Err(error)
        });
        assert_eq!(interrupted.code, 143);
        assert_eq!(interrupted.stream, None);

        let cancelled_with_related = complete(Ok(()), |_, ()| {
            Err(crate::Error::with_source(
                ErrorKind::Cancelled,
                IoError::new(IoErrorKind::Interrupted, "cancelled"),
            )
            .with_related(crate::Error::message("cleanup failed")))
        });
        assert_eq!(cancelled_with_related.code, 130);
        assert_eq!(
            cancelled_with_related.stream,
            Some(CompletionStream::Stderr)
        );
        assert!(
            cancelled_with_related
                .message
                .unwrap()
                .contains("cleanup failed")
        );

        let interrupted_with_cleanup = complete(Ok(()), |_, ()| {
            let mut error = crate::Error::with_source(
                ErrorKind::Cancelled,
                IoError::new(IoErrorKind::Interrupted, "interrupted by signal SIGTERM"),
            );
            error.signal = Some(15);
            Err(error.with_related(crate::Error::message(
                "terminal cleanup failed, RawMode: EIO",
            )))
        });
        assert_eq!(interrupted_with_cleanup.code, 143);
        assert_eq!(
            interrupted_with_cleanup.stream,
            Some(CompletionStream::Stderr)
        );
        assert!(
            interrupted_with_cleanup
                .message
                .unwrap()
                .contains("RawMode: EIO")
        );
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn context_injects_scripted_interaction_into_typed_prompts() {
        let interaction = scripted_interaction([[Event::key(Key::Down), Event::key(Key::Enter)]]);
        let context = Context::new().with_interaction(interaction);
        assert!(context.interaction_available());
        assert_eq!(
            context
                .select("shell")
                .choice("bash", 1)
                .choice("zsh", 2)
                .interact()
                .unwrap(),
            crate::PromptOutcome::Submit(2)
        );
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn auto_policy_rejects_interaction_when_capabilities_are_absent() {
        let context = Context::new()
            .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
            .unwrap();
        let error = context
            .select("shell")
            .choice("bash", 1)
            .interact()
            .unwrap_err();
        assert_eq!(error.kind(), bang::ErrorKind::InteractionUnavailable);
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn context_builds_typed_bang_prompts() {
        let context = Context::new();
        let _select = context.select("shell").choice("bash", 1_u8);
        let _multi = context.multi_select("shells").choice("bash", 1_u8);
        let _search = context.search("shell").choice("bash", 1_u8);
        let _review = context
            .review("shells")
            .item("bash", 1_u8, bang::ReviewState::Unconfirmed)
            .action('a', "accept", true);
        let _text = context.text("Name").placeholder("Ada");
        let _password = context.password("Passphrase");
        let _confirm = context.confirm("Continue");
        let _date = context.date("Due");
        let _number = context.number::<u16>("Port");
    }
}
