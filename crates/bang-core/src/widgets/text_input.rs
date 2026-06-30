// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Context,
    CursorAnchor,
    Event,
    Key,
    KeyEvent,
    Reaction,
    Role,
    Span,
    TextInputView,
    Value,
    View,
    ViewContext,
    ViewId,
    Widget,
    WidgetId,
};

type Validator = dyn Fn(&str) -> Result<(), String> + Send + Sync + 'static;

pub struct TextInput {
    id:          WidgetId,
    prompt:      Vec<Span>,
    placeholder: Option<String>,
    value:       String,
    cursor:      usize,
    error:       Option<String>,
    validator:   Option<Box<Validator>>,
}

impl TextInput {
    #[must_use]
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id:          id.into(),
            prompt:      Vec::new(),
            placeholder: None,
            value:       String::new(),
            cursor:      0,
            error:       None,
            validator:   None,
        }
    }

    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.set_prompt(prompt);
        self
    }

    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = vec![Span::new(prompt, Role::Prompt)];
    }

    #[must_use]
    pub fn with_prompt_spans(mut self, prompt: impl Into<Vec<Span>>) -> Self {
        self.set_prompt_spans(prompt);
        self
    }

    pub fn set_prompt_spans(&mut self, prompt: impl Into<Vec<Span>>) {
        self.prompt = prompt.into();
    }

    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.len();
        self
    }

    #[must_use]
    pub fn with_validator(
        mut self,
        validator: impl Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.validator = Some(Box::new(validator));
        self
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn cursor_byte_index(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn cursor_char_index(&self) -> usize {
        char_count(&self.value[..self.cursor])
    }

    fn insert_char(&mut self, value: char) -> Reaction {
        if value.is_control() {
            return Reaction::Ignored;
        }
        self.value.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.error = None;
        Reaction::Changed
    }

    fn insert_str(&mut self, value: &str) -> Reaction {
        let value: String = value.chars().filter(|value| !value.is_control()).collect();
        if value.is_empty() {
            return Reaction::Ignored;
        }
        self.value.insert_str(self.cursor, &value);
        self.cursor += value.len();
        self.error = None;
        Reaction::Changed
    }

    fn backspace(&mut self) -> Reaction {
        if self.cursor == 0 {
            return Reaction::Ignored;
        }
        let previous = previous_boundary(&self.value, self.cursor);
        self.value.replace_range(previous..self.cursor, "");
        self.cursor = previous;
        self.error = None;
        Reaction::Changed
    }

    fn delete(&mut self) -> Reaction {
        if self.cursor == self.value.len() {
            return Reaction::Ignored;
        }
        let next = next_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..next, "");
        self.error = None;
        Reaction::Changed
    }

    fn move_left(&mut self) -> Reaction {
        if self.cursor == 0 {
            return Reaction::Ignored;
        }
        self.cursor = previous_boundary(&self.value, self.cursor);
        Reaction::Changed
    }

    fn move_right(&mut self) -> Reaction {
        if self.cursor == self.value.len() {
            return Reaction::Ignored;
        }
        self.cursor = next_boundary(&self.value, self.cursor);
        Reaction::Changed
    }

    const fn move_home(&mut self) -> Reaction {
        if self.cursor == 0 {
            return Reaction::Ignored;
        }
        self.cursor = 0;
        Reaction::Changed
    }

    const fn move_end(&mut self) -> Reaction {
        if self.cursor == self.value.len() {
            return Reaction::Ignored;
        }
        self.cursor = self.value.len();
        Reaction::Changed
    }

    fn submit(&mut self) -> Reaction {
        if let Some(validator) = &self.validator {
            match validator(&self.value) {
                Ok(()) => {
                    self.error = None;
                },
                Err(error) => {
                    self.error = Some(error);
                    return Reaction::Changed;
                },
            }
        }
        Reaction::Submit(Value::from(self.value.clone()))
    }

    fn cursor_anchor(&self) -> CursorAnchor {
        CursorAnchor::owned(format!("{}/cursor", self.id.as_str()))
    }
}

impl Widget for TextInput {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
        match event {
            Event::Key(key) => {
                match key.key {
                    Key::Char(value) if no_modifiers(&key) => self.insert_char(value),
                    Key::Backspace => self.backspace(),
                    Key::Delete => self.delete(),
                    Key::Left => self.move_left(),
                    Key::Right => self.move_right(),
                    Key::Home => self.move_home(),
                    Key::End => self.move_end(),
                    Key::Enter => self.submit(),
                    Key::Esc => Reaction::Cancel,
                    _ => Reaction::Ignored,
                }
            },
            Event::Paste(value) => self.insert_str(&value),
            Event::Resize { .. } | Event::Tick => Reaction::Ignored,
        }
    }

    fn view(&self, _cx: &ViewContext) -> View {
        View::TextInput(TextInputView {
            id:            Some(ViewId::owned(format!("{}/input", self.id.as_str()))),
            prompt:        self.prompt.clone(),
            value:         self.value.clone(),
            placeholder:   self.placeholder.clone(),
            cursor:        self.cursor_char_index(),
            cursor_anchor: self.cursor_anchor(),
            error:         self.error.clone(),
        })
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::from(self.value.clone()))
    }
}

fn previous_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _value)| index)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    value[cursor..]
        .char_indices()
        .nth(1)
        .map_or(value.len(), |(index, _value)| cursor + index)
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

const fn no_modifiers(key: &KeyEvent) -> bool {
    key.modifiers.bits() == 0
}
