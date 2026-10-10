// SPDX-License-Identifier: EUPL-1.2

/// A terminal colour.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Color {
    /// ANSI colour 0.
    Black,
    /// ANSI colour 1.
    Red,
    /// ANSI colour 2.
    Green,
    /// ANSI colour 3.
    Yellow,
    /// ANSI colour 4.
    Blue,
    /// ANSI colour 5.
    Magenta,
    /// ANSI colour 6.
    Cyan,
    /// ANSI colour 7.
    White,
    /// Bright variant of [`Color::Black`], often rendered as grey.
    BrightBlack,
    /// Bright variant of [`Color::Red`].
    BrightRed,
    /// Bright variant of [`Color::Green`].
    BrightGreen,
    /// Bright variant of [`Color::Yellow`].
    BrightYellow,
    /// Bright variant of [`Color::Blue`].
    BrightBlue,
    /// Bright variant of [`Color::Magenta`].
    BrightMagenta,
    /// Bright variant of [`Color::Cyan`].
    BrightCyan,
    /// Bright variant of [`Color::White`].
    BrightWhite,
    /// An entry of the 256-colour palette.
    Indexed(u8),
    /// A 24-bit colour given as red, green and blue.
    Rgb(u8, u8, u8),
}

impl Color {
    pub(crate) fn push_sgr(self, background: bool, codes: &mut Vec<String>) {
        let (base, extended) = if background { (40, 48) } else { (30, 38) };
        let simple = |offset: u8| vec![(base + offset).to_string()];
        let parts = match self {
            Self::Black => simple(0),
            Self::Red => simple(1),
            Self::Green => simple(2),
            Self::Yellow => simple(3),
            Self::Blue => simple(4),
            Self::Magenta => simple(5),
            Self::Cyan => simple(6),
            Self::White => simple(7),
            Self::BrightBlack => simple(60),
            Self::BrightRed => simple(61),
            Self::BrightGreen => simple(62),
            Self::BrightYellow => simple(63),
            Self::BrightBlue => simple(64),
            Self::BrightMagenta => simple(65),
            Self::BrightCyan => simple(66),
            Self::BrightWhite => simple(67),
            Self::Indexed(index) => vec![extended.to_string(), "5".to_owned(), index.to_string()],
            Self::Rgb(r, g, b) => {
                vec![
                    extended.to_string(),
                    "2".to_owned(),
                    r.to_string(),
                    g.to_string(),
                    b.to_string(),
                ]
            },
        };
        codes.extend(parts);
    }
}

/// Colours and text attributes applied to written cells.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Style {
    /// Foreground colour, or the terminal default when `None`.
    pub fg:            Option<Color>,
    /// Background colour, or the terminal default when `None`.
    pub bg:            Option<Color>,
    /// Bold weight.
    pub bold:          bool,
    /// Reduced intensity.
    pub dim:           bool,
    /// Italic slant.
    pub italic:        bool,
    /// Underline.
    pub underline:     bool,
    /// Strike-through.
    pub strikethrough: bool,
    /// Swapped foreground and background.
    pub reverse:       bool,
}

/// A semantic text role resolved to a [`Style`] through a [`Theme`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    /// Prompt and heading text.
    Prompt,
    /// Ordinary text.
    Normal,
    /// De-emphasised text.
    Dim,
    /// The selected item of a list.
    Selected,
    /// A highlighted search match.
    Match,
    /// Error text.
    Error,
    /// Success text.
    Success,
}

