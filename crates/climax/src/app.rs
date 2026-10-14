#[cfg(feature = "interactive")] use std::io::Write;

#[cfg(feature = "interactive")] use bang_core::Value;

use crate::Result;
#[cfg(feature = "interactive")]
use crate::output::{
    Format,
    json,
    print_value,
    text,
    write_value,
};
#[cfg(feature = "interactive")] use crate::prompt;
#[cfg(feature = "render")]
use crate::status::{
    Status,
    message as status_message,
};

/// # Errors
///
/// fails with whatever `main` returns.
#[cfg(feature = "parse")]
pub fn run<C, F>(main: F) -> Result<()>
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
{
    #[cfg(feature = "complete")]
    caress::handle(C::SPEC);
    run_with(C::parse(), main)
}

/// # Errors
///
/// fails with whatever `main` returns.
pub fn run_with<C, F>(command: C, main: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    main(Context::new(), command)
}

#[derive(Clone, Debug)]
pub struct Context {
    #[cfg(feature = "interactive")]
    output_format: Format,
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
            output_format:                                 Format::Text,
        }
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn prompt(&self) -> PromptContext {
        PromptContext
    }

    #[cfg(feature = "render")]
    #[must_use]
    pub fn status(&self, message: impl Into<String>) -> Status {
        status_message(message)
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
    pub const fn output_format(&self) -> Format {
        self.output_format
    }

    #[cfg(feature = "interactive")]
    #[must_use]
    pub const fn with_output_format(mut self, format: Format) -> Self {
        self.output_format = format;
        self
    }

    #[cfg(feature = "interactive")]
    pub const fn set_output_format(&mut self, format: Format) {
        self.output_format = format;
    }
}

#[cfg(feature = "interactive")]
#[derive(Clone, Copy, Debug, Default)]
pub struct PromptContext;

#[cfg(feature = "interactive")]
impl PromptContext {
    #[must_use]
    pub fn select(self, id: impl Into<String>) -> prompt::SelectPrompt {
        prompt::select(id)
    }

    #[must_use]
    pub fn multi_select(self, id: impl Into<String>) -> prompt::MultiSelectPrompt {
        prompt::multi_select(id)
    }

    #[must_use]
    pub fn search(self, id: impl Into<String>) -> prompt::SearchPrompt {
        prompt::search(id)
    }

    #[must_use]
    pub fn text(self, prompt: impl Into<String>) -> prompt::TextPrompt {
        prompt::text(prompt)
    }
}

#[cfg(feature = "interactive")]
#[derive(Clone, Copy, Debug)]
pub struct OutputContext {
    format: Format,
}

#[cfg(feature = "interactive")]
impl Default for OutputContext {
    fn default() -> Self {
        Self {
            format: Format::Text,
        }
    }
}

#[cfg(feature = "interactive")]
impl OutputContext {
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    #[must_use]
    pub const fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// # Errors
    ///
    /// fails with [`crate::Error::Draw`] when writing to `writer` fails.
    pub fn write_value(self, writer: impl Write, value: &Value) -> Result<()> {
        write_value(writer, value, self.format)
    }

    /// # Errors
    ///
    /// fails with [`crate::Error::Draw`] when writing to `writer` fails.
    pub fn write_value_as(self, writer: impl Write, value: &Value, format: Format) -> Result<()> {
        write_value(writer, value, format)
    }

    /// # Errors
    ///
    /// fails with [`crate::Error::Draw`] when writing to stdout fails.
    pub fn print_value(self, value: &Value) -> Result<()> {
        print_value(value, self.format)
    }

    /// # Errors
    ///
    /// fails with [`crate::Error::Draw`] when writing to stdout fails.
    pub fn print_value_as(self, value: &Value, format: Format) -> Result<()> {
        print_value(value, format)
    }

    #[must_use]
    pub fn text(self, value: &Value) -> String {
        text(value)
    }

    #[must_use]
    pub fn json(self, value: &Value) -> String {
        json(value)
    }
}
