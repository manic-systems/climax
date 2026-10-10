// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use screw::{
    LocalWidgetRef, RenderCtx, Role, Span, Spans, Stack, Surface, VerticalSize, local_widget,
};

use super::navigation::{
    self, ListFrame, ListRow, PageLayout, move_index, only_control, page_move,
};
use super::{
    SelectItem,
    TextInput,
};
use crate::{Context, Event, Key, Reaction, Widget, WidgetId};

/// A single-choice list filtered by a query typed above it.
///
/// Up, Down, Ctrl-P and Ctrl-N move the selection and Ctrl-Home and Ctrl-End
/// jump to the first and last match. Plain Home and End move the query cursor.
pub struct SearchSelect {
    id:        WidgetId,
    input:     TextInput,
    header:    Vec<Span>,
    items:     Vec<SelectItem>,
    matches:   Vec<usize>,
    selected:  usize,
    top:       usize,
    page_size: usize,
    layout:    PageLayout,
    wrap:      bool,
}

impl SearchSelect {
    /// A list over `items`, unfiltered until the user types.
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let id = id.into();
        let items: Vec<_> = items.into_iter().map(Into::into).collect();
        let matches = (0..items.len()).collect();
        Self {
            input: TextInput::new(WidgetId::owned(format!("{}/query", id.as_str())))
                .with_prompt("search: "),
            header: Vec::new(),
            id,
            items,
            matches,
            selected: 0,
            top: 0,
            page_size: navigation::DEFAULT_PAGE_SIZE,
            layout: PageLayout::default(),
            wrap: true,
        }
    }

    /// Show `prompt` before the query.
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.input.set_prompt(prompt);
        self
    }

    /// Show `placeholder` while the query is empty.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.input = self.input.with_placeholder(placeholder);
        self
    }

    /// Show `header` above the query.
    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.set_header(header);
        self
    }

    /// Show `header` above the query.
    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = vec![Span::new(header).role(Role::Prompt)];
    }

    /// Show styled `header` spans above the query.
    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.set_header_spans(header);
        self
    }

    /// Show styled `header` spans above the query.
    pub fn set_header_spans(&mut self, header: impl Into<Vec<Span>>) {
        self.header = header.into();
    }

    /// Show at most `page_size` matches at once.
    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self.ensure_visible();
        self
    }

    /// Choose whether moving past either end wraps around. Defaults to `true`.
    #[must_use]
    pub const fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Start with the match at `selected`, clamped to the last match.
    #[must_use]
    pub fn with_selected_match_index(mut self, selected: usize) -> Self {
        if !self.matches.is_empty() {
            self.selected = selected.min(self.matches.len() - 1);
            self.ensure_visible();
        }
        self
    }

    /// The text typed so far.
    #[must_use]
    pub fn query(&self) -> &str {
        self.input.value()
    }

    /// The indices of the items that match the query, in list order.
    #[must_use]
    pub fn matched_indices(&self) -> &[usize] {
        &self.matches
    }

    /// The position of the selected item among the matches, or `None` when nothing matches.
    #[must_use]
    pub const fn selected_match_index(&self) -> Option<usize> {
        if self.matches.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// The selected item, or `None` when nothing matches.
    #[must_use]
    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.selected_match_index()
            .map(|selected| &self.items[self.matches[selected]])
    }

    fn handle_input(&mut self, event: Event, cx: &mut Context) -> Reaction {
        match self.input.handle(event, cx) {
            Reaction::Changed => {
                self.recompute_matches();
                Reaction::Changed
            },
            Reaction::Ignored
            | Reaction::Cancel
            | Reaction::Submit(_)
            | Reaction::Action(_)
            | Reaction::Focus(_) => Reaction::Ignored,
        }
    }

    fn recompute_matches(&mut self) {
        let query = self.query();
        self.matches = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| matches_query(&item.label, query).then_some(index))
            .collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
        self.ensure_visible();
    }

    fn move_by(&mut self, delta: isize, wrap: bool) -> Reaction {
        let Some(next) = move_index(self.selected, self.matches.len(), delta, wrap) else {
            return Reaction::Ignored;
        };
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        self.ensure_visible();
        Reaction::Changed
    }

    fn move_to(&mut self, selected: usize) -> Reaction {
        navigation::move_to(&mut self.selected, &mut self.top, selected, self.matches.len(), self.page_size)
    }

    fn move_page(&mut self, target: usize) -> Reaction {
        page_move(&mut self.top, &mut self.selected, target, self.matches.len())
    }

    fn submit(&self) -> Reaction {
        self.selected_item().map_or(Reaction::Ignored, |item| {
            Reaction::Submit(item.value.clone())
        })
    }

    fn ensure_visible(&mut self) {
        navigation::ensure_visible(&mut self.selected, &mut self.top, self.matches.len(), self.page_size);
    }

    fn visible_len(&self) -> usize {
        navigation::visible_len(self.matches.len(), self.page_size)
    }

    fn render_results(&self, ctx: &RenderCtx, out: &mut Surface) {
        let window_size = self.page_size;
        let window = navigation::window_range(self.top, window_size, self.matches.len());
        let rows = self.matches[window.clone()]
            .iter()
            .enumerate()
            .map(|(offset, item_index)| {
                let match_index = window.start + offset;
                let item = &self.items[*item_index];
                let selected = Some(match_index) == self.selected_match_index();
                ListRow {
                    spans: highlight_match(&item.label, self.query(), selected),
                    selected,
                    checked: None,
                }
            })
            .collect();

        navigation::render_list(
            ListFrame {
                header: &[],
                rows,
                window_offset: window.start,
                selected: self.selected_match_index(),
                top: self.top,
                max_visible: self.page_size,
                help: "type filter | enter submit | esc cancel",
            },
            &self.layout,
            ctx,
            out,
        );
    }
}

