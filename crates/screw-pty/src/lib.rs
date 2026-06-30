// SPDX-License-Identifier: EUPL-1.2

//! PTY screen adapter for screw surfaces.

use screw::{
    RenderCtx,
    Style,
    Surface,
    Widget,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PtyFrame {
    lines: Vec<String>,
}

impl PtyFrame {
    #[must_use]
    pub const fn new() -> Self {
        Self { lines: Vec::new() }
    }

    #[must_use]
    pub fn from_lines(lines: impl Into<Vec<String>>) -> Self {
        Self {
            lines: lines.into(),
        }
    }

    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PtyScreen {
    frame: PtyFrame,
}

impl PtyScreen {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frame: PtyFrame::new(),
        }
    }

    #[must_use]
    pub fn frame(&self) -> &PtyFrame {
        &self.frame
    }

    pub fn replace_frame(&mut self, frame: PtyFrame) {
        self.frame = frame;
    }

    pub fn push_lossy(&mut self, bytes: &[u8]) {
        for line in String::from_utf8_lossy(bytes).lines() {
            self.frame.push_line(line);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PtyWidget {
    frame: PtyFrame,
    style: Style,
}

impl PtyWidget {
    #[must_use]
    pub const fn new(frame: PtyFrame) -> Self {
        Self {
            frame,
            style: Style::PLAIN,
        }
    }

    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub const fn frame(&self) -> &PtyFrame {
        &self.frame
    }
}

impl Widget for PtyWidget {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        for (index, line) in self.frame.lines().iter().enumerate() {
            if index > 0 {
                out.newline();
            }
            out.write(line, self.style);
        }
    }
}
