// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Number,
    Value,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[must_use]
pub fn format_output(value: &Value, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format_text(value),
        OutputFormat::Json => format!("{}\n", format_json(value)),
    }
}

#[must_use]
pub fn format_text(value: &Value) -> String {
    match value {
        Value::Null => "\n".to_owned(),
        Value::Bool(bool_value) => format!("{bool_value}\n"),
        Value::String(string_value) => format!("{string_value}\n"),
        Value::Number(number) => format!("{number:?}\n"),
        Value::Date(date) => format!("{date}\n"),
        Value::List(values) => {
            let mut output = values
                .iter()
                .map(|item| {
                    match item {
                        Value::String(string_value) => string_value.clone(),
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
            output.push('\n');
            output
        },
        Value::Object(_) => format!("{}\n", format_json(value)),
    }
}

#[must_use]
pub fn format_json(value: &Value) -> String {
    match value {
        Value::Bool(bool_value) => bool_value.to_string(),
        Value::String(string_value) => format!("\"{}\"", escape_json(string_value)),
        Value::Number(number) => {
            match number {
                Number::Integer(integer_value) => integer_value.to_string(),
                Number::Float(float_value) => float_value.to_string(),
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
                    .map(|(key, object_value)| {
                        format!("\"{}\":{}", escape_json(key), format_json(object_value))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
        Value::Null => "null".to_owned(),
    }
}

#[must_use]
pub fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            match character {
                '"' => "\\\"".chars().collect::<Vec<_>>(),
                '\\' => "\\\\".chars().collect(),
                '\n' => "\\n".chars().collect(),
                '\r' => "\\r".chars().collect(),
                '\t' => "\\t".chars().collect(),
                other => vec![other],
            }
        })
        .collect()
}