struct Results<'a>(&'a SearchSelect);

impl screw::Widget for Results<'_> {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.0.render_results(ctx, out);
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

impl screw::Widget for SearchSelect {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let mut children: Vec<LocalWidgetRef<'_>> = Vec::new();
        if !self.header.is_empty() {
            children.push(local_widget(Spans::new(self.header.iter().cloned())));
        }
        children.push(local_widget(&self.input));
        children.push(local_widget(Results(self)));
        Stack::new(children).render(ctx, out);
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

impl Widget for SearchSelect {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        self.layout.sync_top(&mut self.top);
        match &event {
            Event::Key(key) => match key.key {
                Key::Up => self.move_by(-1, self.wrap),
                Key::Down => self.move_by(1, self.wrap),
                Key::Char('p') if only_control(key) => self.move_by(-1, self.wrap),
                Key::Char('n') if only_control(key) => self.move_by(1, self.wrap),
                Key::PageUp => {
                    match navigation::page_action(self.layout.target(false), self.visible_len(), false) {
                        navigation::PageAction::JumpTo(target) => self.move_page(target),
                        navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                    }
                },
                Key::PageDown => {
                    match navigation::page_action(self.layout.target(true), self.visible_len(), true) {
                        navigation::PageAction::JumpTo(target) => self.move_page(target),
                        navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                    }
                },
                Key::Home if only_control(key) => self.move_to(0),
                Key::End if only_control(key) => self.move_to(self.matches.len().saturating_sub(1)),
                Key::Enter => self.submit(),
                Key::Esc => Reaction::Cancel,
                _ => self.handle_input(event, cx),
            },
            Event::Paste(_) => self.handle_input(event, cx),
            Event::Resize { .. } | Event::Tick | Event::UnknownEscape(_) => Reaction::Ignored,
        }
    }

    fn current_value(&self) -> Option<crate::Value> {
        self.selected_item().map(|item| item.value.clone())
    }
}

fn matches_query(label: &str, query: &str) -> bool {
    query.is_empty() || find_match(label, query).is_some()
}

