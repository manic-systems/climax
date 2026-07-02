// SPDX-License-Identifier: EUPL-1.2

//! widgets for bang.

mod action;
mod event;
mod ids;
mod output;
mod render;
mod session;
mod value;
mod widget;
pub mod widgets;

pub use action::{ActionBinding, ActionLayer};
pub use event::{Event, Key, KeyEvent, Modifiers};
pub use ids::{CursorAnchor, ViewId, WidgetId};
pub use output::{OutputFormat, escape_json, format_json, format_output, format_text};
pub use render::{
    CalendarDay, CalendarView, CalendarWeek, CursorPlacement, ListRow, ListView, Role, Span,
    TextInputView, View, ViewContext, plain_snapshot,
};
pub use session::{Session, SessionStatus};
pub use value::{Date, Number, Value};
pub use widget::{Context, FocusTarget, Reaction, Widget};