/// Maps each [`Role`] to a concrete [`Style`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Theme {
    prompt:   Style,
    normal:   Style,
    dim:      Style,
    selected: Style,
    matched:  Style,
    error:    Style,
    success:  Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Theme {
    /// The built-in theme.
    pub const DEFAULT: Self = Self {
        prompt:   Style::new().bold(),
        normal:   Style::new(),
        dim:      Style::new().dim(),
        selected: Style::new().reverse(),
        matched:  Style::new().fg(Color::Yellow).bold(),
        error:    Style::new().fg(Color::Red).bold(),
        success:  Style::new().fg(Color::Green).bold(),
    };

    /// The style assigned to `role`.
    pub const fn style(self, role: Role) -> Style {
        match role {
            Role::Prompt => self.prompt,
            Role::Normal => self.normal,
            Role::Dim => self.dim,
            Role::Selected => self.selected,
            Role::Match => self.matched,
            Role::Error => self.error,
            Role::Success => self.success,
        }
    }

    /// Returns this theme with `role` mapped to `style`.
    #[must_use]
    pub const fn with(mut self, role: Role, style: Style) -> Self {
        match role {
            Role::Prompt => self.prompt = style,
            Role::Normal => self.normal = style,
            Role::Dim => self.dim = style,
            Role::Selected => self.selected = style,
            Role::Match => self.matched = style,
            Role::Error => self.error = style,
            Role::Success => self.success = style,
        }
        self
    }
}

impl Style {
    /// The empty style, which leaves the terminal defaults untouched.
    pub const PLAIN: Self = Self::new();

    /// The empty style, equal to [`Style::PLAIN`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg:            None,
            bg:            None,
            bold:          false,
            dim:           false,
            italic:        false,
            underline:     false,
            strikethrough: false,
            reverse:       false,
        }
    }

    /// Sets the foreground colour.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Sets the background colour.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Enables bold weight.
    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Enables reduced intensity.
    #[must_use]
    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    /// Enables italics.
    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Enables underline.
    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Enables strike-through.
    #[must_use]
    pub const fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Swaps foreground and background.
    #[must_use]
    pub const fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    pub(crate) fn sgr(self) -> String {
        if self == Self::default() {
            return "\x1b[0m".to_string();
        }

        let mut codes = vec!["0".to_owned()];
        for (enabled, code) in [
            (self.bold, "1"),
            (self.dim, "2"),
            (self.italic, "3"),
            (self.underline, "4"),
            (self.reverse, "7"),
            (self.strikethrough, "9"),
        ] {
            if enabled {
                codes.push(code.to_owned());
            }
        }
        if let Some(fg) = self.fg {
            fg.push_sgr(false, &mut codes);
        }
        if let Some(bg) = self.bg {
            bg.push_sgr(true, &mut codes);
        }

        format!("\x1b[{}m", codes.join(";"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_non_default_style_resets_before_setting_its_own_attributes() {
        assert_eq!(Style::PLAIN.bold().sgr(), "\x1b[0;1m");
        assert_eq!(Style::PLAIN.fg(Color::Red).sgr(), "\x1b[0;31m");
        assert_eq!(Style::default().sgr(), "\x1b[0m");
    }

    #[test]
    fn new_is_the_plain_style() {
        assert_eq!(Style::new(), Style::PLAIN);
        assert_eq!(Style::new(), Style::default());
    }

    #[test]
    fn attributes_emit_their_sgr_codes_in_a_fixed_order() {
        let style = Style::new()
            .strikethrough()
            .underline()
            .italic()
            .reverse()
            .dim()
            .bold();
        assert_eq!(style.sgr(), "\x1b[0;1;2;3;4;7;9m");
    }

    #[test]
    fn bright_indexed_and_rgb_colours_use_their_extended_codes() {
        assert_eq!(Style::new().fg(Color::BrightRed).sgr(), "\x1b[0;91m");
        assert_eq!(Style::new().bg(Color::BrightWhite).sgr(), "\x1b[0;107m");
        assert_eq!(
            Style::new().fg(Color::Indexed(200)).sgr(),
            "\x1b[0;38;5;200m"
        );
        assert_eq!(Style::new().bg(Color::Indexed(7)).sgr(), "\x1b[0;48;5;7m");
        assert_eq!(
            Style::new()
                .fg(Color::Rgb(1, 2, 3))
                .bg(Color::Rgb(255, 0, 128))
                .sgr(),
            "\x1b[0;38;2;1;2;3;48;2;255;0;128m"
        );
    }
}
