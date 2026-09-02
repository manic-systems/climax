//! render primitives

mod geometry;
mod layers;
mod layout;
mod plain;
mod renderer;
mod runtime;
mod style;
mod surface;
mod template;
mod terminal;
mod viewport;
mod widget;

pub use geometry::{Insets, Rect, Size, Viewport};
pub use layers::{Edge, Floating, Layers};
pub use layout::{LayoutBuilder, layout, local_layout};
pub use plain::{
    render_plain, render_plain_with_frame, render_plain_with_frame_and_theme, write_plain,
};
pub use renderer::{CursorVisibility, LayoutMode, RenderStats, Renderer};
pub use runtime::{
    AutoRuntime, AutoRuntimeBuilder, LiveRuntime, PlainRuntime, Runtime, RuntimeHandle,
};
pub use style::{Color, Role, Style, Theme};
pub use surface::{Cell, CursorMerge, Fill, Position, Row, RowBreak, Surface};
pub use template::{TemplateError, local_template, template};
pub use terminal::{FALLBACK_WIDTH, stderr_is_terminal, terminal_width, terminal_width_or_default};
pub use viewport::{VerticalViewport, ViewportReport, ViewportReportHandle};
pub use widget::{
    Grid, GridCell, InputAnchor, Line, List, LocalWidgetRef, Looping, ProgressBar, RenderCtx,
    SharedWidgetRef, Stack, Stateful, Text, TextInput, TickInterest, VerticalSize, Widget,
    WidgetRef, WindowedLines, local_widget, shared_widget, widget,
};

#[macro_export]
macro_rules! screw {
    ($template:literal $(, $name:ident = $widget:expr)* $(,)?) => {{
        $crate::template(
            $template,
            &[$((stringify!($name), $crate::widget($widget))),*],
        )
        .expect("invalid screw! template")
    }};
}

/// Compose a template from widgets which remain on the current thread.
#[macro_export]
macro_rules! local_screw {
    ($template:literal $(, $name:ident = $widget:expr)* $(,)?) => {{
        $crate::local_template(
            $template,
            &[$((stringify!($name), $crate::local_widget($widget))),*],
        )
        .expect("invalid local_screw! template")
    }};
}
