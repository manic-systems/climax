// SPDX-License-Identifier: EUPL-1.2

use std::fmt;

use crate::{InputAnchor, Line, LocalWidgetRef, Stack, WidgetRef, local_widget, widget};

/// Collects rows into a vertical [`Stack`].
///
/// Create one with [`layout`] for thread-safe widgets or [`local_layout`] for
/// widgets that stay on the current thread.
#[derive(Clone)]
pub struct LayoutBuilder<H = WidgetRef> {
    rows: Vec<H>,
}

impl<H> fmt::Debug for LayoutBuilder<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LayoutBuilder")
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl LayoutBuilder<WidgetRef> {
    /// Creates an empty builder for thread-safe widgets.
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// Adds a row whose children are laid out side by side.
    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<WidgetRef>>) -> Self {
        self.rows.push(widget(Line::new(children)));
        self
    }

    /// Adds a row that places the terminal cursor.
    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(widget(input));
        self
    }

    /// Builds the stack and erases it into a shared widget.
    pub fn into_widget(self) -> WidgetRef {
        widget(self.build())
    }
}

impl Default for LayoutBuilder<WidgetRef> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> LayoutBuilder<LocalWidgetRef<'a>> {
    /// Creates an empty builder for widgets that stay on the current thread.
    pub fn new_local() -> Self {
        Self { rows: Vec::new() }
    }

    /// Adds a row whose children are laid out side by side.
    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<LocalWidgetRef<'a>>>) -> Self {
        self.rows.push(local_widget(Line::new(children)));
        self
    }

    /// Adds a row that places the terminal cursor.
    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(local_widget(input));
        self
    }

    /// Builds the stack and erases it into a local widget.
    pub fn into_widget(self) -> LocalWidgetRef<'a> {
        local_widget(self.build())
    }
}

impl<H> LayoutBuilder<H> {
    /// Adds a widget as its own row.
    #[must_use]
    pub fn widget(mut self, child: H) -> Self {
        self.rows.push(child);
        self
    }

    /// Builds the rows into a [`Stack`].
    pub fn build(self) -> Stack<H> {
        Stack::new(self.rows)
    }
}

/// Starts a layout of thread-safe widgets.
pub fn layout() -> LayoutBuilder {
    LayoutBuilder::new()
}

/// Build a layout whose erased widgets stay on the current thread.
pub fn local_layout<'a>() -> LayoutBuilder<LocalWidgetRef<'a>> {
    LayoutBuilder::new_local()
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::{
        Looping,
        ProgressBar,
        RenderCtx,
        Style,
        Surface,
        Text,
        WindowedLines,
        render_plain_with_frame,
    };

    fn render_cursor(widget: &impl crate::Widget) -> Option<crate::Position> {
        let mut surface = Surface::new();
        widget.render(&RenderCtx::new(), &mut surface);
        surface.cursor()
    }

    #[test]
    fn builder_composes_lines_widgets_and_input_anchor() {
        let logs = WindowedLines::new(2);
        logs.push("one");
        logs.push("two");
        let progress = ProgressBar::new(4);
        progress.set_fraction(0.5);

        let app = layout()
            .line(vec![
                widget(Looping::new(["/", "-"]).style(Style::default())),
                widget(Text::new(" building")),
            ])
            .widget(widget(logs))
            .widget(widget(progress))
            .input(InputAnchor::prompt("> "))
            .build();

        assert_eq!(
            render_plain_with_frame(&app, 0),
            "/ building\none\ntwo\n[━━──]\n> "
        );
        assert_eq!(
            render_cursor(&app),
            Some(crate::Position { row: 4, col: 2 })
        );
    }

    struct LocalText(Rc<RefCell<String>>);

    impl crate::Widget for LocalText {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write(&*self.0.borrow(), Style::PLAIN);
        }
    }

    #[test]
    fn local_builder_composes_rc_refcell_widgets() {
        let value = Rc::new(RefCell::new("before".to_owned()));
        let app = local_layout()
            .line(vec![local_widget(LocalText(value.clone()))])
            .widget(local_widget("tail"))
            .build();

        assert_eq!(crate::render_plain(&app), "before\ntail");
        *value.borrow_mut() = "after".to_owned();
        assert_eq!(crate::render_plain(&app), "after\ntail");
    }
}
