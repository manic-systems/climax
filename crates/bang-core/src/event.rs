// SPDX-License-Identifier: EUPL-1.2

use core::ops::{
    BitOr,
    BitOrAssign,
};

/// an input or lifecycle event delivered to a widget
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// key press
    Key(KeyEvent),
    /// text paste
    Paste(String),
    /// bytes the decoder did not understand, a whole escape sequence or one
    /// invalid byte, never write them back to a terminal
    UnknownEscape(Vec<u8>),
    /// term resize
    Resize {
        /// new width in columns
        cols: u16,
        /// new height in rows
        rows: u16,
    },
    /// animation tick
    Tick,
    /// move a list to the row at index `n` among the rows it shows, sent by
    /// scripted input and never by a key, a widget that is no list or has no
    /// such row ignores it
    Row(usize),
}

impl Event {
    /// a key press with no modifiers
    #[must_use]
    pub const fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    /// a press of the character key `value` with no modifiers
    #[must_use]
    pub const fn char(value: char) -> Self {
        Self::key(Key::Char(value))
    }
}

/// a key press together with the modifiers held
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    /// the key that was pressed
    pub key:       Key,
    /// the modifiers held while it was pressed
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// a press of `key` with no modifiers
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::empty(),
        }
    }

    /// a press of `key` with `modifiers` held
    #[must_use]
    pub const fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// usable keys
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Key {
    /// a printable character
    Char(char),
    /// Enter or Return
    Enter,
    /// Escape
    Esc,
    /// Tab
    Tab,
    /// Shift and Tab together
    Backtab,
    /// Backspace
    Backspace,
    /// Delete
    Delete,
    /// cursor up
    Up,
    /// cursor down
    Down,
    /// cursor left
    Left,
    /// cursor right
    Right,
    /// Home
    Home,
    /// End
    End,
    /// Page Up
    PageUp,
    /// Page Down
    PageDown,
    /// Insert
    Insert,
    /// F1 to F12
    Function(u8),
}

/// a set of held modifier keys
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    /// Shift
    pub const SHIFT: Self = Self(1 << 0);
    /// Alt, also called Option
    pub const ALT: Self = Self(1 << 1);
    /// Control
    pub const CONTROL: Self = Self(1 << 2);
    /// Super, also called Command or Windows
    pub const SUPER: Self = Self(1 << 3);

    /// no modifiers
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// whether every modifier in `other` is held
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// the modifiers as a bit set, with `SHIFT` as bit zero
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
