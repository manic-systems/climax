// SPDX-License-Identifier: EUPL-1.2

use core::fmt;

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

use super::navigation::{
    chorded,
    no_modifiers,
};
use crate::{
    Context,
    Event,
    Key,
    KeyEvent,
    Modifiers,
    Reaction,
    Value,
    Widget,
    WidgetId,
};

type Validator = dyn Fn(&str) -> Result<(), String> + 'static;

/// a single line of editable text
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

impl fmt::Debug for TextInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextInput")
            .field("id", &self.id)
            .field("prompt", &self.prompt)
            .field("placeholder", &self.placeholder)
            .field(
                "value",
                &self.mask.map_or(self.value.as_str(), |_| "<masked>"),
            )
            .field("cursor", &self.cursor)
            .field("error", &self.error)
            .field("mask", &self.mask)
            .finish_non_exhaustive()
    }
}

impl TextInput {
    /// an empty input with no prompt
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

    /// shows `prompt` before the text
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.set_prompt(prompt);
        self
    }

    /// shows `prompt` before the text
    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = vec![Span::new(prompt).role(Role::Prompt)];
    }

    /// shows styled `prompt` spans before the text
    #[must_use]
    pub fn with_prompt_spans(mut self, prompt: impl Into<Vec<Span>>) -> Self {
        self.set_prompt_spans(prompt);
        self
    }

    /// shows styled `prompt` spans before the text
    pub fn set_prompt_spans(&mut self, prompt: impl Into<Vec<Span>>) {
        self.prompt = prompt.into();
    }

    /// shows `placeholder` while the value is empty
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// starts with `value` typed and the cursor after it, control characters
    /// are dropped as they are when typing
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into().chars().filter(|ch| !ch.is_control()).collect();
        self.cursor = self.value.len();
        self
    }

    /// refuses to submit with the returned message until `validator` accepts
    /// the value
    #[must_use]
    pub fn with_validator(
        mut self,
        validator: impl Fn(&str) -> Result<(), String> + 'static,
    ) -> Self {
        self.validator = Some(Box::new(validator));
        self
    }

    /// renders every grapheme cluster of the value as `mask`, leaving the value
    /// itself untouched
    ///
    /// # Panics
    ///
    /// panics when `mask` is a control character or takes no columns
    #[must_use]
    pub fn with_mask(mut self, mask: char) -> Self {
        assert!(
            !mask.is_control() && screw::width(mask.encode_utf8(&mut [0; 4])) > 0,
            "text input mask {mask:?} must be a visible character"
        );
        self.mask = Some(mask);
        self
    }

    /// the text typed so far
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// the cursor position as a byte offset into the value
    #[must_use]
    pub const fn cursor_byte_index(&self) -> usize {
        self.cursor
    }

    /// the cursor position as a count of grapheme clusters from the start
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
        let cleaned: String = value.chars().filter(|ch| !ch.is_control()).collect();
        if cleaned.is_empty() {
            return Reaction::Ignored;
        }
        self.value.insert_str(self.cursor, &cleaned);
        self.cursor += cleaned.len();
        self.snap_cursor();
        self.error = None;
        Reaction::Changed
    }

    fn snap_cursor(&mut self) {
        self.cursor = cluster_end(&self.value, self.cursor);
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

    fn word_start(&self) -> usize {
        if self.mask.is_some() {
            return 0;
        }
        let mut segments = self.value[..self.cursor]
            .split_word_bound_indices()
            .rev()
            .peekable();
        while segments
            .next_if(|(_, text)| word_class(text) == WordClass::Space)
            .is_some()
        {}
        let Some((mut start, first)) = segments.next() else {
            return 0;
        };
        if word_class(first) == WordClass::Punctuation {
            while let Some((index, _)) =
                segments.next_if(|(_, text)| word_class(text) == WordClass::Punctuation)
            {
                start = index;
            }
        }
        cluster_start(&self.value, start)
    }

    fn word_end(&self) -> usize {
        if self.mask.is_some() {
            return self.value.len();
        }
        let rest = &self.value[self.cursor..];
        let mut segments = rest.split_word_bound_indices().peekable();
        while segments
            .next_if(|(_, text)| word_class(text) == WordClass::Space)
            .is_some()
        {}
        let Some((index, first)) = segments.next() else {
            return self.value.len();
        };
        let mut end = index + first.len();
        if word_class(first) == WordClass::Punctuation {
            while let Some((at, text)) =
                segments.next_if(|(_, text)| word_class(text) == WordClass::Punctuation)
            {
                end = at + text.len();
            }
        }
        cluster_end(&self.value, self.cursor + end)
    }

    fn move_word_left(&mut self) -> Reaction {
        let target = self.word_start();
        if target == self.cursor {
            return Reaction::Ignored;
        }
        self.cursor = target;
        Reaction::Changed
    }

    fn move_word_right(&mut self) -> Reaction {
        let target = self.word_end();
        if target == self.cursor {
            return Reaction::Ignored;
        }
        self.cursor = target;
        Reaction::Changed
    }

    fn delete_word_left(&mut self) -> Reaction {
        let start = self.word_start();
        if start == self.cursor {
            return Reaction::Ignored;
        }
        self.value.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.snap_cursor();
        self.error = None;
        Reaction::Changed
    }

    fn delete_word_right(&mut self) -> Reaction {
        let end = self.word_end();
        if end == self.cursor {
            return Reaction::Ignored;
        }
        self.value.replace_range(self.cursor..end, "");
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
                    Key::Char('w') if key.modifiers == Modifiers::CONTROL => {
                        self.delete_word_left()
                    },
                    Key::Char('d') if key.modifiers == Modifiers::ALT => self.delete_word_right(),
                    Key::Backspace if key.modifiers == Modifiers::ALT => self.delete_word_left(),
                    Key::Delete if key.modifiers == Modifiers::CONTROL => self.delete_word_right(),
                    Key::Left if by_word(&key) => self.move_word_left(),
                    Key::Right if by_word(&key) => self.move_word_right(),
                    Key::Left | Key::Right | Key::Enter | Key::Esc if chorded(&key) => {
                        Reaction::Ignored
                    },
                    Key::Backspace => self.backspace(),
                    Key::Delete => self.delete(),
                    Key::Left => self.move_left(),
                    Key::Right => self.move_right(),
                    Key::Home => self.move_home(),
                    Key::End => self.move_end(),
                    Key::Enter => self.submit(),
                    Key::Esc => Reaction::Cancel,
                    Key::Char(_)
                    | Key::Tab
                    | Key::Backtab
                    | Key::Up
                    | Key::Down
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Insert
                    | Key::Function(_) => Reaction::Ignored,
                }
            },
            Event::Paste(value) => self.insert_str(&value),
            Event::Resize { .. } | Event::Tick | Event::UnknownEscape(_) | Event::Row(_) => {
                Reaction::Ignored
            },
        }
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::from(self.value.clone()))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WordClass {
    Space,
    Word,
    Punctuation,
}