/// The byte range of the first case-insensitive occurrence of `query` in
/// `label`, compared by Unicode lowercase and measured in `label` itself.
fn find_match(label: &str, query: &str) -> Option<Range<usize>> {
    let wanted: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    label.char_indices().find_map(|(start, _)| {
        let mut matched = 0;
        for (offset, character) in label[start..].char_indices() {
            for lower in character.to_lowercase() {
                if wanted.get(matched) != Some(&lower) {
                    return None;
                }
                matched += 1;
            }
            if matched == wanted.len() {
                return Some(start..start + offset + character.len_utf8());
            }
        }
        None
    })
}

fn highlight_match(label: &str, query: &str, selected: bool) -> Vec<(String, Role)> {
    let base_role = if selected {
        Role::Selected
    } else {
        Role::Normal
    };
    if query.is_empty() {
        return vec![(label.to_owned(), base_role)];
    }

    let Some(Range { start, end }) = find_match(label, query) else {
        return vec![(label.to_owned(), base_role)];
    };

    let mut spans = Vec::new();
    if start > 0 {
        spans.push((label[..start].to_owned(), base_role));
    }
    spans.push((label[start..end].to_owned(), Role::Match));
    if end < label.len() {
        spans.push((label[end..].to_owned(), base_role));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KeyEvent, Modifiers};

    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::with_modifiers(Key::Char(c), Modifiers::CONTROL))
    }

    #[test]
    fn the_header_renders_above_the_query() {
        let select = SearchSelect::new("s", ["alpha", "beta"]).with_header("pick one");

        let plain = screw::render_plain(&select);
        let mut lines = plain.lines();

        assert_eq!(lines.next(), Some("pick one"));
        assert!(lines.next().unwrap().starts_with("search: "), "{plain}");
        assert!(lines.next().unwrap().contains("alpha"), "{plain}");
    }

    #[test]
    fn matching_ignores_case_beyond_ascii() {
        assert!(matches_query("Étage", "é"));
        assert!(matches_query("école", "É"));
        assert_eq!(find_match("Étage", "TAG"), Some(2..5));
        assert_eq!(find_match("Étage", "zzz"), None);
    }

    fn ctrl_key(key: Key) -> Event {
        Event::Key(KeyEvent::with_modifiers(key, Modifiers::CONTROL))
    }

    #[test]
    fn ctrl_home_and_ctrl_end_jump_while_plain_home_moves_the_query_cursor() {
        let mut select = SearchSelect::new("s", ["alpha", "beta", "gamma"]).with_selected_match_index(1);
        let mut cx = Context::new();

        select.handle(Event::char('a'), &mut cx);
        assert_eq!(select.handle(ctrl_key(Key::End), &mut cx), Reaction::Changed);
        assert_eq!(select.selected_match_index(), Some(2));
        assert_eq!(select.handle(ctrl_key(Key::Home), &mut cx), Reaction::Changed);
        assert_eq!(select.selected_match_index(), Some(0));

        select.handle(Event::Key(KeyEvent::new(Key::Home)), &mut cx);
        select.handle(Event::char('x'), &mut cx);
        assert_eq!(select.query(), "xa");
    }

    #[test]
    fn ctrl_n_and_ctrl_p_navigate_while_j_and_k_type_into_the_query() {
        let mut select = SearchSelect::new("s", ["alpha", "beta", "gamma"]);
        let mut cx = Context::new();

        assert_eq!(select.handle(ctrl('n'), &mut cx), Reaction::Changed);
        assert_eq!(select.selected_match_index(), Some(1));

        assert_eq!(select.handle(ctrl('p'), &mut cx), Reaction::Changed);
        assert_eq!(select.selected_match_index(), Some(0));

        select.handle(Event::char('j'), &mut cx);
        select.handle(Event::char('k'), &mut cx);
        assert_eq!(select.query(), "jk");
        assert_eq!(select.selected_match_index(), None);
    }
}
