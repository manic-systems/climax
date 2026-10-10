// SPDX-License-Identifier: EUPL-1.2

//! screw: a differential terminal renderer with composable widgets.
//!
//! a tree of [`Widget`]s draws into a [`Surface`] of styled rows. the
//! [`Renderer`] remembers the previous frame and writes only the sequences that
//! turn it into the next one
//!
//! ```
//! use std::sync::{
//!     Arc,
//!     atomic::{
//!         AtomicU64,
//!         Ordering,
//!     },
//! };
//!
//! use screw::{
//!     RenderCtx,
//!     Runtime,
//!     Style,
//!     Surface,
//!     Widget,
//!     widget,
//! };
//!
//! #[derive(Clone)]
//! struct Download(Arc<AtomicU64>);
//!
//! impl Widget for Download {
//!     fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
//!         let received = self.0.load(Ordering::Relaxed);
//!         out.write(format!("{received} bytes"), Style::new());
//!     }
//! }
//!
//! # fn main() -> std::io::Result<()> {
//! let download = Download(Arc::new(AtomicU64::new(0)));
//! let runtime = Runtime::new(Vec::new(), widget(download.clone()))
//!     .viewport(40, 1)
//!     .start();
//!
//! download.0.store(300, Ordering::Relaxed);
//! runtime.mark_dirty()?;
//! assert!(!runtime.finish()?.is_empty());
//! assert_eq!(screw::render_plain(&download), "300 bytes");
//! # Ok(())
//! # }
//! ```
//!
//! # Render contract
//!
//! - write with [`Surface::write`], each call with its own [`Style`]. a newline
//!   starts a new row, and a widget inside a [`Line`] continues the row, so
//!   measure from [`Surface::current_col`]
//! - [`RenderCtx::available_columns`] is `None` when the width is unknown
//! - rendering runs every frame and must not block
//!
//! text is laid out per grapheme cluster, each taking the columns
//! `unicode-width` reports for it whole. terminals that do not shape VS16 or
//! ZWJ sequences draw them wider
//!
//! [`widget`] erases a widget into a `Send + Sync` [`WidgetRef`]. widgets
//! holding `Rc` or local borrows use [`local_widget`], and cannot be started on
//! a thread. a [`Role`] resolved by a [`Theme`] recolours widgets without
//! touching them, and [`screw!`] and [`layout`] build a [`Stack`] of [`Line`]s
//!
//! the `examples` directory has `cade`, `gallery` and `layers`

#![warn(missing_docs)]
#![expect(
    clippy::non_ascii_literal,
    reason = "terminal glyphs read better as themselves than as escapes"
)]

extern crate alloc;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod geometry;
mod layers;
mod layout;
mod measure;
mod plain;
mod renderer;
mod runtime;
mod style;
mod surface;
mod sync;
mod template;
mod terminal;
mod viewport;
mod widget;

pub use geometry::{
    Insets,
    Rect,
    Size,
    Viewport,
};
pub use layers::{
    Edge,
    Floating,
    Layers,
};
pub use layout::{
    LayoutBuilder,
    layout,
    local_layout,
};
pub use measure::{
    Align,
    pad,
    truncate,
    width,
};
pub use plain::{
    render_plain,
    render_plain_with_frame,
    render_plain_with_frame_and_theme,
    write_plain,
};
pub use renderer::{
    CursorVisibility,
    LayoutMode,
    RenderStats,
    Renderer,
    colors_enabled,
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
    CursorMerge,
    Fill,
    Position,
    Row,
    RowBreak,
    Surface,
};
pub use template::{
    TemplateError,
    local_template,
    template,
};
pub use viewport::{
    VerticalViewport,
    ViewportReport,
    ViewportReportHandle,
};
pub use widget::{
    CellOverflow,
    InputAnchor,
    Line,
    List,
    LocalWidgetRef,
    Looping,
    ProgressBar,
    RenderCtx,
    Span,
    Spans,
    Stack,
    Stateful,
    Table,
    Text,
    TextInput,
    TickInterest,
    VerticalSize,
    Widget,
    WidgetRef,
    WindowedLines,
    combine_tick_interest,
    local_widget,
    widget,
};

/// builds a [`Stack`] from a template, each newline starting a row and each
/// `{name}` replaced by the widget given as `name = widget`
///
/// `{{` and `}}` are literal braces, widgets must be `Send + Sync + 'static`,
/// and a bad template panics, so use [`template`] to handle the error
///
/// ```
/// use screw::{
///     Looping,
///     ProgressBar,
///     screw,
/// };
///
/// let ui = screw!(
///     "{spinner} working\n{bar}",
///     spinner = Looping::new(["-", "|"]),
///     bar = ProgressBar::new(4),
/// );
/// assert_eq!(screw::render_plain(&ui), "- working\n[────]");
/// ```
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

/// [`screw!`] for widgets that stay on the current thread, the result is a
/// local [`Stack`]
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
