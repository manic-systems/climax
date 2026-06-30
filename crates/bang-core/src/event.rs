// SPDX-License-Identifier: EUPL-1.2

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// key press
    Key(KeyEvent),
    /// text paste
    Paste(String),
    /// term resize
    Resize { cols: u16, rows: u16 },
    /// animation tick
    Tick,
}

impl Event {
    #[must_use]
    pub const fn key(key: Key) -> Self {
        Self::Key(KeyEvent::new(key))
    }

    #[must_use]
    pub const fn char(value: char) -> Self {
        Self::key(Key::Char(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub key:       Key,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::empty(),
        }
    }

    #[must_use]
    pub const fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// usable keys
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Tab,
    Backtab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const SHIFT: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const CONTROL: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

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
