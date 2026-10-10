// SPDX-License-Identifier: EUPL-1.2

#[cfg(feature = "parse")]
use std::{io, process::ExitCode};
use std::marker::PhantomData;

use crate::Result;

/// Run a parsed command-line application and report its process outcome.
///
/// Help and version are written to stdout with a successful exit code, parse
/// failures are written to stderr with exit code 2, and application failures
/// are written to stderr with exit code 1. A cancellation that reaches this
/// function exits with 130, or with 128 plus the signal number when a signal
/// interrupted a live prompt (see [`crate::Error::signal`]). Both are silent
/// unless related errors are attached, in which case those are printed.
///
/// A typed prompt the user cancels resolves to `PromptOutcome::Leave` so the
/// handler decides what leaving means. A signal delivered during a live prompt
/// instead surfaces as an error that carries the signal. Exit 130 without a
/// signal comes from `Context::with_terminal_application` or a direct
/// `bang` dependency that builds a cancelled error.
#[cfg(feature = "parse")]
pub fn main<C, F>(f: F) -> ExitCode
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    complete(C::try_parse(), f).report()
}

/// Run a parsed application without handling output or process exit status.
#[cfg(feature = "parse")]
pub fn try_run<C, F>(f: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    run_with(C::try_parse()?, f)
}

/// Run an application from supplied arguments without handling process output.
#[cfg(feature = "parse")]
pub fn try_run_from<'a, C, F, I>(args: I, f: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
    I: IntoIterator<Item = &'a str>,
{
    run_with(C::try_parse_from(args)?, f)
}

pub fn run_with<C, F>(command: C, f: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    execute(Context::new(), command, f)
}

fn execute<C, F>(context: Context, command: C, f: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    let output = context.output();
    let diagnostic = context.diagnostic();
    #[cfg(feature = "render")]
    let transient = context.transient.clone();
    match f(context, command) {
        Ok(()) => {
            // Both lifecycles must close: `diagnostic` owns its own result slot,
            // so committing only `output` would silently drop diagnostic results.
            let mut failure = output.commit().err();
            collect_related(&mut failure, diagnostic.commit());
            #[cfg(feature = "render")]
            collect_related(&mut failure, transient.flush_transient());
            failure.map_or(Ok(()), Err)
        },
        Err(error) => {
            output.discard();
            diagnostic.discard();
            #[cfg(feature = "render")]
            let error = match transient.flush_transient() {
                Ok(()) => error,
                Err(cleanup) => error.with_related(cleanup),
            };
            Err(error)
        },
    }
}

#[cfg(feature = "parse")]
fn complete<C, F>(parsed: std::result::Result<C, pound::Error>, f: F) -> Completion
where
    F: FnOnce(Context, C) -> Result<()>,
{
    match parsed {
        Ok(command) => match run_with(command, f) {
            Ok(()) => Completion::success(),
            Err(error) if error.kind() == crate::error::ErrorKind::Cancelled => {
                let code = cancelled_code(&error);
                if error.related_errors().is_empty() {
                    Completion::cancelled(code)
                } else {
                    Completion::error(code, error)
                }
            },
            Err(error) => Completion::error(1, error),
        },
        Err(error) if error.is_exit() => Completion::output(error.render()),
        Err(error) => Completion {
            code: 2,
            stream: Some(CompletionStream::Stderr),
            message: Some(error.render()),
        },
    }
}

#[cfg(feature = "parse")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct Completion {
    code: u8,
    stream: Option<CompletionStream>,
    message: Option<String>,
}

#[cfg(feature = "parse")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompletionStream {
    Stdout,
    Stderr,
}