fn word_class(segment: &str) -> WordClass {
    if segment.chars().all(char::is_whitespace) {
        WordClass::Space
    } else if segment.chars().any(char::is_alphanumeric) {
        WordClass::Word
    } else {
        WordClass::Punctuation
    }
}

fn by_word(key: &KeyEvent) -> bool {
    key.modifiers == Modifiers::CONTROL || key.modifiers == Modifiers::ALT
}

fn cluster_start(value: &str, at: usize) -> usize {
    if is_boundary(value, at) {
        at
    } else {
        previous_boundary(value, at)
    }
}

fn cluster_end(value: &str, at: usize) -> usize {
    if is_boundary(value, at) {
        at
    } else {
        next_boundary(value, at)
    }
}

fn is_boundary(value: &str, at: usize) -> bool {
    GraphemeCursor::new(at, value.len(), true)
        .is_boundary(value, 0)
        .unwrap_or(true)
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

        let mut pasted = TextInput::new("t").with_value("\u{1f469}\u{1f4bb}");
        pasted.handle(left.clone(), &mut cx);
        pasted.handle(Event::Paste("\u{200d}".to_owned()), &mut cx);
        assert_eq!(pasted.cursor_byte_index(), pasted.value().len());

        let mut combining = TextInput::new("t").with_value("ex");
        combining.handle(left, &mut cx);
        type_str(&mut combining, "\u{301}");
        assert_eq!(combining.cursor_byte_index(), "e\u{301}".len());

        let mut leading = TextInput::new("t").with_value("\u{301}");
        leading.handle(Event::Key(crate::KeyEvent::new(Key::Home)), &mut cx);
        type_str(&mut leading, "e");
        assert_eq!(leading.value(), "e\u{301}");
        assert_eq!(leading.cursor_byte_index(), leading.value().len());
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

        let mut forward = TextInput::new("t").with_value(flags);
        forward.handle(Event::Key(crate::KeyEvent::new(Key::Home)), &mut cx);
        forward.handle(Event::Key(crate::KeyEvent::new(Key::Right)), &mut cx);
        forward.handle(Event::Key(crate::KeyEvent::new(Key::Delete)), &mut cx);
        assert_eq!(forward.value(), merged);
        assert_eq!(forward.cursor_byte_index(), merged.len());
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
    #[should_panic(expected = "must be a visible character")]
    fn a_newline_mask_is_refused() {
        drop(TextInput::new("secret").with_mask('\n'));
    }

    #[test]
    #[should_panic(expected = "must be a visible character")]
    fn a_zero_width_mask_is_refused() {
        drop(TextInput::new("secret").with_mask('\u{200b}'));
    }

    #[test]
    fn a_starting_value_loses_its_control_characters() {
        let widget = TextInput::new("name").with_value("a\nb\u{1b}[2Jc");
        assert_eq!(widget.value(), "ab[2Jc");
        assert_eq!(widget.cursor_byte_index(), widget.value().len());
    }

    fn chord(widget: &mut TextInput, key: Key, modifiers: Modifiers) -> Reaction {
        widget.handle(
            Event::Key(KeyEvent::with_modifiers(key, modifiers)),
            &mut Context::new(),
        )
    }

    #[test]
    fn control_and_alt_arrows_move_by_word() {
        for modifiers in [Modifiers::CONTROL, Modifiers::ALT] {
            let mut widget = TextInput::new("line").with_value("foo  bar, baz");
            chord(&mut widget, Key::Left, modifiers);
            assert_eq!(widget.cursor_byte_index(), 10);
            chord(&mut widget, Key::Left, modifiers);
            assert_eq!(widget.cursor_byte_index(), 8);
            chord(&mut widget, Key::Left, modifiers);
            assert_eq!(widget.cursor_byte_index(), 5);
            chord(&mut widget, Key::Left, modifiers);
            chord(&mut widget, Key::Left, modifiers);
            assert_eq!(widget.cursor_byte_index(), 0);
            assert_eq!(chord(&mut widget, Key::Left, modifiers), Reaction::Ignored);

            chord(&mut widget, Key::Right, modifiers);
            assert_eq!(widget.cursor_byte_index(), 3);
            chord(&mut widget, Key::Right, modifiers);
            assert_eq!(widget.cursor_byte_index(), 8);
            chord(&mut widget, Key::Right, modifiers);
            assert_eq!(widget.cursor_byte_index(), 9);
            chord(&mut widget, Key::Right, modifiers);
            chord(&mut widget, Key::Right, modifiers);
            assert_eq!(widget.cursor_byte_index(), widget.value().len());
            assert_eq!(chord(&mut widget, Key::Right, modifiers), Reaction::Ignored);
        }
    }

    #[test]
    fn word_deletion_works_in_both_directions() {
        let mut widget = TextInput::new("line").with_value("foo bar baz");
        chord(&mut widget, Key::Char('w'), Modifiers::CONTROL);
        assert_eq!(widget.value(), "foo bar ");
        chord(&mut widget, Key::Backspace, Modifiers::ALT);
        assert_eq!(widget.value(), "foo ");

        chord(&mut widget, Key::Home, Modifiers::empty());
        chord(&mut widget, Key::Char('d'), Modifiers::ALT);
        assert_eq!(widget.value(), " ");
        let mut forward = TextInput::new("line").with_value("foo bar");
        chord(&mut forward, Key::Home, Modifiers::empty());
        chord(&mut forward, Key::Delete, Modifiers::CONTROL);
        assert_eq!(forward.value(), " bar");
        assert_eq!(forward.cursor_byte_index(), 0);
    }

    #[test]
    fn a_word_never_splits_a_grapheme_cluster() {
        let mut widget = TextInput::new("line").with_value("cafe\u{301} bar");
        chord(&mut widget, Key::Left, Modifiers::CONTROL);
        chord(&mut widget, Key::Left, Modifiers::CONTROL);
        assert_eq!(widget.cursor_byte_index(), 0);
        chord(&mut widget, Key::Right, Modifiers::CONTROL);
        assert_eq!(widget.cursor_byte_index(), "cafe\u{301}".len());
        chord(&mut widget, Key::Char('w'), Modifiers::CONTROL);
        assert_eq!(widget.value(), " bar");

        for text in ["ทำ", "ກຳ"] {
            assert_eq!(text.graphemes(true).count(), 1);
            let mut sara_am = TextInput::new("line").with_value(text);
            chord(&mut sara_am, Key::Left, Modifiers::CONTROL);
            assert_eq!(sara_am.cursor_byte_index(), 0);
            chord(&mut sara_am, Key::Right, Modifiers::CONTROL);
            assert_eq!(sara_am.cursor_byte_index(), text.len());
            chord(&mut sara_am, Key::Backspace, Modifiers::ALT);
            assert_eq!(sara_am.value(), "");
        }
    }

    #[test]
    fn wide_characters_and_punctuation_runs_move_like_words() {
        let mut widget = TextInput::new("line").with_value("日本 x...y");
        chord(&mut widget, Key::Left, Modifiers::ALT);
        assert_eq!(widget.cursor_byte_index(), "日本 x...".len());
        chord(&mut widget, Key::Left, Modifiers::ALT);
        assert_eq!(widget.cursor_byte_index(), "日本 x".len());
        chord(&mut widget, Key::Left, Modifiers::ALT);
        chord(&mut widget, Key::Left, Modifiers::ALT);
        assert_eq!(widget.cursor_byte_index(), "日".len());
    }

    #[test]
    fn a_masked_value_is_one_word() {
        let mut widget = TextInput::new("secret")
            .with_mask('*')
            .with_value("two words");
        chord(&mut widget, Key::Left, Modifiers::CONTROL);
        assert_eq!(widget.cursor_byte_index(), 0);
        chord(&mut widget, Key::Right, Modifiers::CONTROL);
        assert_eq!(widget.cursor_byte_index(), widget.value().len());
        chord(&mut widget, Key::Char('w'), Modifiers::CONTROL);
        assert_eq!(widget.value(), "");
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
