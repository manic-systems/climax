// SPDX-License-Identifier: EUPL-1.2

//! bang-core: widget state and interaction machinery for bang
//!
//! applications should use the `bang` crate instead

#![warn(missing_docs)]
#![cfg_attr(
    test,
    expect(
        clippy::non_ascii_literal,
        reason = "terminal glyphs read better as themselves than as escapes"
    )
)]

extern crate alloc;

mod event;
mod ids;
mod output;
mod session;
mod value;
mod widget;

pub use event::{
    Event,
    Key,
    KeyEvent,
    Modifiers,
};
pub use ids::WidgetId;
pub use output::{
    OutputFormat,
    escape_json,
    format_json,
    format_output,
    format_text,
};
pub use session::{
    Session,
    SessionReaction,
    SessionStatus,
};
pub use value::{
    Date,
    Number,
    Value,
};
pub use widget::{
    Context,
    FocusTarget,
    Reaction,
    Widget,
};
