// SPDX-License-Identifier: EUPL-1.2

//! Widget state and interaction machinery for Bang.
//!
//! Bang is an interactive layer over `screw`. A [`Widget`] is a `screw`
//! widget that also handles input, so it renders into the cell grid of a
//! `screw` `Surface` and any terminal renderer can show it by copying cells.
//! A list widget measures its own layout while it renders and keeps it for the
//! next event, so paging follows the rows that physically fit.
//!
//! Most applications should use the user-facing `bang` crate.

#![warn(missing_docs)]

mod event;
mod ids;
mod output;
mod session;
mod value;
mod widget;
/// The built-in widgets.
pub mod widgets;

pub use event::{Event, Key, KeyEvent, Modifiers};
pub use ids::WidgetId;
pub use output::{OutputFormat, escape_json, format_json, format_output, format_text};
pub use session::{Session, SessionReaction, SessionStatus};
pub use value::{Date, Number, Value};
pub use widget::{Context, FocusTarget, Reaction, Widget};
