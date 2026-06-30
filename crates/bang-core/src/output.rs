// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Number,
    Value,
};

/// Durable output format for submitted values.
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
        Value::Bool(value) => format!("{value}\n"),
        Value::String(value) => format!("{value}\n"),
        Value::Number(number) => format!("{number:?}\n"),
        Value::Date(date) => format!("{date}\n"),
        Value::List(values) => {
            values
                .iter()
                .map(|value| {
                    match value {
                        Value::String(value) => value.clone(),
                        _ => format_json(value),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        },
        _ => format!("{}\n", format_json(value)),
    }
}

#[must_use]
pub fn format_json(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::String(value) => format!("\"{}\"", escape_json(value)),
        Value::Number(number) => {
            match number {
                Number::Integer(value) => value.to_string(),
                Number::Float(value) => value.to_string(),
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
                    .map(|(key, value)| format!("\"{}\":{}", escape_json(key), format_json(value)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
        _ => "null".to_owned(),
    }
}

#[must_use]
pub fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|value| {
            match value {
                '"' => "\\\"".chars().collect::<Vec<_>>(),
                '\\' => "\\\\".chars().collect(),
                '\n' => "\\n".chars().collect(),
                '\r' => "\\r".chars().collect(),
                '\t' => "\\t".chars().collect(),
                value => vec![value],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        Date,
        OutputFormat,
        Value,
        format_json,
        format_output,
    };

    #[test]
    fn formats_text_and_json_outputs() {
        assert_eq!(
            format_output(&Value::from("alpha"), OutputFormat::Text),
            "alpha\n"
        );
        assert_eq!(
            format_output(
                &Value::List(vec![Value::from("alpha"), Value::from(1_i64)]),
                OutputFormat::Text
            ),
            "alpha\n1\n"
        );
        assert_eq!(
            format_output(
                &Value::Date(Date::new(2026, 6, 30).expect("valid date")),
                OutputFormat::Json
            ),
            "\"2026-06-30\"\n"
        );
    }

    #[test]
    fn formats_json_objects_with_escaping() {
        let value = Value::Object(BTreeMap::from([(
            "quote".to_owned(),
            Value::from("a \"quoted\" value"),
        )]));

        assert_eq!(
            format_json(&value),
            "{\"quote\":\"a \\\"quoted\\\" value\"}"
        );
    }
}
