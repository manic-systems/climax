use std::{error::Error, fmt};

use crate::{Line, LocalWidgetRef, Stack, Text, WidgetRef, local_widget, widget};

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

pub fn template(source: &str, slots: &[(&str, WidgetRef)]) -> Result<Stack, TemplateError> {
    template_with(source, slots, widget, widget)
}

/// Parse a template whose slots and resulting composition remain local to the
/// current thread.
pub fn local_template<'a>(
    source: &str,
    slots: &[(&str, LocalWidgetRef<'a>)],
) -> Result<Stack<LocalWidgetRef<'a>>, TemplateError> {
    template_with(source, slots, local_widget, local_widget)
}

fn template_with<H, T, L>(
    source: &str,
    slots: &[(&str, H)],
    wrap_text: T,
    wrap_line: L,
) -> Result<Stack<H>, TemplateError>
where
    H: Clone,
    T: Fn(Text) -> H,
    L: Fn(Line<H>) -> H,
{
    let mut rows = vec![Vec::new()];
    let mut text = String::new();
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                flush_text(&mut rows, &mut text, &wrap_text);
                rows.push(Vec::new());
            },
            '{' if chars.peek() == Some(&'{') => {
                let _ = chars.next();
                text.push('{');
            },
            '{' => {
                flush_text(&mut rows, &mut text, &wrap_text);
                let name = parse_slot_name(&mut chars)?;
                let slot = find_slot(slots, &name)?;
                current_row_mut(&mut rows).push(slot);
            },
            '}' if chars.peek() == Some(&'}') => {
                let _ = chars.next();
                text.push('}');
            },
            '}' => return Err(TemplateError::UnmatchedCloseBrace),
            _ => text.push(ch),
        }
    }

    flush_text(&mut rows, &mut text, &wrap_text);
    Ok(Stack::new(
        rows.into_iter()
            .map(|row| wrap_line(Line::new(row)))
            .collect::<Vec<_>>(),
    ))
}

fn parse_slot_name(
    chars: &mut std::iter::Peekable<impl Iterator<Item = char>>,
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

fn find_slot<H: Clone>(slots: &[(&str, H)], name: &str) -> Result<H, TemplateError> {
    slots
        .iter()
        .rev()
        .find_map(|(slot_name, slot)| (*slot_name == name).then(|| slot.clone()))
        .ok_or_else(|| TemplateError::MissingSlot(name.to_owned()))
}

fn flush_text<H>(rows: &mut [Vec<H>], text: &mut String, wrap: &impl Fn(Text) -> H) {
    if !text.is_empty() {
        current_row_mut(rows).push(wrap(Text::new(std::mem::take(text))));
    }
}

const fn current_row_mut<H>(rows: &mut [Vec<H>]) -> &mut Vec<H> {
    rows.last_mut()
        .expect("template parser always keeps a current row")
}

#[cfg(test)]
mod tests {
    use crate::{RenderCtx, Style, Surface, Widget, local_widget, render_plain};

    use super::local_template;

    struct BorrowedText<'a>(&'a str);

    impl Widget for BorrowedText<'_> {
        fn render(&self, _context: &RenderCtx, output: &mut Surface) {
            output.write(self.0, Style::PLAIN);
        }
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn local_template_retains_borrowed_slots() {
        let value = String::from("borrowed");
        let slot = local_widget(BorrowedText(&value));
        let rendered = local_template("{value}/{value}", &[("value", slot)]).unwrap();
        assert_eq!(render_plain(&rendered), "borrowed/borrowed");
    }
}
