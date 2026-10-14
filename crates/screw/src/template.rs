use alloc::sync::Arc;
use core::{
    error::Error,
    fmt,
    iter::Peekable,
    mem,
};

use crate::{
    Line,
    Stack,
    Text,
    WidgetRef,
    widget,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TemplateError {
    EmptySlotName,
    MissingSlot(String),
    UnclosedSlot(String),
    UnmatchedCloseBrace,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySlotName => f.write_str("template slot name cannot be empty"),
            Self::MissingSlot(name) => write!(f, "template references unknown slot `{name}`"),
            Self::UnclosedSlot(name) => {
                write!(f, "template slot {name} is missing a closing brace")
            },
            Self::UnmatchedCloseBrace => {
                f.write_str("template contains an unmatched closing brace")
            },
        }
    }
}

impl Error for TemplateError {}

/// # Errors
///
/// fails when `source` has an unmatched `}`, an empty or unclosed slot, or
/// names a slot missing from `slots`.
pub fn template(source: &str, slots: &[(&str, WidgetRef)]) -> Result<Stack, TemplateError> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut text = String::new();
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                flush_text(&mut row, &mut text);
                rows.push(mem::take(&mut row));
            },
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            },
            '{' => {
                flush_text(&mut row, &mut text);
                let name = parse_slot_name(&mut chars)?;
                let slot = find_slot(slots, &name)?;
                row.push(slot);
            },
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            },
            '}' => return Err(TemplateError::UnmatchedCloseBrace),
            _ => text.push(ch),
        }
    }

    flush_text(&mut row, &mut text);
    rows.push(row);
    Ok(Stack::new(
        rows.into_iter()
            .map(|cells| widget(Line::new(cells)))
            .collect::<Vec<_>>(),
    ))
}

fn parse_slot_name(
    chars: &mut Peekable<impl Iterator<Item = char>>,
) -> Result<String, TemplateError> {
    let mut name = String::new();
    for ch in chars.by_ref() {
        if ch == '}' {
            if name.is_empty() {
                return Err(TemplateError::EmptySlotName);
            }
            return Ok(name);
        }
        name.push(ch);
    }
    Err(TemplateError::UnclosedSlot(name))
}

fn find_slot(slots: &[(&str, WidgetRef)], name: &str) -> Result<WidgetRef, TemplateError> {
    slots
        .iter()
        .rev()
        .find_map(|(slot_name, slot)| (*slot_name == name).then(|| Arc::clone(slot)))
        .ok_or_else(|| TemplateError::MissingSlot(name.to_owned()))
}

fn flush_text(row: &mut Vec<WidgetRef>, text: &mut String) {
    if !text.is_empty() {
        row.push(widget(Text::new(mem::take(text))));
    }
}
