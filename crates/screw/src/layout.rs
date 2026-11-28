// SPDX-License-Identifier: EUPL-1.2

use core::fmt;

use crate::{
    InputAnchor,
    Line,
    Stack,
    WidgetRef,
    widget,
};

/// collects rows into a vertical [`Stack`]
#[derive(Clone)]
pub struct LayoutBuilder<H = WidgetRef> {
    rows: Vec<H>,
}

impl<H> fmt::Debug for LayoutBuilder<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutBuilder")
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl LayoutBuilder<WidgetRef> {
    /// empty builder for thread-safe widgets
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    /// adds a row whose children are laid out side by side
    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<WidgetRef>>) -> Self {
        self.rows.push(widget(Line::new(children)));
        self
    }

    /// adds a row that places the terminal cursor
    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(widget(input));
        self
    }

    /// builds the stack and erases it into a shared widget
    pub fn into_widget(self) -> WidgetRef {
        widget(self.build())
    }
}

impl Default for LayoutBuilder<WidgetRef> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> LayoutBuilder<H> {
    /// adds a widget as its own row
    #[must_use]
    pub fn widget(mut self, child: H) -> Self {
        self.rows.push(child);
        self
    }

    /// builds the rows into a [`Stack`]
    pub fn build(self) -> Stack<H> {
        Stack::new(self.rows)
    }
}

/// starts a layout of thread-safe widgets
pub fn layout() -> LayoutBuilder {
    LayoutBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Looping,
        ProgressBar,
        RenderCtx,
        Style,
        Surface,
        Text,
        WindowedLines,
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
            crate::render_plain(&app),
            "/ building\none\ntwo\n[\u{2501}\u{2501}\u{2500}\u{2500}]\n> "
        );
        assert_eq!(
            render_cursor(&app),
            Some(crate::Position { row: 4, col: 2 })
        );
    }
}
