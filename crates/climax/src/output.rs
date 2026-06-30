#[cfg(feature = "interactive")]
use std::io::{
    self,
    Write,
};

#[cfg(feature = "interactive")]
use bang_core::{
    Number,
    Value,
};

#[cfg(feature = "interactive")] use crate::Result;

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    #[default]
    Text,
    #[cfg(feature = "json")]
    Json,
}

#[cfg(feature = "interactive")]
pub fn write_value(mut writer: impl Write, value: &Value, format: Format) -> Result<()> {
    match format {
        Format::Text => write!(writer, "{}", text(value))?,
        #[cfg(feature = "json")]
        Format::Json => writeln!(writer, "{}", json(value))?,
    }
    Ok(())
}

#[cfg(feature = "interactive")]
pub fn print_value(value: &Value, format: Format) -> Result<()> {
    write_value(io::stdout().lock(), value, format)
}

#[cfg(feature = "interactive")]
#[must_use]
pub fn text(value: &Value) -> String {
    match value {
        Value::Null => "\n".to_owned(),
        Value::Bool(value) => format!("{value}\n"),
        Value::String(value) => format!("{value}\n"),
        Value::Number(number) => format_number(*number) + "\n",
        Value::Date(date) => format!("{:04}-{:02}-{:02}\n", date.year, date.month, date.day),
        Value::List(values) => {
            values
                .iter()
                .map(|value| {
                    match value {
                        Value::String(value) => value.clone(),
                        _ => json_fallback(value),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        },
        _ => json_fallback(value) + "\n",
    }
}

#[cfg(all(feature = "interactive", feature = "json"))]
#[must_use]
pub fn json(value: &Value) -> serde_json::Value {
    match value {
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Number(Number::Integer(value)) => serde_json::Value::Number((*value).into()),
        Value::Number(Number::Float(value)) => {
            serde_json::Number::from_f64(*value)
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        },
        Value::Date(date) => {
            serde_json::Value::String(format!(
                "{:04}-{:02}-{:02}",
                date.year, date.month, date.day
            ))
        },
        Value::List(values) => serde_json::Value::Array(values.iter().map(json).collect()),
        Value::Object(values) => {
            serde_json::Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), json(value)))
                    .collect(),
            )
        },
        _ => serde_json::Value::Null,
    }
}

#[cfg(feature = "interactive")]
fn format_number(number: Number) -> String {
    match number {
        Number::Integer(value) => value.to_string(),
        Number::Float(value) => value.to_string(),
        _ => "null".to_owned(),
    }
}

#[cfg(all(feature = "interactive", feature = "json"))]
fn json_fallback(value: &Value) -> String {
    json(value).to_string()
}

#[cfg(all(feature = "interactive", not(feature = "json")))]
fn json_fallback(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::String(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        Value::Number(number) => format_number(*number),
        Value::Date(date) => format!("\"{:04}-{:02}-{:02}\"", date.year, date.month, date.day),
        Value::List(values) => {
            format!(
                "[{}]",
                values
                    .iter()
                    .map(json_fallback)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
        Value::Object(values) => {
            format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| {
                        format!("\"{}\":{}", key.replace('"', "\\\""), json_fallback(value))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
        _ => "null".to_owned(),
    }
}
