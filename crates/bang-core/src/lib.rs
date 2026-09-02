// SPDX-License-Identifier: EUPL-1.2

//! Widget state and interaction machinery for Bang.
//!
//! Most applications should use the user-facing `bang` crate. Renderer and
//! terminal integrations consume the deliberately separate [`adapter`]
//! contract.

mod action;
mod event;
mod ids;
mod output;
mod render;
mod session;
mod value;
mod widget;
pub mod widgets;

/// Renderer-neutral view descriptions shared with Bang renderer adapters.
///
/// This module is an adapter contract, not Bang's friendly prompt API.
/// Widgets produce a [`View`](adapter::View), and renderer crates translate
/// that view into their own output model. Keeping these types free of terminal
/// I/O and renderer-specific styling lets adapters evolve independently of
/// widget state and event handling.
///
/// Adapter crates exhaustively interpret [`View`](adapter::View). Adding a new
/// semantic view is therefore a deliberate breaking change which produces a
/// compile error in adapters instead of silently dropping user-visible data.
/// Application code should prefer prompt builders and built-in widgets instead
/// of constructing or inspecting these values directly.
pub mod adapter {
    pub use crate::render::{
        CalendarDay, CalendarView, CalendarWeek, CursorPlacement, ListPresentation, ListRow,
        ListView, Presentation, Role, Span, TextInputView, View, ViewContext, plain_snapshot,
    };
}

pub use action::{ActionBinding, ActionLayer};
// Transitional aliases for workspace crates which predate the explicit
// adapter namespace. New adapter code should import from `adapter`.
#[doc(hidden)]
pub use adapter::{
    CalendarDay, CalendarView, CalendarWeek, CursorPlacement, ListPresentation, ListRow, ListView,
    Presentation, Role, Span, TextInputView, View, ViewContext, plain_snapshot,
};
pub use event::{Event, Key, KeyEvent, Modifiers};
pub use ids::{CursorAnchor, ViewId, WidgetId};
pub use output::{OutputFormat, escape_json, format_json, format_output, format_text};
pub use session::{Session, SessionStatus};
pub use value::{Date, Number, Value};
pub use widget::{Context, FocusTarget, Reaction, Widget};
