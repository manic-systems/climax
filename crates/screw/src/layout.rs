use crate::{
    InputAnchor,
    Line,
    Stack,
    WidgetRef,
    widget,
};

#[derive(Clone, Default)]
pub struct LayoutBuilder {
    rows: Vec<WidgetRef>,
}

impl LayoutBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn line(mut self, children: impl Into<Vec<WidgetRef>>) -> Self {
        self.rows.push(widget(Line::new(children)));
        self
    }

    #[must_use]
    pub fn widget(mut self, child: WidgetRef) -> Self {
        self.rows.push(child);
        self
    }

    #[must_use]
    pub fn input(mut self, input: InputAnchor) -> Self {
        self.rows.push(widget(input));
        self
    }

    pub fn build(self) -> Stack {
        Stack::new(self.rows)
    }

    pub fn into_widget(self) -> WidgetRef {
        widget(self.build())
    }
}

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
        render_plain_with_frame,
    };

    fn render_cursor(widget: &impl crate::Widget) -> Option<crate::Position> {
        let mut surface = Surface::new();
        widget.render(
            &RenderCtx {
                frame: 0,
                width: None,
                theme: crate::Theme::default(),
            },
            &mut surface,
        );
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