#[cfg(feature = "parse")]
impl Completion {
    const fn success() -> Self {
        Self {
            code: 0,
            stream: None,
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

    const fn output(message: String) -> Self {
        Self {
            code: 0,
            stream: Some(CompletionStream::Stdout),
            message: Some(message),
        }
    }

    fn error(code: u8, error: impl std::fmt::Display) -> Self {
        Self {
            code,
            stream: Some(CompletionStream::Stderr),
            message: Some(format!("error: {error}")),
        }
    }

    fn report(self) -> ExitCode {
        let Some(stream) = self.stream else {
            return ExitCode::from(self.code);
        };
        let message = self.message.expect("a completion stream has a message");
        let result = match stream {
            CompletionStream::Stdout => write_message(io::stdout().lock(), &message),
            CompletionStream::Stderr => write_message(io::stderr().lock(), &message),
        };
        ExitCode::from(if result.is_ok() { self.code } else { 1 })
    }
}

#[cfg(feature = "parse")]
fn cancelled_code(error: &crate::Error) -> u8 {
    error
        .signal()
        .and_then(|signal| u8::try_from(128 + signal).ok())
        .unwrap_or(130)
}

#[cfg(feature = "parse")]
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
    output: crate::output::Output,
    diagnostic: crate::output::Output,
    terminal: crate::terminal::TerminalPolicy,
    #[cfg(feature = "interactive")]
    interaction: bang::Interaction,
    #[cfg(feature = "interactive")]
    custom_interaction: bool,
    #[cfg(feature = "render")]
    transient:          crate::status::StatusCoordinator,
    _not_send: PhantomData<std::rc::Rc<()>>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    #[must_use]
    pub fn new() -> Self {
        let terminal = crate::terminal::TerminalPolicy::process();
        #[cfg(feature = "render")]
        let transient = crate::status::StatusCoordinator::new(
            crate::output::SharedWriter::stderr(),
            terminal.effective_status_mode(),
        );
        #[cfg(feature = "interactive")]
        let interaction = interaction_for(terminal);
        let diagnostic = crate::output::Output::new(crate::output::Format::Text)
            .with_shared_writer(crate::output::SharedWriter::stderr());
        #[cfg(feature = "render")]
        let output = crate::output::Output::new(crate::output::Format::Text)
            .with_transient(crate::status::TransientNotice::coordinator(transient.clone()));
        #[cfg(not(feature = "render"))]
        let output = crate::output::Output::new(crate::output::Format::Text);
        Self {
            output,
            diagnostic,
            terminal,
            #[cfg(feature = "interactive")]
            interaction,
            #[cfg(feature = "interactive")]
            custom_interaction: false,
            #[cfg(feature = "render")]
            transient,
            _not_send: PhantomData,
        }
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn select<T>(&self, header: impl Into<String>) -> bang::SelectPrompt<T> {
        bang::select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn multi_select<T>(&self, header: impl Into<String>) -> bang::MultiSelectPrompt<T> {
        bang::multi_select(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn search<T>(&self, header: impl Into<String>) -> bang::SearchPrompt<T> {
        bang::search(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn review<T>(&self, header: impl Into<String>) -> bang::ReviewPrompt<T> {
        bang::review(header).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn text(&self, prompt: impl Into<String>) -> bang::TextPrompt {
        bang::text(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn password(&self, prompt: impl Into<String>) -> bang::PasswordPrompt {
        bang::password(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn confirm(&self, prompt: impl Into<String>) -> bang::ConfirmPrompt {
        bang::confirm(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn date(&self, prompt: impl Into<String>) -> bang::DatePrompt {
        bang::date(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn number<T>(&self, prompt: impl Into<String>) -> bang::NumberPrompt<T>
    where
        T: std::str::FromStr + 'static,
        T::Err: std::fmt::Display,
    {
        bang::number(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "render")]
    #[must_use]
    pub fn status(&self, message: impl Into<String>) -> crate::status::Status {
        crate::status::Status::new(message, self.transient.clone())
    }

    #[must_use]
    pub fn output(&self) -> crate::output::Output {
        self.output.clone()
    }

    #[must_use]
    pub const fn output_format(&self) -> crate::output::Format {
        self.output.format()
    }

    #[must_use]
    pub fn with_output_format(mut self, format: crate::output::Format) -> Self {
        self.output = self.output.with_format(format);
        self
    }

    pub fn set_output_format(&mut self, format: crate::output::Format) {
        self.output = self.output.clone().with_format(format);
    }

    /// Sideband output for human-facing context (stderr in text mode).
    ///
    /// Has its own result slot, separate from [`Self::output`], and writes to the
    /// diagnostic stream. Both slots are committed together when the application
    /// handler returns.
    ///
    /// Notice routing and suppression always match [`Self::output`], since a
    /// separately tracked diagnostic notice policy could drift from it.
    #[must_use]
    pub fn diagnostic(&self) -> crate::output::Output {
        self.diagnostic.clone().with_notices_from(&self.output)
    }

    #[must_use]
    pub const fn terminal(&self) -> crate::terminal::TerminalPolicy {
        self.terminal
    }

    #[must_use]
    pub const fn interaction_available(&self) -> bool {
        self.terminal.interaction_available()
    }

    #[cfg_attr(
        not(any(feature = "interactive", feature = "render")),
        allow(clippy::missing_const_for_fn)
    )]
    pub fn set_terminal_capabilities(
        &mut self,
        capabilities: crate::terminal::TerminalCapabilities,
    ) -> Result<()> {
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

    #[cfg_attr(not(feature = "interactive"), allow(clippy::missing_const_for_fn))]
    pub fn set_interaction_mode(&mut self, mode: crate::terminal::InteractionMode) {
        self.terminal.set_interaction_mode(mode);
        #[cfg(feature = "interactive")]
        {
            self.interaction = interaction_for(self.terminal);
            self.custom_interaction = false;
        }
    }

    #[cfg_attr(not(feature = "render"), allow(clippy::missing_const_for_fn))]
    pub fn set_status_mode(&mut self, mode: crate::terminal::StatusMode) -> Result<()> {
        self.terminal.set_status_mode(mode);
        #[cfg(feature = "render")]
        self.transient
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    #[cfg(feature = "interactive")]
    pub fn set_interaction(&mut self, interaction: bang::Interaction) {
        self.terminal
            .set_interaction_mode(crate::terminal::InteractionMode::Force);
        self.interaction = interaction;
        self.custom_interaction = true;
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn with_interaction(mut self, interaction: bang::Interaction) -> Self {
        self.set_interaction(interaction);
        self
    }

    #[must_use]
    pub fn with_output_writer(mut self, writer: impl std::io::Write + Send + 'static) -> Self {
        self.output = self.output.with_writer(writer);
        self
    }

    /// Without the `render` feature notices follow this writer too. With it
    /// they belong to the transient channel, moved by `with_transient_writer`.
    #[must_use]
    pub fn with_diagnostic_writer(mut self, writer: impl std::io::Write + Send + 'static) -> Self {
        let writer = crate::output::SharedWriter::new(writer);
        #[cfg(not(feature = "render"))]
        {
            self.output = self.output.with_notice_writer(writer.clone());
        }
        self.diagnostic = self.diagnostic.with_shared_writer(writer);
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
    #[cfg(feature = "render")]
    pub fn with_transient_writer(
        mut self,
        writer: impl std::io::Write + Send + 'static,
    ) -> Result<Self> {
        if !self.transient.is_idle() {
            return Err(crate::Error::with_source(
                crate::error::ErrorKind::Output,
                std::io::Error::other(
                    "the transient writer cannot change while status output is in use",
                ),
            ));
        }
        let mode = self.terminal.effective_status_mode();
        let previous = std::mem::replace(
            &mut self.transient,
            crate::status::StatusCoordinator::with_width(
                crate::output::SharedWriter::new(writer),
                mode,
                crate::status::WidthSource::FALLBACK,
            ),
        );
        previous.supersede(&self.transient);
        self.output = self.output.with_transient(crate::status::TransientNotice::coordinator(
            self.transient.clone(),
        ));
        Ok(self)
    }

    pub fn with_terminal_capabilities(
        mut self,
        capabilities: crate::terminal::TerminalCapabilities,
    ) -> Result<Self> {
        self.set_terminal_capabilities(capabilities)?;
        Ok(self)
    }

    #[must_use]
    pub fn with_interaction_mode(mut self, mode: crate::terminal::InteractionMode) -> Self {
        self.set_interaction_mode(mode);
        self
    }

    pub fn with_status_mode(mut self, mode: crate::terminal::StatusMode) -> Result<Self> {
        self.set_status_mode(mode)?;
        Ok(self)
    }

    #[cfg(feature = "interactive")]
    fn prompt_interaction(&self) -> bang::Interaction {
        self.interaction.clone()
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
fn interaction_for(terminal: crate::terminal::TerminalPolicy) -> bang::Interaction {
    match terminal.interaction_mode() {
        crate::terminal::InteractionMode::Auto if terminal.interaction_available() => {
            bang::Interaction::live()
        },
        crate::terminal::InteractionMode::Auto | crate::terminal::InteractionMode::Disabled => {
            bang::Interaction::disabled()
        },
        crate::terminal::InteractionMode::Force => bang::Interaction::forced(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
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
        context.set_output_format(crate::output::Format::Json);
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
        context.set_output_format(crate::output::Format::Json);
        context.diagnostic().notice("heads up").unwrap();
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[cfg(feature = "render")]
    #[test]
    fn transient_writer_cannot_change_under_a_live_status() {
        let context = Context::new().with_transient_writer(Sink::default()).unwrap();
        let status = context.status("working").start();
        let error = context.with_transient_writer(Sink::default()).unwrap_err();
        assert_eq!(error.kind(), crate::error::ErrorKind::Output);
        status.finish().unwrap();
    }

    #[cfg(feature = "structured")]
    #[derive(Clone, Default)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    #[cfg(feature = "structured")]
    impl Capture {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[cfg(feature = "structured")]
    impl std::io::Write for Capture {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn context_carries_output_policy() {
        let context = Context::new().with_output_format(crate::output::Format::Json);
        assert_eq!(context.output_format(), crate::output::Format::Json);
        assert_eq!(context.output().format(), crate::output::Format::Json);
    }

    #[test]
    fn run_with_supplies_the_command_and_context() {
        let result = run_with(42, |context, command| {
            assert_eq!(command, 42);
            assert_eq!(context.output_format(), crate::output::Format::Text);
            Ok(())
        });
        assert!(result.is_ok());
    }

    #[cfg(feature = "structured")]
    #[test]
    fn lifecycle_commits_a_finite_result_after_success() {
        let capture = Capture::default();
        let context = Context::new().with_output_writer(capture.clone());
        execute(context, (), |context, ()| {
            context
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
        let error = execute(context, (), |context, ()| {
            context.output().result(&42).text(|value| value).emit()?;
            Err(crate::Error::message("later failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "later failure");
        assert_eq!(capture.text(), "");
    }

    #[cfg(feature = "parse")]
    #[test]
    fn lifecycle_distinguishes_early_exit_parse_and_application_errors() {
        let help = complete::<(), _>(
            Err(pound::ErrorKind::Help("help".to_owned()).into()),
            |_, ()| unreachable!(),
        );
        assert_eq!(help.code, 0);
        assert_eq!(help.stream, Some(CompletionStream::Stdout));
        assert_eq!(help.message.as_deref(), Some("help"));

        let parse = complete::<(), _>(
            Err(pound::Error {
                kind: pound::ErrorKind::Unknown {
                    arg: "--wat".to_owned(),
                    closest: Some("--watch".to_owned()),
                },
                usage: Some("Usage: demo [OPTION]...".to_owned()),
                help_flag: None,
            }),
            |_, ()| unreachable!(),
        );
        assert_eq!(parse.code, 2);
        assert_eq!(parse.stream, Some(CompletionStream::Stderr));
        assert_eq!(
            parse.message.as_deref(),
            Some("error: unrecognized argument '--wat'\n\n  tip: did you mean '--watch'\n\nUsage: demo [OPTION]...")
        );

        let application = complete(Ok(()), |_, ()| Err(crate::Error::message("boom")));
        assert_eq!(application.code, 1);
        assert_eq!(application.stream, Some(CompletionStream::Stderr));
        assert_eq!(application.message.as_deref(), Some("error: boom"));

        let cancelled = complete(Ok(()), |_, ()| {
            Err(crate::Error::with_source(
                crate::error::ErrorKind::Cancelled,
                std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"),
            ))
        });
        assert_eq!(cancelled.code, 130);
        assert_eq!(cancelled.stream, None);

        let interrupted = complete(Ok(()), |_, ()| {
            let mut error = crate::Error::with_source(
                crate::error::ErrorKind::Cancelled,
                std::io::Error::new(std::io::ErrorKind::Interrupted, "interrupted by signal SIGTERM"),
            );
            error.signal = Some(15);
            Err(error)
        });
        assert_eq!(interrupted.code, 143);
        assert_eq!(interrupted.stream, None);

        let cancelled_with_related = complete(Ok(()), |_, ()| {
            Err(crate::Error::with_source(
                crate::error::ErrorKind::Cancelled,
                std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"),
            )
            .with_related(crate::Error::message("cleanup failed")))
        });
        assert_eq!(cancelled_with_related.code, 130);
        assert_eq!(cancelled_with_related.stream, Some(CompletionStream::Stderr));
        assert!(cancelled_with_related.message.unwrap().contains("cleanup failed"));

        let interrupted_with_cleanup = complete(Ok(()), |_, ()| {
            let mut error = crate::Error::with_source(
                crate::error::ErrorKind::Cancelled,
                std::io::Error::new(std::io::ErrorKind::Interrupted, "interrupted by signal SIGTERM"),
            );
            error.signal = Some(15);
            Err(error.with_related(crate::Error::message("terminal cleanup failed, RawMode: EIO")))
        });
        assert_eq!(interrupted_with_cleanup.code, 143);
        assert_eq!(interrupted_with_cleanup.stream, Some(CompletionStream::Stderr));
        assert!(interrupted_with_cleanup.message.unwrap().contains("RawMode: EIO"));
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn context_injects_scripted_interaction_into_typed_prompts() {
        let interaction = bang::advanced::scripted_interaction([[
            bang::advanced::Event::key(bang::advanced::Key::Down),
            bang::advanced::Event::key(bang::advanced::Key::Enter),
        ]]);
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
            .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(
                false, false, false,
            ))
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
