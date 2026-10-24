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

/// runs a parsed command-line application and reports its process outcome
///
/// help and version exit 0 on stdout, parse failures exit 2 and application
/// failures exit 1, both on stderr. a cancellation exits 130, or 128 plus the
/// signal number after a signal during a live prompt, see
/// [`crate::Error::signal`]. [`crate::Error::with_exit_code`] overrides any
/// code
///
/// a closed pipe while writing output exits 141 without a message
///
/// with `complete`, a shell completion request is answered and exits before
/// parsing
#[cfg(feature = "parse")]
pub fn main<C, F>(application: F) -> ExitCode
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    complete(parse_env::<C>(), application).report()
}

/// runs an application that takes no arguments and reports its process outcome
///
/// errors are reported and exit codes chosen as in `main`
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

/// runs a parsed application without handling output or process exit status
///
/// # Errors
///
/// fails with the parse error when the arguments are invalid or help was
/// requested, and otherwise with whatever the application returns
///
/// with `complete`, a shell completion request is answered and the process
/// exits before this returns, `try_run_from` never does
#[cfg(feature = "parse")]
pub fn try_run<C, F>(application: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    run_with(parse_env::<C>()?, application)
}

/// runs an application from supplied arguments without handling process output
///
/// # Errors
///
/// fails with the parse error when the arguments are invalid or help was
/// requested, and otherwise with whatever the application returns
#[cfg(feature = "parse")]
pub fn try_run_from<'a, C, F, I>(args: I, application: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
    I: IntoIterator<Item = &'a str>,
{
    run_with(C::try_parse_from(args)?, application)
}

