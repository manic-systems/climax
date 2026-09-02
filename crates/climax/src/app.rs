#[cfg(feature = "parse")]
use std::{io, process::ExitCode};

use crate::Result;

/// Run a parsed command-line application and report its process outcome.
///
/// Help and version are written to stdout with a successful exit code, parse
/// failures are written to stderr with exit code 2, and application failures
/// are written to stderr with exit code 1.
#[cfg(feature = "parse")]
pub fn main<C, F>(f: F) -> ExitCode
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    complete(C::try_parse(), f).report()
}

#[cfg(feature = "parse")]
#[deprecated(note = "use climax::main for a process entry point or try_run for embedding")]
pub fn run<C, F>(f: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    try_run(f)
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
    match f(context, command) {
        Ok(()) => output.commit(),
        Err(error) => {
            output.discard();
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
                Completion::cancelled()
            },
            Err(error) => Completion::error(1, error),
        },
        Err(pound::Error::Help(text) | pound::Error::Version(text)) => Completion::output(text),
        Err(error) => Completion::error(2, error),
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

    const fn cancelled() -> Self {
        Self {
            code: 130,
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
fn write_message(mut writer: impl io::Write, message: &str) -> io::Result<()> {
    writer.write_all(message.as_bytes())?;
    writer.write_all(b"\n")
}

/// Application policy and access to the composed command-line facilities.
#[derive(Clone, Debug)]
pub struct Context {
    output: crate::output::Output,
    diagnostic: crate::output::Output,
    terminal: crate::terminal::TerminalPolicy,
    #[cfg(feature = "interactive")]
    interaction: bang::Interaction,
    #[cfg(feature = "render")]
    statuses: crate::status::StatusCoordinator,
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
        let diagnostic = crate::output::SharedWriter::stderr();
        #[cfg(feature = "render")]
        let transient = diagnostic.clone();
        #[cfg(feature = "render")]
        let statuses =
            crate::status::StatusCoordinator::new(transient, terminal.effective_status_mode());
        #[cfg(feature = "interactive")]
        let interaction = interaction_for(terminal);
        Self {
            output: crate::output::Output::new(crate::output::Format::Text)
                .with_notice_writer(diagnostic.clone()),
            diagnostic: crate::output::Output::new(crate::output::Format::Text)
                .with_shared_writer(diagnostic),
            terminal,
            #[cfg(feature = "interactive")]
            interaction,
            #[cfg(feature = "render")]
            statuses,
        }
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn select<T>(&self, id: impl Into<String>) -> bang::SelectPrompt<T> {
        bang::select(id).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn multi_select<T>(&self, id: impl Into<String>) -> bang::MultiSelectPrompt<T> {
        bang::multi_select(id).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn search<T>(&self, id: impl Into<String>) -> bang::SearchPrompt<T> {
        bang::search(id).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn review<T>(&self, id: impl Into<String>) -> bang::ReviewPrompt<T> {
        bang::review(id).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn text(&self, prompt: impl Into<String>) -> bang::TextPrompt {
        bang::text(prompt).interaction(self.prompt_interaction())
    }

    #[cfg(feature = "render")]
    #[must_use]
    pub fn status(&self, message: impl Into<String>) -> crate::status::Status {
        crate::status::Status::new(message, self.statuses.clone())
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

    #[must_use]
    pub fn diagnostic(&self) -> crate::output::Output {
        self.diagnostic.clone()
    }

    #[must_use]
    pub const fn terminal(&self) -> crate::terminal::TerminalPolicy {
        self.terminal
    }

    #[must_use]
    pub const fn interaction_available(&self) -> bool {
        self.terminal.interaction_available()
    }

    /// Run a custom terminal application while prompts and live statuses are
    /// excluded from the configured transient stream.
    #[cfg(all(feature = "interactive", feature = "render"))]
    pub fn with_terminal_application<T>(
        &self,
        operation: impl FnOnce(&mut crate::terminal::TerminalApplication<'_>) -> Result<T>,
    ) -> Result<T> {
        self.acquire_terminal_application()?;
        let _guard = self.statuses.application_guard()?;
        let stdin = std::io::stdin();
        let mut terminal = crate::terminal::TerminalApplication::new(
            Box::new(stdin.lock()),
            Box::new(self.statuses.writer()),
            self.terminal.capabilities(),
        );
        operation(&mut terminal)
    }

    /// Run a custom terminal application on caller-supplied handles while
    /// prompts and live statuses are excluded from the transient region.
    #[cfg(all(feature = "interactive", feature = "render"))]
    pub fn with_terminal_application_on<'a, T, I, O>(
        &self,
        input: I,
        output: O,
        operation: impl FnOnce(&mut crate::terminal::TerminalApplication<'a>) -> Result<T>,
    ) -> Result<T>
    where
        I: crate::terminal::TerminalInput + 'a,
        O: std::io::Write + 'a,
    {
        self.acquire_terminal_application()?;
        let _guard = self.statuses.application_guard()?;
        let mut terminal = crate::terminal::TerminalApplication::new(
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

    pub fn set_terminal_capabilities(
        &mut self,
        capabilities: crate::terminal::TerminalCapabilities,
    ) -> Result<()> {
        self.terminal.set_capabilities(capabilities);
        self.refresh_policy()
    }

    #[cfg_attr(not(feature = "interactive"), allow(clippy::missing_const_for_fn))]
    pub fn set_interaction_mode(&mut self, mode: crate::terminal::InteractionMode) {
        self.terminal.set_interaction_mode(mode);
        #[cfg(feature = "interactive")]
        {
            self.interaction = interaction_for(self.terminal);
        }
    }

    #[cfg_attr(not(feature = "render"), allow(clippy::missing_const_for_fn))]
    pub fn set_status_mode(&mut self, mode: crate::terminal::StatusMode) -> Result<()> {
        self.terminal.set_status_mode(mode);
        #[cfg(feature = "render")]
        self.statuses
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
    }

    #[cfg(feature = "interactive")]
    pub fn set_interaction(&mut self, interaction: bang::Interaction) {
        self.terminal
            .set_interaction_mode(crate::terminal::InteractionMode::Force);
        self.interaction = interaction;
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

    #[must_use]
    pub fn with_diagnostic_writer(mut self, writer: impl std::io::Write + Send + 'static) -> Self {
        let writer = crate::output::SharedWriter::new(writer);
        self.output = self.output.with_notice_writer(writer.clone());
        self.diagnostic = self.diagnostic.with_shared_writer(writer);
        self
    }

    #[cfg(feature = "render")]
    #[must_use]
    pub fn with_transient_writer(mut self, writer: impl std::io::Write + Send + 'static) -> Self {
        self.statuses = crate::status::StatusCoordinator::new(
            crate::output::SharedWriter::new(writer),
            self.terminal.effective_status_mode(),
        );
        self
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
        #[cfg(feature = "render")]
        {
            guarded_interaction(self.interaction.clone(), &self.statuses)
        }
        #[cfg(not(feature = "render"))]
        self.interaction.clone()
    }

    #[cfg_attr(not(feature = "render"), allow(clippy::unnecessary_wraps))]
    fn refresh_policy(&mut self) -> Result<()> {
        self.set_interaction_mode(self.terminal.interaction_mode());
        #[cfg(feature = "render")]
        self.statuses
            .set_mode(self.terminal.effective_status_mode())?;
        Ok(())
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

#[cfg(all(feature = "interactive", feature = "render"))]
fn guarded_interaction(
    interaction: bang::Interaction,
    statuses: &crate::status::StatusCoordinator,
) -> bang::Interaction {
    let statuses = statuses.clone();
    interaction.with_guard(move || statuses.prompt_guard())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(all(feature = "interactive", feature = "render", feature = "structured"))]
    #[test]
    fn terminal_application_uses_configured_output_and_excludes_nested_owners() {
        use std::io::Write as _;

        let capture = Capture::default();
        let context = Context::new()
            .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(
                false, false, false,
            ))
            .unwrap()
            .with_interaction_mode(crate::terminal::InteractionMode::Force)
            .with_transient_writer(capture.clone());

        context
            .with_terminal_application(|terminal| {
                terminal.write_all(b"application").unwrap();
                let nested = context.with_terminal_application(|_| Ok(()));
                assert_eq!(
                    nested.unwrap_err().kind(),
                    crate::error::ErrorKind::InteractionBusy,
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(capture.text(), "application");
    }

    #[cfg(all(feature = "interactive", feature = "render"))]
    #[test]
    fn terminal_application_releases_exclusivity_after_an_error() {
        let context = Context::new()
            .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(
                false, false, false,
            ))
            .unwrap()
            .with_interaction_mode(crate::terminal::InteractionMode::Force);
        let failed = context.with_terminal_application::<()>(|_| Err("failed".into()));
        assert!(failed.is_err());
        context.with_terminal_application(|_| Ok(())).unwrap();
    }

    #[cfg(all(feature = "interactive", feature = "render"))]
    #[test]
    fn terminal_application_accepts_caller_supplied_handles() {
        use std::{io::Write as _, os::unix::net::UnixStream};

        let (input, _peer) = UnixStream::pair().unwrap();
        let mut output = Vec::new();
        let context = Context::new()
            .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(
                false, false, false,
            ))
            .unwrap()
            .with_interaction_mode(crate::terminal::InteractionMode::Force);

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
            Err(pound::Error::Help("help".to_owned())),
            |_, ()| unreachable!(),
        );
        assert_eq!(help.code, 0);
        assert_eq!(help.stream, Some(CompletionStream::Stdout));
        assert_eq!(help.message.as_deref(), Some("help"));

        let parse = complete::<(), _>(
            Err(pound::Error::Unknown("--wat".to_owned())),
            |_, ()| unreachable!(),
        );
        assert_eq!(parse.code, 2);
        assert_eq!(parse.stream, Some(CompletionStream::Stderr));
        assert_eq!(
            parse.message.as_deref(),
            Some("error: unrecognised argument '--wat'")
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
            bang::PromptOutcome::Submit(2)
        );
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn auto_policy_rejects_interaction_when_capabilities_are_absent() {
        let context = Context::new().with_terminal_capabilities(
            crate::terminal::TerminalCapabilities::new(false, false, false),
        ).unwrap();
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
    }
}
