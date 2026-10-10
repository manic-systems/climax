// SPDX-License-Identifier: EUPL-1.2

//! A differential terminal renderer with composable widgets.
//!
//! screw draws a tree of [`Widget`]s into a [`Surface`] of styled rows, remembers the previous
//! frame and writes only the control sequences needed to turn it into the next one. That keeps
//! animated status lines, spinners and progress bars cheap and free of flicker.
//!
//! # Writing a widget
//!
//! A widget implements [`Widget::render`] and writes into the surface it is given. State that
//! changes while the widget is on screen lives behind a shared handle, so the application can
//! update it from any thread and then tell the runtime to redraw.
//!
//! ```
//! use std::sync::{
//!     Arc,
//!     atomic::{AtomicU64, Ordering},
//! };
//!
//! use screw::{Color, RenderCtx, Runtime, Style, Surface, Widget, widget};
//!
//! #[derive(Clone)]
//! struct Download {
//!     received: Arc<AtomicU64>,
//!     total: u64,
//! }
//!
//! impl Widget for Download {
//!     fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
//!         let received = self.received.load(Ordering::Relaxed);
//!         let style = if received >= self.total {
//!             Style::new().fg(Color::Green)
//!         } else {
//!             Style::new()
//!         };
//!         out.write(format!("{received}/{} bytes", self.total), style);
//!     }
//! }
//!
//! # fn main() -> std::io::Result<()> {
//! let download = Download {
//!     received: Arc::new(AtomicU64::new(0)),
//!     total: 300,
//! };
//! let runtime = Runtime::new(Vec::new(), widget(download.clone()))
//!     .viewport(40, 1)
//!     .start();
//!
//! download.received.store(300, Ordering::Relaxed);
//! runtime.mark_dirty()?;
//!
//! let output = runtime.finish()?;
//! assert!(!output.is_empty());
//! assert_eq!(screw::render_plain(&download), "300/300 bytes");
//! # Ok(())
//! # }
//! ```
//!
//! # The render contract
//!
//! - Write text with [`Surface::write`], as many times as needed, each time with its own
//!   [`Style`]. A tab expands to spaces up to the next multiple of eight columns, and control
//!   characters other than tab and newline are dropped.
//! - A newline in the text or a call to [`Surface::newline`] starts a new row. A widget that
//!   follows another inside a [`Line`] continues on the same row, so measure from
//!   [`Surface::current_col`] and not from zero.
//! - [`RenderCtx::available_columns`] is `None` when the width is unknown, for example when
//!   output is not a terminal. Do not assume a width.
//! - Rendering runs on every frame and must not block. Cache expensive work outside the widget.
//! - Place the terminal cursor with [`Surface::set_cursor`] only when the widget owns input.
//!
//! [`width`], [`truncate`] and [`pad`] measure text exactly as [`Surface::write`] lays it out,
//! which is what a custom column layout needs. [`Span`] and [`Spans`] mix styles on one row, and
//! [`Table`] aligns columns.
//!
//! # Cluster widths
//!
//! Text is laid out per extended grapheme cluster, and each cluster takes the columns that the
//! `unicode-width` crate reports for it as a whole. A variation selector, a ZWJ sequence or a pair
//! of regional indicators is therefore one cell, and a combining mark adds nothing. Terminals
//! disagree about some of these (a terminal that does not shape VS16 or ZWJ sequences draws them
//! wider), so a row containing such text can drift on those terminals. Cluster text written by
//! separate [`Surface::write`] calls is joined and measured again.
//!
//! # Redrawing
//!
//! A widget reports how often it changes through [`Widget::tick_interest`].
//!
//! - [`TickInterest::Never`] means it only changes when the application calls `mark_dirty`
//!   on the runtime. This is the default.
//! - [`TickInterest::EveryFrame`] redraws at the runtime frame rate, and [`RenderCtx::frame`]
//!   advances each time. Use it for animation such as a [`Looping`] spinner.
//! - [`TickInterest::Every`] redraws at most that often.
//!
//! Composite widgets combine their children with [`combine_tick_interest`].
//!
//! # Runtime and Renderer
//!
//! A [`Renderer`] is the synchronous core. It draws one widget to one writer and diffs against
//! what it drew last, and the caller decides when to draw.
//!
//! A [`Runtime`] owns a renderer and a root widget and adds frame pacing. Call [`Runtime::tick`]
//! from your own loop, or [`Runtime::start`] to move it onto a thread and drive it through a
//! [`LiveRuntime`]. [`Runtime::auto`] takes an `interactive` flag from the caller and
//! chooses live output when it is set and plain text written once at the end otherwise, as
//! suits a pipe or a CI log. [`Runtime::stderr_auto`] sets the flag from whether standard error
//! is a terminal.
//!
//! # Threads
//!
//! [`widget`] erases a widget into a [`WidgetRef`], which is `Send + Sync` so a runtime thread
//! can render it. Widgets that hold `Rc` or borrow local data use [`local_widget`] and
//! [`LocalWidgetRef`] instead. The `local_` variants of [`layout`], [`template`] and
//! [`Layers`] exist for the same reason, and a local tree can be driven with a [`Renderer`] or a
//! synchronous [`Runtime`] but cannot be started on a thread.
//!
//! # Styles, roles and themes
//!
//! A [`Style`] is a concrete set of colours and attributes. A [`Role`] is a semantic name such
//! as [`Role::Error`] that a [`Theme`] resolves to a style at draw time, so one application can
//! be recoloured without touching its widgets. Built-in widgets accept either.
//!
//! # Composing
//!
//! [`screw!`] builds a [`Stack`] of [`Line`]s from a template with named slots, and [`layout`]
//! builds the same thing from code. [`Layers`] draws floating panes over a base widget.
//!
//! # Examples
//!
//! The [`examples`] directory has a build-progress display (`cade`), a tour of the built-in
//! widgets (`gallery`) and floating panes (`layers`). Run one with
//! `cargo run -p screw --example gallery`.
//!
//! [`examples`]: https://github.com/manic-systems/climax/tree/main/crates/screw/examples

