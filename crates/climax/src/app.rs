#[cfg(feature = "interactive")]
use std::io::Write;

#[cfg(feature = "interactive")]
use bang_core::Value;

use std::marker::PhantomData;

use crate::Result;

#[cfg(feature = "parse")]
pub fn run<C, F>(f: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    run_with(C::try_parse()?, f)
}

pub fn run_with<C, F>(command: C, f: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    f(Context::new(), command)
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
    terminal: crate::terminal::TerminalPolicy,
    #[cfg(feature = "interactive")]
    interaction: bang::Interaction,
    #[cfg(feature = "interactive")]
    custom_interaction: bool,
    #[cfg(feature = "interactive")]
    output_format: crate::output::Format,
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
            terminal,
            #[cfg(feature = "interactive")]
            interaction: interaction_for(terminal),
            #[cfg(feature = "interactive")]
            custom_interaction: false,
            #[cfg(feature = "interactive")]
            output_format: crate::output::Format::Text,
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

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn output(&self) -> OutputContext {
        OutputContext {
            format: self.output_format,
        }
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn output_format(&self) -> crate::output::Format {
        self.output_format
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn with_output_format(mut self, format: crate::output::Format) -> Self {
        self.output_format = format;
        self
    }

    #[cfg(feature = "interactive")]
    pub const fn set_output_format(&mut self, format: crate::output::Format) {
        self.output_format = format;
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
    pub fn with_interaction_mode(mut self, mode: crate::terminal::InteractionMode) -> Self {
        self.set_interaction_mode(mode);
        self
    }

    #[cfg(feature = "interactive")]
    fn prompt_interaction(&self) -> bang::Interaction {
        self.interaction.clone()
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

#[cfg(feature = "interactive")]
#[derive(Clone, Copy, Debug)]
pub struct OutputContext {
    format: crate::output::Format,
}

#[cfg(feature = "interactive")]
impl Default for OutputContext {
    fn default() -> Self {
        Self {
            format: crate::output::Format::Text,
        }
    }
}

#[cfg(feature = "interactive")]
impl OutputContext {
    #[must_use]
    pub const fn format(&self) -> crate::output::Format {
        self.format
    }

    #[must_use]
    pub const fn with_format(mut self, format: crate::output::Format) -> Self {
        self.format = format;
        self
    }

    pub fn write_value(self, writer: impl Write, value: &Value) -> Result<()> {
        crate::output::write_value(writer, value, self.format)
    }

    pub fn write_value_as(
        self,
        writer: impl Write,
        value: &Value,
        format: crate::output::Format,
    ) -> Result<()> {
        crate::output::write_value(writer, value, format)
    }

    pub fn print_value(self, value: &Value) -> Result<()> {
        crate::output::print_value(value, self.format)
    }

    pub fn print_value_as(self, value: &Value, format: crate::output::Format) -> Result<()> {
        crate::output::print_value(value, format)
    }

    #[must_use]
    pub fn text(self, value: &Value) -> String {
        crate::output::text(value)
    }

    #[must_use]
    pub fn json(self, value: &Value) -> String {
        crate::output::json(value)
    }
}
