#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Color {
    pub(crate) const fn fg_code(self) -> u8 {
        match self {
            Self::Black => 30,
            Self::Red => 31,
            Self::Green => 32,
            Self::Yellow => 33,
            Self::Blue => 34,
            Self::Magenta => 35,
            Self::Cyan => 36,
            Self::White => 37,
        }
    }

    pub(crate) const fn bg_code(self) -> u8 {
        match self {
            Self::Black => 40,
            Self::Red => 41,
            Self::Green => 42,
            Self::Yellow => 43,
            Self::Blue => 44,
            Self::Magenta => 45,
            Self::Cyan => 46,
            Self::White => 47,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Style {
    pub fg:      Option<Color>,
    pub bg:      Option<Color>,
    pub bold:    bool,
    pub dim:     bool,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Role {
    Prompt,
    Normal,
    Dim,
    Selected,
    Match,
    Error,
    Success,
}

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
    pub const DEFAULT: Self = Self {
        prompt:   Style::PLAIN.bold(),
        normal:   Style::PLAIN,
        dim:      Style::PLAIN.dim(),
        selected: Style::PLAIN.reverse(),
        matched:  Style::PLAIN.fg(Color::Yellow).bold(),
        error:    Style::PLAIN.fg(Color::Red).bold(),
        success:  Style::PLAIN.fg(Color::Green).bold(),
    };

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
    pub const PLAIN: Self = Self {
        fg:      None,
        bg:      None,
        bold:    false,
        dim:     false,
        reverse: false,
    };

    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    #[must_use]
    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    #[must_use]
    pub const fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    pub(crate) fn sgr(self) -> String {
        if self == Self::default() {
            return "\x1b[0m".to_string();
        }

        let mut codes = Vec::new();
        if self.bold {
            codes.push(1);
        }
        if self.dim {
            codes.push(2);
        }
        if self.reverse {
            codes.push(7);
        }
        if let Some(fg) = self.fg {
            codes.push(fg.fg_code());
        }
        if let Some(bg) = self.bg {
            codes.push(bg.bg_code());
        }

        let codes = codes
            .into_iter()
            .map(|code| code.to_string())
            .collect::<Vec<_>>()
            .join(";");
        format!("\x1b[{codes}m")
    }
}
