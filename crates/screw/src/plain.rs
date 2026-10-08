// SPDX-License-Identifier: EUPL-1.2

use std::io::{
    self,
    Write,
};

use crate::{
    RenderCtx,
    Surface,
    Theme,
    Widget,
};

/// Renders a widget to unstyled text at frame zero with the default theme.
///
/// Rows are joined by newlines and no control sequences are produced.
pub fn render_plain<T>(widget: &T) -> String
where
    T: Widget + ?Sized,
{
    render_plain_with_frame(widget, 0)
}

/// Like [`render_plain`] at a chosen frame, which animated widgets use to pick what to show.
pub fn render_plain_with_frame<T>(widget: &T, frame: u64) -> String
where
    T: Widget + ?Sized,
{
    render_plain_with_frame_and_theme(widget, frame, Theme::default())
}

/// Like [`render_plain_with_frame`] with an explicit theme for role styles.
pub fn render_plain_with_frame_and_theme<T>(widget: &T, frame: u64, theme: Theme) -> String
where
    T: Widget + ?Sized,
{
    let mut surface = Surface::new();
    widget.render(&RenderCtx::new().with_frame(frame).with_theme(theme), &mut surface);
    surface.plain_text()
}

/// Writes the [`render_plain`] output of a widget to `writer`.
pub fn write_plain<T>(writer: &mut impl Write, widget: &T) -> io::Result<()>
where
    T: Widget + ?Sized,
{
    writer.write_all(render_plain(widget).as_bytes())
}
