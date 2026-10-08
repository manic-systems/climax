// SPDX-License-Identifier: EUPL-1.2

/// An input or lifecycle event delivered to a widget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// key press
    Key(KeyEvent),
    /// text paste
    Paste(String),
    /// A complete terminal escape sequence not understood by the decoder.
    ///
    /// The bytes are surfaced for application policy rather than being
    /// mistaken for the standalone Escape key. They must not be written back
    /// to a terminal verbatim because they may themselves be terminal control
    /// sequences.
    UnknownEscape(Vec<u8>),
    /// term resize
    Resize {
        /// The new width in columns.
        cols: u16,
        /// The new height in rows.
        rows: u16,
    },
    /// animation tick
    Tick,
}

impl Event {
    /// A key press with no modifiers.
    #[must_use]
    pub const fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    /// A press of the character key `value` with no modifiers.
    #[must_use]
    pub const fn char(value: char) -> Self {
        Self::key(Key::Char(value))
    }
}

/// A key press together with the modifiers held.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    /// The key that was pressed.
    pub key:       Key,
    /// The modifiers held while it was pressed.
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// A press of `key` with no modifiers.
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::empty(),
        }
    }

    /// A press of `key` with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// usable keys
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// Enter or Return.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Shift and Tab together.
    Backtab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Cursor up.
    Up,
    /// Cursor down.
    Down,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// F1-F5, the only function keys the Linux console reports through
    /// `ESC [ [ A`..`E` rather than the xterm `ESC O` / `ESC [ ... ~` forms.
    Function(u8),
}

/// A set of held modifier keys.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    /// Shift.
    pub const SHIFT: Self = Self(1 << 0);
    /// Alt, also called Option.
    pub const ALT: Self = Self(1 << 1);
    /// Control.
    pub const CONTROL: Self = Self(1 << 2);
    /// Super, also called Command or Windows.
    pub const SUPER: Self = Self(1 << 3);

    /// No modifiers.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every modifier in `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The modifiers as a bit set, with `SHIFT` as bit zero.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl core::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
