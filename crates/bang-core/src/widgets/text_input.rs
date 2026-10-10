// SPDX-License-Identifier: EUPL-1.2

use screw::{
    RenderCtx,
    Role,
    Span,
    Spans,
    Surface,
};
use unicode_segmentation::{
    GraphemeCursor,
    UnicodeSegmentation as _,
};

use super::navigation::no_modifiers;
use crate::{
    Context,
    Event,
    Key,
    Reaction,
    Value,
    Widget,
    WidgetId,
};

type Validator = dyn Fn(&str) -> Result<(), String> + 'static;

/// A single line of editable text.
pub struct TextInput {
    id:          WidgetId,
    prompt:      Vec<Span>,
    placeholder: Option<String>,
    value:       String,
    cursor:      usize,
    error:       Option<String>,
    validator:   Option<Box<Validator>>,
    mask:        Option<char>,
}

impl TextInput {
    /// An empty input with no prompt.
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
            mask:        None,
        }
    }

    /// Show `prompt` before the text.
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.set_prompt(prompt);
        self
    }

    /// Show `prompt` before the text.
    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = vec![Span::new(prompt).role(Role::Prompt)];
    }

    /// Show styled `prompt` spans before the text.
    #[must_use]
    pub fn with_prompt_spans(mut self, prompt: impl Into<Vec<Span>>) -> Self {
        self.set_prompt_spans(prompt);
        self
    }

    /// Show styled `prompt` spans before the text.
    pub fn set_prompt_spans(&mut self, prompt: impl Into<Vec<Span>>) {
        self.prompt = prompt.into();
    }

    /// Show `placeholder` while the value is empty.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Start with `value` typed and the cursor after it.
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.len();
        self
    }

    /// Refuse to submit with the returned message until `validator` accepts the
    /// value.
    #[must_use]
    pub fn with_validator(
        mut self,
        validator: impl Fn(&str) -> Result<(), String> + 'static,
    ) -> Self {
        self.validator = Some(Box::new(validator));
        self
    }

    /// Render every grapheme cluster of the value as `mask`, leaving the value
    /// itself untouched.
    #[must_use]
    pub const fn with_mask(mut self, mask: char) -> Self {
        self.mask = Some(mask);
        self
    }

    /// The text typed so far.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor position as a byte offset into the value.
    #[must_use]
    pub const fn cursor_byte_index(&self) -> usize {
        self.cursor
    }

    /// The cursor position as a count of grapheme clusters from the start.
    #[must_use]
    pub fn cursor_grapheme_index(&self) -> usize {
        self.value[..self.cursor].graphemes(true).count()
    }

    fn insert_char(&mut self, value: char) -> Reaction {
        if value.is_control() {
            return Reaction::Ignored;
        }
        self.value.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.snap_cursor();
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
        self.snap_cursor();
        self.error = None;
        Reaction::Changed
    }

    fn snap_cursor(&mut self) {
        let on_boundary = GraphemeCursor::new(self.cursor, self.value.len(), true)
            .is_boundary(&self.value, 0)
            .unwrap_or(true);
        if !on_boundary {
            self.cursor = next_boundary(&self.value, self.cursor);
        }
    }

    fn backspace(&mut self) -> Reaction {
        if self.cursor == 0 {
            return Reaction::Ignored;
        }
        let previous = previous_boundary(&self.value, self.cursor);
        self.value.replace_range(previous..self.cursor, "");
        self.cursor = previous;
        self.snap_cursor();
        self.error = None;
        Reaction::Changed
    }

    fn delete(&mut self) -> Reaction {
        if self.cursor == self.value.len() {
            return Reaction::Ignored;
        }
        let next = next_boundary(&self.value, self.cursor);
        self.value.replace_range(self.cursor..next, "");
        self.snap_cursor();
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

    fn displayed_value(&self) -> String {
        self.mask.map_or_else(
            || self.value.clone(),
            |mask| self.value.graphemes(true).map(|_| mask).collect(),
        )
    }
}

impl screw::Widget for TextInput {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let theme = ctx.theme();
        Spans::new(self.prompt.iter().cloned()).render(ctx, out);
        let value = self.displayed_value();
        if value.is_empty() {
            out.set_cursor_here();
            if let Some(placeholder) = &self.placeholder {
                out.write(placeholder, theme.style(Role::Dim));
            }
        } else {
            let split = self.mask.map_or(self.cursor, |mask| {
                mask.len_utf8() * self.cursor_grapheme_index()
            });
            let style = theme.style(Role::Normal);
            out.write(&value[..split], style);
            out.set_cursor_here();
            out.write(&value[split..], style);
        }
        if let Some(error) = &self.error {
            out.newline();
            out.write(error, theme.style(Role::Error));
        }
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
            Event::Resize { .. } | Event::Tick | Event::UnknownEscape(_) => Reaction::Ignored,
        }
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::from(self.value.clone()))
    }
}

