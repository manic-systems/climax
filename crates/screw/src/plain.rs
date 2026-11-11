// SPDX-License-Identifier: EUPL-1.2

use crate::{
    RenderCtx,
    Surface,
    Widget,
};

/// renders a widget to unstyled text at frame zero with the default theme
pub fn render_plain<T>(widget: &T) -> String
where
    T: Widget + ?Sized,
{
    render_plain_with(widget, &RenderCtx::new())
}

/// [`render_plain`] with an explicit context, for the frame, theme and size
pub fn render_plain_with<T>(widget: &T, ctx: &RenderCtx) -> String
where
    T: Widget + ?Sized,
{
    let mut surface = Surface::new();
    widget.render(ctx, &mut surface);
    surface.plain_text()
}
