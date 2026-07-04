#[cfg(feature = "interactive")]
use std::io::{
    self,
    Write,
};

#[cfg(feature = "interactive")]
use bang_core::{
    OutputFormat,
    Value,
};

#[cfg(feature = "interactive")] use crate::Result;

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    #[default]
    Text,
    Json,
}

#[cfg(feature = "interactive")]
pub fn write_value(mut writer: impl Write, value: &Value, format: Format) -> Result<()> {
    writer.write_all(bang_core::format_output(value, format.into()).as_bytes())?;
    Ok(())
}

#[cfg(feature = "interactive")]
pub fn print_value(value: &Value, format: Format) -> Result<()> {
    write_value(io::stdout().lock(), value, format)
}

#[cfg(feature = "interactive")]
#[must_use]
pub fn text(value: &Value) -> String {
    bang_core::format_text(value)
}

#[cfg(feature = "interactive")]
#[must_use]
pub fn json(value: &Value) -> String {
    bang_core::format_json(value)
}

#[cfg(feature = "interactive")]
impl From<Format> for OutputFormat {
    fn from(value: Format) -> Self {
        match value {
            Format::Text => Self::Text,
            Format::Json => Self::Json,
        }
    }
}
