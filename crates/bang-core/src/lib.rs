// SPDX-License-Identifier: EUPL-1.2

//! bang-core: widget state and interaction machinery for bang
//!
//! applications should use the `bang` crate instead
//!
//! a widget implements [`InputWidget`] and a [`Session`] drives it from
//! events, which a test can script without a terminal
//!
//! ```
//! use bang_core::{
//!     Event,
//!     Key,
//!     Session,
//!     SessionStatus,
//!     Value,
//!     widgets::TextInput,
//! };
//!
//! let mut session = Session::new(TextInput::new("name"));
//! for event in Event::text("ada").chain([Event::key(Key::Enter)]) {
//!     session.handle(event);
//! }
//! assert_eq!(
//!     session.status(),
//!     &SessionStatus::Submitted(Value::from("ada".to_owned()))
//! );
//! ```

#![warn(missing_docs)]
#![expect(
    clippy::non_ascii_literal,
    reason = "terminal glyphs read better as themselves than as escapes"
)]

#[cfg(not(unix))]
compile_error!("bang-core supports unix targets only");

extern crate alloc;

mod action;
mod error;
mod event;
mod ids;
mod output;
mod session;
mod value;
mod widget;
/// the built-in widgets
pub mod widgets;

pub use action::{
    ActionBinding,
    ActionLayer,
};
pub use error::InvalidConfiguration;
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
    ParseDateError,
    Value,
};
pub use widget::{
    Context,
    FocusTarget,
    InputWidget,
    Reaction,
};
