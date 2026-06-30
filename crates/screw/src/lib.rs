//! render primitives
//!
//! widgets render cells into a `Surface`,
//! `Renderer` diffs the previous surface against the new one
//! and emits necessary sequences for redraw

mod layout;
mod plain;
mod renderer;
mod runtime;
mod style;
mod surface;
mod template;
mod terminal;
mod widget;

pub use layout::{
    LayoutBuilder,
    layout,
};
pub use plain::{
    render_plain,
    render_plain_with_frame,
    render_plain_with_frame_and_theme,
    write_plain,
};
pub use renderer::{
    LayoutMode,
    RenderStats,
    Renderer,
};
pub use runtime::{
    AutoRuntime,
    AutoRuntimeBuilder,
    LiveRuntime,
    PlainRuntime,
    Runtime,
    RuntimeHandle,
};
pub use style::{
    Color,
    Role,
    Style,
    Theme,
};
pub use surface::{
    Cell,
    Position,
    Row,
    RowBreak,
    Surface,
};
pub use template::{
    TemplateError,
    template,
};
pub use terminal::{
    FALLBACK_WIDTH,
    stderr_is_terminal,
    terminal_width,
    terminal_width_or_default,
};
pub use widget::{
    Grid,
    GridCell,
    InputAnchor,
    Line,
    List,
    Looping,
    ProgressBar,
    RenderCtx,
    Stack,
    Stateful,
    Text,
    TextInput,
    TickInterest,
    Widget,
    WidgetRef,
    WindowedLines,
    widget,
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
