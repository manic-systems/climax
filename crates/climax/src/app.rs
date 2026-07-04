#[cfg(feature = "interactive")]
use std::io::Write;

#[cfg(feature = "interactive")]
use bang_core::Value;

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

#[derive(Clone, Debug)]
pub struct Context {
    #[cfg(feature = "interactive")]
    output_format: crate::output::Format,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "interactive")]
            output_format: crate::output::Format::Text,
        }
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn prompt(&self) -> PromptContext {
        PromptContext
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
}

#[cfg(feature = "interactive")]
#[derive(Clone, Copy, Debug, Default)]
pub struct PromptContext;

#[cfg(feature = "interactive")]
impl PromptContext {
    #[must_use]
    pub fn select(self, id: impl Into<String>) -> crate::prompt::SelectPrompt {
        crate::prompt::select(id)
    }

    #[must_use]
    pub fn multi_select(self, id: impl Into<String>) -> crate::prompt::MultiSelectPrompt {
        crate::prompt::multi_select(id)
    }

    #[must_use]
    pub fn search(self, id: impl Into<String>) -> crate::prompt::SearchPrompt {
        crate::prompt::search(id)
    }

    #[must_use]
    pub fn text(self, prompt: impl Into<String>) -> crate::prompt::TextPrompt {
        crate::prompt::text(prompt)
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