/// runs an application from an already built command and reports its result
///
/// on failure the registered results are discarded and its error is returned
///
/// # Errors
///
/// fails with whatever `application` returns, or with the failure to close the
/// output and status lifecycles it opened
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
            // `diagnostic` owns its own result slot, so committing only
            // `output` drops its results
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
        Err(error) if error.is_closed_pipe() => Completion::cancelled(141),
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

    fn error(code: u8, error: &crate::Error) -> Self {
        Self {
            code,
            stream: Some(CompletionStream::Stderr),
            message: Some(error.report()),
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
            // 128 + SIGPIPE, as pound's Error::exit reports
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

/// application policy and access to the composed command-line facilities
///
/// not cloneable, since interaction and terminal policy share one owner, and
/// not `Send` in any feature configuration, so it stays on the thread that
/// built it
///
/// a prompt builder from `Context::select` and the other prompt methods
/// keeps the interaction driver held when it is built, ignoring later mode or
/// terminal changes
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
    /// a context for the current process, using stdin, stdout and stderr
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
    /// builds a prompt that picks one of several choices
    #[must_use]
    pub fn select<T>(&self, header: impl Into<String>) -> bang::SelectPrompt<T> {
        bang::select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that picks any number of choices
    #[must_use]
    pub fn multi_select<T>(&self, header: impl Into<String>) -> bang::MultiSelectPrompt<T> {
        bang::multi_select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that filters its choices as the user types
    #[must_use]
    pub fn search<T>(&self, header: impl Into<String>) -> bang::SearchPrompt<T> {
        bang::search(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that confirms, changes or rejects a list of items
    #[must_use]
    pub fn review<T>(&self, header: impl Into<String>) -> bang::ReviewPrompt<T> {
        bang::review(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that reads a line of text
    #[must_use]
    pub fn text(&self, prompt: impl Into<String>) -> bang::TextPrompt {
        bang::text(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that reads a line of text without echoing it
    #[must_use]
    pub fn password(&self, prompt: impl Into<String>) -> bang::PasswordPrompt {
        bang::password(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that asks a yes or no question
    #[must_use]
    pub fn confirm(&self, prompt: impl Into<String>) -> bang::ConfirmPrompt {
        bang::confirm(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that picks a calendar date
    #[must_use]
    pub fn date(&self, prompt: impl Into<String>) -> bang::DatePrompt {
        bang::date(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    /// builds a prompt that reads a number of type `T`
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
    /// starts building a transient status line for a long operation
    ///
    /// off a terminal `Auto` prints one plain line for a success and for a
    /// failure with a `failure_message`, a failure without one prints
    /// nothing, and `Silent` prints nothing. Ctrl-C during a live status
    /// with no prompt open keeps the default disposition and leaves the
    /// spinner line on screen
    pub fn status(&self, message: impl Into<String>) -> Status {
        Status::new(message, self.transient.clone())
    }

    #[must_use]
    /// output handle for results, streams and notices
    ///
    /// every handle shares one lifecycle, so a second finite result is an error
    pub fn output(&self) -> Output {
        self.output.clone()
    }

    /// format `output` handles from this context currently use
    #[must_use]
    pub const fn output_format(&self) -> Format {
        self.output.format()
    }

    /// builds this context with `format`, so results and streams are written as
    /// JSON under [`Format::Json`]
    ///
    /// handles taken before the change keep the old format
    #[must_use]
    pub fn with_output_format(mut self, format: Format) -> Self {
        self.output = self.output.with_format(format);
        self
    }

    /// sets the format for `output` handles taken after this call
    ///
    /// notices are suppressed in JSON mode and streams write JSON Lines
    pub fn set_output_format(&mut self, format: Format) {
        self.output = self.output.clone().with_format(format);
    }

    /// sideband output for human-facing context on stderr, in the format of
    /// [`Self::output`]
    ///
    /// has its own result slot, committed together with [`Self::output`] when
    /// the handler returns
    #[must_use]
    pub fn diagnostic(&self) -> Output {
        self.diagnostic
            .clone()
            .with_format(self.output.format())
            .with_notices_from(&self.output)
    }

    #[must_use]
    /// terminal policy this context applies, with its capabilities and any mode
    /// overrides
    pub const fn terminal(&self) -> TerminalPolicy {
        self.terminal
    }

    #[must_use]
    /// whether prompts can run under the current policy
    ///
    /// `Force` and `Disabled` override detection
    pub const fn interaction_available(&self) -> bool {
        self.terminal.interaction_available()
    }

    /// runs a custom terminal application while prompts and live statuses are
    /// excluded from the configured transient stream
    ///
    /// reads from [`Self::with_terminal`]'s handle when one is configured, else
    /// process stdin
    ///
    /// # Errors
    ///
    /// fails when interaction is unavailable, another prompt or application
    /// holds the terminal, or `operation` fails
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

    /// moves prompts, statuses and notices onto a caller-owned terminal handle,
    /// such as `/dev/tty` opened for reading and writing, instead of
    /// process stdin and stderr
    ///
    /// without `render` only prompts and notices move. diagnostics stay on
    /// process stderr and output routes around the transient region only
    /// when both stdout and `terminal` are terminals. a status started from
    /// another thread during the swap may stay on the previous coordinator
    ///
    /// # Errors
    ///
    /// fails while a status is live or when the handle cannot be adopted
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

    /// runs a custom terminal application on caller-supplied handles while
    /// prompts and live statuses are excluded from the transient region
    ///
    /// # Errors
    ///
    /// fails when interaction is unavailable, another prompt or application
    /// holds the terminal, or `operation` fails
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
    /// replaces the detected terminal capabilities, for tests or a host that
    /// knows better
    ///
    /// the interaction driver is rederived unless one was injected
    ///
    /// # Errors
    ///
    /// fails when the status coordinator cannot switch to the resulting status
    /// mode
    pub fn set_terminal_capabilities(&mut self, capabilities: TerminalCapabilities) -> Result<()> {
        self.terminal.set_capabilities(capabilities);
        // an injected interaction belongs to the caller, so detection leaves it
        #[cfg(feature = "interactive")]
        if !self.custom_interaction {
            self.set_interaction_mode(self.terminal.interaction_mode());
        }
        #[cfg(feature = "render")]
        self.transient
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    /// prompt builders made earlier keep the driver they captured
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
    /// overrides how statuses present, see
    /// [`StatusMode`]
    ///
    /// # Errors
    ///
    /// fails when the status coordinator cannot apply the resulting mode
    pub fn set_status_mode(&mut self, mode: StatusMode) -> Result<()> {
        self.terminal.set_status_mode(mode);
        #[cfg(feature = "render")]
        self.transient
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    #[cfg(feature = "interactive")]
    /// uses `interaction` for every prompt made afterwards and forces
    /// interaction on
    ///
    /// later capability changes keep it
    pub fn set_interaction(&mut self, interaction: bang::Interaction) {
        self.terminal.set_interaction_mode(InteractionMode::Force);
        self.interaction = interaction;
        self.custom_interaction = true;
    }

    /// whether submitted prompts leave a one-line summary in the scrollback,
    /// such as `Deploy to prod? › no`
    ///
    /// applies to every prompt built afterwards. a prompt's own `summary`
    /// setting still wins
    #[cfg(feature = "interactive")]
    pub const fn set_prompt_summaries(&mut self, summaries: bool) {
        self.prompt_summaries = Some(summaries);
    }

    /// builder form of [`Self::set_prompt_summaries`]
    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn with_prompt_summaries(mut self, summaries: bool) -> Self {
        self.set_prompt_summaries(summaries);
        self
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    /// builder form of [`Self::set_interaction`]
    pub fn with_interaction(mut self, interaction: bang::Interaction) -> Self {
        self.set_interaction(interaction);
        self
    }

    #[must_use]
    /// writes finite results and streams to `writer` instead of stdout,
    /// directly and not around a live status, while notices keep their own
    /// destination
    pub fn with_output_writer(mut self, writer: impl io::Write + Send + 'static) -> Self {
        self.output = self.output.with_writer(writer);
        self
    }

    /// without the `render` feature notices follow this writer too, with it
    /// they belong to the transient channel, moved by
    /// `with_transient_writer`
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

    /// moves status presentation and notices to `writer`
    ///
    /// prompts keep their driver and the live region has the fallback width
    ///
    /// # Errors
    ///
    /// fails while statuses are live, a prompt holds the terminal or failed
    /// transient lines are still queued
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

    /// builder form of [`Self::set_terminal_capabilities`]
    ///
    /// # Errors
    ///
    /// fails as [`Self::set_terminal_capabilities`] does
    pub fn with_terminal_capabilities(
        mut self,
        capabilities: TerminalCapabilities,
    ) -> Result<Self> {
        self.set_terminal_capabilities(capabilities)?;
        Ok(self)
    }

    #[must_use]
    /// builder form of [`Self::set_interaction_mode`]
    pub fn with_interaction_mode(mut self, mode: InteractionMode) -> Self {
        self.set_interaction_mode(mode);
        self
    }

    /// builder form of [`Self::set_status_mode`]
    ///
    /// # Errors
    ///
    /// fails as [`Self::set_status_mode`] does
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
    use core::error::Error as StdError;
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

    struct Failing(io::ErrorKind);

    impl io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn finish_reports_the_cause_chain() {
        #[derive(Debug)]
        struct Layered(io::Error);

        impl fmt::Display for Layered {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("could not save")
            }
        }

        impl StdError for Layered {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(&self.0)
            }
        }

        let completion = finish(&Err(crate::Error::application(Layered(io::Error::other(
            "disk full",
        )))));
        assert_eq!(
            completion.message.as_deref(),
            Some("error: could not save\n  caused by: disk full")
        );
    }

    #[cfg(feature = "anyhow")]
    #[test]
    fn finish_reports_an_anyhow_chain() {
        let error = anyhow::anyhow!("root").context("mid").context("top");
        let completion = finish(&Err(error.into()));
        assert_eq!(
            completion.message.as_deref(),
            Some("error: top\n  caused by: mid\n  caused by: root")
        );
    }

    #[test]
    fn diagnostic_follows_the_output_format() {
        let context = Context::new().with_output_format(Format::Json);
        assert_eq!(context.diagnostic().format(), Format::Json);
        assert_eq!(Context::new().diagnostic().format(), Format::Text);
    }

    #[test]
    fn a_closed_pipe_while_writing_output_exits_141_silently() {
        let output = Context::new().output();
        let closed = output
            .write_message(Failing(io::ErrorKind::BrokenPipe), "hello")
            .unwrap_err();
        let completion = finish(&Err(closed));
        assert_eq!((completion.code, completion.stream), (141, None));

        let full = output
            .write_message(Failing(io::ErrorKind::StorageFull), "hello")
            .unwrap_err();
        assert_eq!(finish(&Err(full)).code, 1);
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
                "terminal cleanup failed, raw mode failed, EIO",
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
                .contains("raw mode failed, EIO")
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
