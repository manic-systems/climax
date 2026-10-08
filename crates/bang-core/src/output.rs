// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Number,
    Value,
};

/// how a submitted value is written out
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutputFormat {
    /// plain lines, strings unquoted
    #[default]
    Text,
    /// a single JSON document
    Json,
}

/// renders `value` in `output`, ending with a newline
#[must_use]
pub fn format_output(value: &Value, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format_text(value),
        OutputFormat::Json => format!("{}\n", format_json(value)),
    }
}

/// renders `value` as plain text, a list becomes one line per item, structures
/// fall back to JSON
#[must_use]
pub fn format_text(value: &Value) -> String {
    match value {
        Value::Null => "\n".to_owned(),
        Value::Bool(flag) => format!("{flag}\n"),
        Value::String(text) => format!("{text}\n"),
        Value::Number(number) => format!("{number}\n"),
        Value::Date(date) => format!("{date}\n"),
        Value::List(values) => {
            let mut lines = values
                .iter()
                .map(|item| {
                    match item {
                        Value::String(text) => text.clone(),
                        Value::Null
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::Date(_)
                        | Value::List(_)
                        | Value::Object(_) => format_json(item),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            lines.push('\n');
            lines
        },
        Value::Object(_) => format!("{}\n", format_json(value)),
    }
}

/// renders `value` as JSON, non-finite floats become `null` and dates become
/// strings
#[must_use]
pub fn format_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::String(text) => format!("\"{}\"", escape_json(text)),
        Value::Number(number) => {
            match number {
                Number::Integer(integer) => integer.to_string(),
                // JSON has no NaN or Infinity
                Number::Float(float) if float.is_finite() => float.to_string(),
                Number::Float(_) => "null".to_owned(),
            }
        },
        Value::Date(date) => format!("\"{date}\""),
        Value::List(values) => {
            format!(
                "[{}]",
                values.iter().map(format_json).collect::<Vec<_>>().join(",")
            )
        },
        Value::Object(values) => {
            format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, item)| format!("\"{}\":{}", escape_json(key), format_json(item)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
    }
}

/// escapes `value` for use inside a JSON string literal
#[must_use]
pub fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            match ch {
                '"' => "\\\"".chars().collect::<Vec<_>>(),
                '\\' => "\\\\".chars().collect(),
                '\n' => "\\n".chars().collect(),
                '\r' => "\\r".chars().collect(),
                '\t' => "\\t".chars().collect(),
                '\u{08}' => "\\b".chars().collect(),
                '\u{0C}' => "\\f".chars().collect(),
                control if control.is_control() => {
                    format!("\\u{:04x}", u32::from(control)).chars().collect()
                },
                other => vec![other],
            }
        })
        .collect()
}
