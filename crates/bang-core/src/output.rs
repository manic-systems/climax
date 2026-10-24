// SPDX-License-Identifier: EUPL-1.2

//! text prints non-finite floats as `NaN`, `inf` and `-inf`, json writes them
//! as the strings `"NaN"`, `"Infinity"` and `"-Infinity"` so the document stays
//! valid json

use crate::{
    Number,
    Value,
};

/// how a submitted value is written out
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
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

/// renders `value` as plain text, a list becomes one line per item and an empty
/// list prints nothing, structures fall back to JSON
#[must_use]
pub fn format_text(value: &Value) -> String {
    match value {
        Value::Null => "\n".to_owned(),
        Value::Bool(flag) => format!("{flag}\n"),
        Value::String(text) => format!("{text}\n"),
        Value::Number(number) => format!("{number}\n"),
        Value::Date(date) => format!("{date}\n"),
        Value::List(values) if values.is_empty() => String::new(),
        Value::List(values) => {
            let mut lines = values
                .iter()
                .map(|item| {
                    match item {
                        Value::String(text) => text.clone(),
                        Value::Number(number) => number.to_string(),
                        Value::Null
                        | Value::Bool(_)
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

/// renders `value` as JSON, non-finite floats become the strings `"NaN"`,
/// `"Infinity"` and `"-Infinity"` and dates become strings
#[must_use]
pub fn format_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::String(text) => format!("\"{}\"", escape_json(text)),
        Value::Number(number) => {
            match number {
                Number::Integer(integer) => integer.to_string(),
                Number::Float(float) if float.is_finite() => number.to_string(),
                Number::Float(float) if float.is_nan() => "\"NaN\"".to_owned(),
                Number::Float(float) if *float > 0.0 => "\"Infinity\"".to_owned(),
                Number::Float(_) => "\"-Infinity\"".to_owned(),
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

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;
    use crate::Date;

    #[test]
    fn json_escapes_control_characters() {
        assert_eq!(
            format_json(&Value::from("a\"b\\c\n\t\u{1}\u{7f}")),
            "\"a\\\"b\\\\c\\n\\t\\u0001\\u007f\""
        );
        assert_eq!(escape_json("\u{8}\u{c}\r"), "\\b\\f\\r");
    }

    #[test]
    fn json_writes_non_finite_floats_as_strings() {
        for (float, text) in [
            (f64::NAN, "\"NaN\""),
            (f64::INFINITY, "\"Infinity\""),
            (f64::NEG_INFINITY, "\"-Infinity\""),
        ] {
            assert_eq!(format_json(&Value::from(float)), text);
        }
        let list = Value::List(vec![Value::from(f64::NAN), Value::from(1.5)]);
        assert_eq!(format_json(&list), "[\"NaN\",1.5]");
        assert_eq!(format_json(&Value::from(1.0)), "1.0");
        assert_eq!(format_json(&Value::from(i64::MIN)), i64::MIN.to_string());
    }

    #[test]
    fn json_nests_lists_objects_and_dates() {
        let value = Value::Object(BTreeMap::from([
            (
                "when".to_owned(),
                Value::Date(Date::new(2024, 2, 29).unwrap()),
            ),
            (
                "items\n".to_owned(),
                Value::List(vec![Value::Null, Value::from(true), Value::from(2)]),
            ),
        ]));
        assert_eq!(
            format_json(&value),
            "{\"items\\n\":[null,true,2],\"when\":\"2024-02-29\"}"
        );
    }

    #[test]
    fn text_prints_lists_one_item_per_line() {
        let list = Value::List(vec![Value::from("a b"), Value::from(2), Value::Null]);
        assert_eq!(format_text(&list), "a b\n2\nnull\n");
        assert_eq!(format_text(&Value::from("plain")), "plain\n");
        assert_eq!(format_text(&Value::Null), "\n");
        assert_eq!(format_text(&Value::from(1.5)), "1.5\n");
    }

    #[test]
    fn an_empty_list_prints_nothing_unlike_a_list_of_one_empty_string() {
        assert_eq!(format_text(&Value::List(Vec::new())), "");
        assert_eq!(format_text(&Value::List(vec![Value::from("")])), "\n");
        assert_eq!(
            format_output(&Value::List(Vec::new()), OutputFormat::Text),
            ""
        );
    }

    #[test]
    fn text_prints_non_finite_floats_plainly() {
        for (float, text) in [
            (f64::NAN, "NaN\n"),
            (f64::INFINITY, "inf\n"),
            (f64::NEG_INFINITY, "-inf\n"),
        ] {
            assert_eq!(format_text(&Value::from(float)), text);
        }
        let list = Value::List(vec![Value::from(f64::NAN), Value::from(f64::NEG_INFINITY)]);
        assert_eq!(format_text(&list), "NaN\n-inf\n");
    }

    #[test]
    fn the_format_decides_the_framing() {
        let value = Value::from("x");
        assert_eq!(format_output(&value, OutputFormat::Text), "x\n");
        assert_eq!(format_output(&value, OutputFormat::Json), "\"x\"\n");
    }
}
