// SPDX-License-Identifier: EUPL-1.2

use std::marker::PhantomData;

use crate::Result;

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
    match f(context, command) {
        Ok(()) => {
            // Both lifecycles must close: `diagnostic` owns its own result slot,
            // so committing only `output` would silently drop diagnostic results.
            let mut failure = output.commit().err();
            collect_related(&mut failure, diagnostic.commit());
            failure.map_or(Ok(()), Err)
        },
        Err(error) => {
            output.discard();
            diagnostic.discard();
            Err(error)
        },
    }
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
        Self {
            output: crate::output::Output::new(crate::output::Format::Text),
            diagnostic: crate::output::Output::new(crate::output::Format::Text)
                .with_shared_writer(crate::output::SharedWriter::stderr()),
            terminal,
            #[cfg(feature = "interactive")]
            interaction: interaction_for(terminal),
            #[cfg(feature = "interactive")]
            custom_interaction: false,
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
        crate::status::message(message)
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

    #[cfg_attr(not(feature = "interactive"), allow(clippy::missing_const_for_fn))]
    pub fn set_interaction_mode(&mut self, mode: crate::terminal::InteractionMode) {
        self.terminal.set_interaction_mode(mode);
        #[cfg(feature = "interactive")]
        {
            self.interaction = interaction_for(self.terminal);
            self.custom_interaction = false;
        }
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

    /// Notices follow this writer too, so a diagnostic handle and its paired
    /// output handle never disagree about where human-facing context goes.
    #[must_use]
    pub fn with_diagnostic_writer(mut self, writer: impl std::io::Write + Send + 'static) -> Self {
        let writer = crate::output::SharedWriter::new(writer);
        self.output = self.output.with_notice_writer(writer.clone());
        self.diagnostic = self.diagnostic.with_shared_writer(writer);
        self
    }

    #[must_use]
    pub fn with_interaction_mode(mut self, mode: crate::terminal::InteractionMode) -> Self {
        self.set_interaction_mode(mode);
        self
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

    #[test]
    fn diagnostic_writer_carries_notices() {
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