fn previous_boundary(value: &str, cursor: usize) -> usize {
    GraphemeCursor::new(cursor, value.len(), true)
        .prev_boundary(value, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
    GraphemeCursor::new(cursor, value.len(), true)
        .next_boundary(value, 0)
        .ok()
        .flatten()
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use screw::{
        Position,
        Theme,
    };

    use super::*;

    fn type_str(widget: &mut TextInput, text: &str) {
        let mut cx = Context::new();
        for value in text.chars() {
            widget.handle(Event::Key(crate::KeyEvent::new(Key::Char(value))), &mut cx);
        }
    }

    fn draw(widget: &TextInput) -> Surface {
        let mut surface = Surface::new();
        screw::Widget::render(widget, &RenderCtx::new(), &mut surface);
        surface
    }

    #[test]
    fn a_masked_input_shows_the_mask_and_keeps_the_real_value() {
        let mut widget = TextInput::new("secret").with_mask('*');
        type_str(&mut widget, "hünter2");

        let surface = draw(&widget);

        assert_eq!(surface.plain_text(), "*******");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 7 }));
        assert_eq!(widget.value(), "hünter2");
        assert_eq!(widget.current_value(), Some(Value::from("hünter2")));
    }

    #[test]
    fn the_cursor_counts_masked_characters_after_editing() {
        let mut widget = TextInput::new("secret").with_mask('•');
        type_str(&mut widget, "añb");
        let mut cx = Context::new();
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Left)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Backspace)), &mut cx);

        let surface = draw(&widget);

        assert_eq!(surface.plain_text(), "••");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 1 }));
        assert_eq!(widget.value(), "ab");
    }

    #[test]
    fn an_insertion_that_merges_clusters_leaves_the_cursor_on_a_boundary() {
        let mut cx = Context::new();
        let left = Event::Key(crate::KeyEvent::new(Key::Left));

        let mut widget = TextInput::new("t").with_value("\u{1f469}\u{1f4bb}");
        widget.handle(left.clone(), &mut cx);
        type_str(&mut widget, "\u{200d}");
        assert_eq!(widget.value(), "\u{1f469}\u{200d}\u{1f4bb}");
        assert_eq!(widget.cursor_byte_index(), widget.value().len());

        let mut widget = TextInput::new("t").with_value("\u{1f469}\u{1f4bb}");
        widget.handle(left.clone(), &mut cx);
        widget.handle(Event::Paste("\u{200d}".to_owned()), &mut cx);
        assert_eq!(widget.cursor_byte_index(), widget.value().len());

        let mut widget = TextInput::new("t").with_value("ex");
        widget.handle(left, &mut cx);
        type_str(&mut widget, "\u{301}");
        assert_eq!(widget.cursor_byte_index(), "e\u{301}".len());

        let mut widget = TextInput::new("t").with_value("\u{301}");
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Home)), &mut cx);
        type_str(&mut widget, "e");
        assert_eq!(widget.value(), "e\u{301}");
        assert_eq!(widget.cursor_byte_index(), widget.value().len());
    }

    #[test]
    fn a_deletion_that_merges_clusters_snaps_the_cursor_forward_past_the_merged_cluster() {
        let mut cx = Context::new();
        let flags = "\u{1f1e6}x\u{1f1e7}";
        let merged = "\u{1f1e6}\u{1f1e7}";

        let mut widget = TextInput::new("t").with_value(flags);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Left)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Backspace)), &mut cx);
        assert_eq!(widget.value(), merged);
        assert_eq!(widget.cursor_byte_index(), merged.len());

        let mut widget = TextInput::new("t").with_value(flags);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Home)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Right)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Delete)), &mut cx);
        assert_eq!(widget.value(), merged);
        assert_eq!(widget.cursor_byte_index(), merged.len());
    }

    #[test]
    fn editing_and_masking_work_on_grapheme_clusters() {
        let mut widget = TextInput::new("secret").with_mask('*');
        type_str(&mut widget, "ae\u{301}\u{1f468}\u{200d}\u{1f469}");
        assert_eq!(draw(&widget).plain_text(), "***");
        assert_eq!(widget.cursor_grapheme_index(), 3);

        let mut cx = Context::new();
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Left)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Backspace)), &mut cx);
        assert_eq!(widget.value(), "a\u{1f468}\u{200d}\u{1f469}");

        widget.handle(Event::Key(crate::KeyEvent::new(Key::Delete)), &mut cx);
        assert_eq!(widget.value(), "a");
        assert_eq!(draw(&widget).cursor(), Some(Position { row: 0, col: 1 }));
    }

    #[test]
    fn an_unmasked_input_shows_the_value() {
        let mut widget = TextInput::new("plain");
        type_str(&mut widget, "abc");

        assert_eq!(draw(&widget).plain_text(), "abc");
    }

    #[test]
    fn the_cursor_converts_scalars_to_display_columns() {
        let mut widget = TextInput::new("input")
            .with_prompt("> ")
            .with_value("a界z")
            .with_validator(|_value| Err("try again".to_owned()));
        let mut cx = Context::new();
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Left)), &mut cx);
        widget.handle(Event::Key(crate::KeyEvent::new(Key::Enter)), &mut cx);

        let surface = draw(&widget);

        assert_eq!(surface.plain_text(), "> a界z\ntry again");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 5 }));
        assert_eq!(
            surface.rows()[1].cells()[0].style(),
            Theme::default().style(Role::Error)
        );
    }

    #[test]
    fn an_empty_input_puts_the_cursor_before_its_placeholder() {
        let widget = TextInput::new("search")
            .with_prompt("search: ")
            .with_placeholder("type to filter");

        let surface = draw(&widget);

        assert_eq!(surface.plain_text(), "search: type to filter");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 8 }));
    }
}
