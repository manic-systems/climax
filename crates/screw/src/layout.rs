use crate::{InputAnchor, Line, LocalWidgetRef, Stack, WidgetRef, local_widget, widget};

#[derive(Clone)]
pub struct LayoutBuilder<H = WidgetRef> {
    rows: Vec<H>,
}

impl LayoutBuilder<WidgetRef> {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<WidgetRef>>) -> Self {
        self.rows.push(widget(Line::new(children)));
        self
    }

    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(widget(input));
        self
    }

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
    pub fn new_local() -> Self {
        Self { rows: Vec::new() }
    }

    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<LocalWidgetRef<'a>>>) -> Self {
        self.rows.push(local_widget(Line::new(children)));
        self
    }

    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(local_widget(input));
        self
    }

    pub fn into_widget(self) -> LocalWidgetRef<'a> {
        local_widget(self.build())
    }
}

impl<H> LayoutBuilder<H> {
    #[must_use]
    pub fn widget(mut self, child: H) -> Self {
        self.rows.push(child);
        self
    }

    pub fn build(self) -> Stack<H> {
        Stack::new(self.rows)
    }
}

pub fn layout() -> LayoutBuilder {
    LayoutBuilder::new()
}

/// Build a layout whose erased widgets stay on the current thread.
pub fn local_layout<'a>() -> LayoutBuilder<LocalWidgetRef<'a>> {
    LayoutBuilder::new_local()
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
}
