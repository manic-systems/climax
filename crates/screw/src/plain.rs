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

pub fn render_plain<T>(widget: &T) -> String
where
    T: Widget + ?Sized,
{
    render_plain_with_frame(widget, 0)
}

pub fn render_plain_with_frame<T>(widget: &T, frame: u64) -> String
where
    T: Widget + ?Sized,
{
    render_plain_with_frame_and_theme(widget, frame, Theme::default())
}

pub fn render_plain_with_frame_and_theme<T>(widget: &T, frame: u64, theme: Theme) -> String
where
    T: Widget + ?Sized,
{
    let mut surface = Surface::new();
    widget.render(
        &RenderCtx {
            frame,
            width: None,
            theme,
        },
        &mut surface,
    );
    surface.plain_text()
}

pub fn write_plain<T>(writer: &mut impl Write, widget: &T) -> io::Result<()>
where
    T: Widget + ?Sized,
{
    writer.write_all(render_plain(widget).as_bytes())
}