#![warn(missing_docs)]

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

pub use geometry::{Insets, Rect, Size, Viewport};
pub use layers::{Edge, Floating, Layers};
pub use layout::{LayoutBuilder, layout, local_layout};
pub use measure::{Align, pad, truncate, width};
pub use plain::{
    render_plain, render_plain_with_frame, render_plain_with_frame_and_theme, write_plain,
};
pub use renderer::{CursorVisibility, LayoutMode, RenderStats, Renderer, colors_enabled};
pub use runtime::{
    AutoRuntime, AutoRuntimeBuilder, LiveRuntime, PlainRuntime, Runtime, RuntimeHandle,
};
pub use style::{Color, Role, Style, Theme};
pub use surface::{Cell, CursorMerge, Fill, Position, Row, RowBreak, Surface};
pub use template::{TemplateError, local_template, template};
pub use viewport::{VerticalViewport, ViewportReport, ViewportReportHandle};
pub use widget::{
    CellOverflow, InputAnchor, Line, List, LocalWidgetRef, Looping, ProgressBar, RenderCtx,
    Span, Spans, Stack, Stateful, Table, Text, TextInput, TickInterest, VerticalSize, Widget,
    WidgetRef, WindowedLines, combine_tick_interest, local_widget, widget,
};

/// Builds a [`Stack`] from a template, binding each slot to a widget.
///
/// Each newline in the template starts a row and `{name}` is replaced by the widget given as
/// `name = widget`. Use `{{` and `}}` for literal braces. The widgets must be `Send + Sync` and
/// `'static`. An invalid template panics, so use [`template`] to handle the error instead.
///
/// ```
/// use screw::{Looping, ProgressBar, screw};
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

/// Like [`screw!`] for widgets that remain on the current thread.
///
/// The widgets need not be `Send` or `Sync`, and the result is a local [`Stack`].
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
